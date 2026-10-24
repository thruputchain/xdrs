//! Tests that verify tracing instrumentation produces expected spans and events.

use std::io;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use thruput_rpc_driver::{
    CallBuf, ProgVers, RpcError, RpcServer, connect_client, handler::RpcHandler,
    handler::ServiceMap,
};
use tokio::net::TcpListener;
use tracing_subscriber::fmt::format::FmtSpan;

/// tracing caches each callsite's interest process-wide. While another test
/// in this binary runs without a subscriber (or with a different one), that
/// cache can record a span as disabled for the subscriber a test installed,
/// and the test then sees, say, `rpc_call` without its `rpc_connection`
/// parent. Each test holds this for its whole run.
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn call_with(payload: &[u8]) -> CallBuf {
    let mut call = CallBuf::new();
    call.extend_from_slice(payload);
    call
}

/// Thread-safe buffer that captures tracing output for assertions.
#[derive(Clone)]
struct CaptureBuffer(Arc<Mutex<Vec<u8>>>);

impl CaptureBuffer {
    fn new() -> Self {
        Self(Arc::new(Mutex::new(Vec::new())))
    }

    fn contents(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).to_string()
    }
}

impl io::Write for CaptureBuffer {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for CaptureBuffer {
    type Writer = Self;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// Poll the capture buffer until `needle` appears, with a timeout.
/// Avoids flaky sleep-based assertions on slow CI runners.
async fn wait_for(buf: &CaptureBuffer, needle: &str) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        if buf.contents().contains(needle) {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for {needle:?} in output:\n{}",
            buf.contents()
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

struct EchoService;

impl RpcHandler for EchoService {
    fn dispatch(
        &self,
        proc: u32,
        data: &[u8],
        reply: thruput_rpc_api::ReplyBuf,
        slot: thruput_rpc_api::FutureSlot,
    ) -> Result<thruput_rpc_api::RpcFuture<'_>, RpcError> {
        // Pins down the body's error type for `?`.
        fn body<F: std::future::Future<Output = Result<Vec<u8>, RpcError>>>(f: F) -> F {
            f
        }
        let data = data.to_vec();
        Ok(slot.put(async move {
            let data: &[u8] = &data;
            let result = body(async {
                match proc {
                    0 => Ok(vec![]),
                    1 => Ok(data.to_vec()),
                    _ => Err(RpcError::InvalidProcedure),
                }
            })
            .await;
            result.map(|bytes| reply.with(&bytes))
        }))
    }

    fn prog_vers(&self) -> ProgVers {
        ProgVers::new(100, 1)
    }
}

/// Verify that connection spans and rpc_call spans appear with expected fields.
#[tokio::test]
async fn test_tracing_connection_and_call_spans() {
    let _serial = SERIAL.lock().await;
    let buf = CaptureBuffer::new();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_writer(buf.clone())
        .with_ansi(false)
        .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE)
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);

    // Start server
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let mut services = ServiceMap::new();
    services.add_service(Box::new(EchoService));
    let server = RpcServer::new(listener, services);
    server.start().await;

    // Connect and make a call
    let client = connect_client(&addr).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    let response = client
        .send_call(ProgVers::new(100, 1), 1, call_with(b"hello"))
        .await
        .unwrap();
    assert_eq!(response, b"hello");

    // Null call
    let response = client
        .send_call(ProgVers::new(100, 1), 0, call_with(b""))
        .await
        .unwrap();
    assert_eq!(response, b"");

    // Poison the client to close the connection — this drops the connection span.
    client.poison().await;

    // Wait for the server-side span to close (server detects EOF and drops its span).
    wait_for(&buf, r#"role="server""#).await;

    let output = buf.contents();

    // -- Connection spans --
    assert!(
        output.contains("rpc_connection") && output.contains(r#"role="server""#),
        "expected server connection span in output:\n{output}"
    );
    assert!(
        output.contains("rpc_connection") && output.contains(r#"role="client""#),
        "expected client connection span in output:\n{output}"
    );
    assert!(
        output.contains(&format!("peer={addr}")),
        "expected peer address {addr} in output:\n{output}"
    );

    // -- RPC call spans --
    assert!(
        output.contains("rpc_call"),
        "expected rpc_call span in output:\n{output}"
    );
    assert!(
        output.contains("prog=100"),
        "expected prog=100 in output:\n{output}"
    );
    assert!(
        output.contains("vers=1"),
        "expected vers=1 in output:\n{output}"
    );
    assert!(
        output.contains("proc=1"),
        "expected proc=1 in output:\n{output}"
    );
    assert!(
        output.contains("xid="),
        "expected xid field in output:\n{output}"
    );
}

/// Verify that rpc_call spans still appear for failed RPC calls.
#[tokio::test]
async fn test_tracing_spans_for_failed_calls() {
    let _serial = SERIAL.lock().await;
    let buf = CaptureBuffer::new();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_writer(buf.clone())
        .with_ansi(false)
        .with_span_events(FmtSpan::NEW)
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let mut services = ServiceMap::new();
    services.add_service(Box::new(EchoService));
    let server = RpcServer::new(listener, services);
    server.start().await;

    let client = connect_client(&addr).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Call a non-existent program — this produces a normal RPC error response,
    // not a tracing error. But the rpc_call span should still appear on both sides.
    let err = client
        .send_call(ProgVers::new(999, 1), 0, call_with(b""))
        .await
        .unwrap_err();
    assert_eq!(err, RpcError::ProgUnavail);

    // Call an invalid procedure — same, produces RPC error with rpc_call spans
    let err = client
        .send_call(ProgVers::new(100, 1), 99, call_with(b""))
        .await
        .unwrap_err();
    assert_eq!(err, RpcError::InvalidProcedure);

    tokio::time::sleep(Duration::from_millis(50)).await;

    let output = buf.contents();

    // Client-side rpc_call spans should appear for the failed calls too
    assert!(
        output.contains("prog=999"),
        "expected prog=999 in rpc_call span:\n{output}"
    );
    assert!(
        output.contains("proc=99"),
        "expected proc=99 in rpc_call span:\n{output}"
    );
}

/// Verify that the rpc_call span is nested inside rpc_connection span.
/// With the fmt subscriber, child spans appear as context on the same line.
#[tokio::test]
async fn test_tracing_span_nesting() {
    let _serial = SERIAL.lock().await;
    let buf = CaptureBuffer::new();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_writer(buf.clone())
        .with_ansi(false)
        .with_span_events(FmtSpan::NEW)
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let mut services = ServiceMap::new();
    services.add_service(Box::new(EchoService));
    let server = RpcServer::new(listener, services);
    server.start().await;

    let client = connect_client(&addr).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    client
        .send_call(ProgVers::new(100, 1), 1, call_with(b"test"))
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    let output = buf.contents();

    // The fmt subscriber shows parent spans as context. A line containing
    // rpc_call should also contain the parent rpc_connection span.
    // Verify both client-side and server-side rpc_call spans are nested.
    let has_client_nested = output.lines().any(|line| {
        line.contains("rpc_call")
            && line.contains("rpc_connection")
            && line.contains(r#"role="client""#)
    });
    assert!(
        has_client_nested,
        "expected client rpc_call nested inside rpc_connection:\n{output}"
    );

    let has_server_nested = output.lines().any(|line| {
        line.contains("rpc_call")
            && line.contains("rpc_connection")
            && line.contains(r#"role="server""#)
    });
    assert!(
        has_server_nested,
        "expected server rpc_call nested inside rpc_connection:\n{output}"
    );
}

/// Verify that RPC calls work without any tracing subscriber installed.
#[tokio::test]
async fn test_no_subscriber() {
    let _serial = SERIAL.lock().await;
    // No subscriber installed — tracing macros are no-ops, no panics.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let mut services = ServiceMap::new();
    services.add_service(Box::new(EchoService));
    let server = RpcServer::new(listener, services);
    server.start().await;

    let client = connect_client(&addr).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Should work without any subscriber — no panics, no output
    let response = client
        .send_call(ProgVers::new(100, 1), 1, call_with(b"hello"))
        .await
        .unwrap();
    assert_eq!(response, b"hello");
}

/// Verify INFO-level subscriber does not see DEBUG-level rpc_call spans.
///
/// The key assertion is that rpc_call (DEBUG) does NOT appear when the
/// subscriber is set to INFO. We verify the subscriber is active by
/// waiting for the ERROR events that fire on connection close.
#[tokio::test(flavor = "current_thread")]
async fn test_info_level_filters_rpc_calls() {
    let _serial = SERIAL.lock().await;
    let buf = CaptureBuffer::new();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_writer(buf.clone())
        .with_ansi(false)
        .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE)
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let mut services = ServiceMap::new();
    services.add_service(Box::new(EchoService));
    let server = RpcServer::new(listener, services);
    server.start().await;

    let client = connect_client(&addr).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    client
        .send_call(ProgVers::new(100, 1), 1, call_with(b"hello"))
        .await
        .unwrap();

    // Close the connection to flush events.
    client.poison().await;

    // Wait for the server-side error event to confirm subscriber is active
    // and connection lifecycle is complete.
    wait_for(&buf, "read task failed").await;

    let output = buf.contents();

    // RPC call spans are DEBUG — should NOT appear at INFO level.
    // This is the key assertion: the filter is working.
    assert!(
        !output.contains("rpc_call"),
        "rpc_call should not appear at INFO level:\n{output}"
    );
}
