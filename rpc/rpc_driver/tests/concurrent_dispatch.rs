//! Tests for the single ready-inline / suspended-task dispatch path.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use rpc_driver::{
    CallBuf, ConnectionConfig, ProgVers, RpcError, RpcServer, connect_client_with_config,
    handler::RpcHandler, handler::ServiceMap,
};

const ECHO_PROG: ProgVers = ProgVers::new(1, 1);
const CALLBACK_PROG: ProgVers = ProgVers::new(2, 1);

fn call_with(payload: &[u8]) -> CallBuf {
    let mut call = CallBuf::new();
    call.extend_from_slice(payload);
    call
}

/// Simple echo service.
struct EchoService;

#[async_trait]
impl RpcHandler for EchoService {
    async fn dispatch(&self, _proc: u32, data: &[u8]) -> Result<Vec<u8>, RpcError> {
        Ok(data.to_vec())
    }
    fn prog_vers(&self) -> ProgVers {
        ECHO_PROG
    }
}

/// Service that calls back to the peer inline (no tokio::spawn).
/// With serial dispatch this deadlocks. With concurrent dispatch it works.
struct InlineCallbackService;

#[async_trait]
impl RpcHandler for InlineCallbackService {
    async fn dispatch(&self, proc: u32, data: &[u8]) -> Result<Vec<u8>, RpcError> {
        match proc {
            0 => Ok(vec![]),
            1 => {
                let peer = rpc_driver::peer();
                // Call back to the client's EchoService on the same connection.
                // This requires the read loop to process the REPLY while this
                // handler is still running — only possible with concurrent dispatch.
                let mut call = peer.call_buf();
                call.extend_from_slice(data);
                let response = peer.send_call(ECHO_PROG, 1, call).await?;
                Ok(response.to_vec())
            }
            _ => Err(RpcError::InvalidProcedure),
        }
    }
    fn prog_vers(&self) -> ProgVers {
        CALLBACK_PROG
    }
}

/// Service that sleeps for a configurable duration before responding.
struct SleepService {
    delay: Duration,
}

#[async_trait]
impl RpcHandler for SleepService {
    async fn dispatch(&self, _proc: u32, data: &[u8]) -> Result<Vec<u8>, RpcError> {
        tokio::time::sleep(self.delay).await;
        Ok(data.to_vec())
    }
    fn prog_vers(&self) -> ProgVers {
        ECHO_PROG
    }
}

/// Service that records whether each dispatch ran on the dispatch loop's
/// probe poll (never suspending) or was moved to a task.
///
/// Proc 0 returns without ever awaiting, so it must complete on the first
/// poll. Proc 1 yields once first, so it must suspend.
struct ReadinessService {
    entered: Arc<AtomicUsize>,
}

#[async_trait]
impl RpcHandler for ReadinessService {
    async fn dispatch(&self, proc: u32, data: &[u8]) -> Result<Vec<u8>, RpcError> {
        self.entered.fetch_add(1, Ordering::SeqCst);
        match proc {
            0 => Ok(data.to_vec()),
            1 => {
                tokio::task::yield_now().await;
                Ok(data.to_vec())
            }
            _ => Err(RpcError::InvalidProcedure),
        }
    }
    fn prog_vers(&self) -> ProgVers {
        ECHO_PROG
    }
}

/// Service that panics before its first await, so the panic surfaces during
/// the dispatch loop's probe poll rather than inside a spawned task.
struct PanicBeforeAwaitService;

#[async_trait]
impl RpcHandler for PanicBeforeAwaitService {
    async fn dispatch(&self, proc: u32, data: &[u8]) -> Result<Vec<u8>, RpcError> {
        match proc {
            0 => Ok(data.to_vec()),
            1 => panic!("intentional panic on first poll"),
            // Suspends after earlier calls have panicked.
            2 => {
                tokio::task::yield_now().await;
                Ok(data.to_vec())
            }
            _ => Err(RpcError::InvalidProcedure),
        }
    }
    fn prog_vers(&self) -> ProgVers {
        ECHO_PROG
    }
}

/// Service that panics on proc 1.
struct PanickingService;

#[async_trait]
impl RpcHandler for PanickingService {
    async fn dispatch(&self, proc: u32, data: &[u8]) -> Result<Vec<u8>, RpcError> {
        match proc {
            0 => Ok(data.to_vec()),
            1 => panic!("intentional test panic"),
            _ => Err(RpcError::InvalidProcedure),
        }
    }
    fn prog_vers(&self) -> ProgVers {
        ECHO_PROG
    }
}

/// Start a server with the given service, return its address.
async fn start_server(service: Box<dyn RpcHandler>) -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let mut services = ServiceMap::new();
    services.add_service(service);
    let server = RpcServer::new(listener, services);
    server.start().await;
    addr
}

/// Start a server with the given config (services are in the config).
async fn start_server_with_config(config: ConnectionConfig) -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = RpcServer::new_with_config(listener, config);
    server.start().await;
    addr
}

async fn start_inline_server(service: Box<dyn RpcHandler>) -> std::net::SocketAddr {
    let mut services = ServiceMap::new();
    services.add_service(service);
    start_server_with_config(ConnectionConfig::new(None, services)).await
}

/// Keep a callback pending until the test explicitly releases it.
struct GatedEchoService {
    entered: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Semaphore>,
}

#[async_trait]
impl RpcHandler for GatedEchoService {
    async fn dispatch(&self, _proc: u32, data: &[u8]) -> Result<Vec<u8>, RpcError> {
        self.entered.notify_one();
        self.release.acquire().await.unwrap().forget();
        Ok(data.to_vec())
    }

    fn prog_vers(&self) -> ProgVers {
        ECHO_PROG
    }
}

#[tokio::test]
async fn suspended_cap_sheds_without_blocking_ready_calls_or_callback_replies() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut services = ServiceMap::new();
        services.add_service(Box::new(InlineCallbackService));
        services.add_service(Box::new(ReadinessService {
            entered: Arc::new(AtomicUsize::new(0)),
        }));
        let addr = start_server_with_config(
            ConnectionConfig::new(None, services).with_max_suspended_handlers(Some(1)),
        )
        .await;
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Semaphore::new(0));
        let mut services = ServiceMap::new();
        services.add_service(Box::new(GatedEchoService {
            entered: entered.clone(),
            release: release.clone(),
        }));
        let client = connect_client_with_config(&addr, ConnectionConfig::new(None, services))
            .await
            .unwrap();

        // Repeat to prove that finishing a suspended task releases its slot.
        for _ in 0..3 {
            let handle = client.handle().clone();
            let call = tokio::spawn(async move {
                handle
                    .send_call(CALLBACK_PROG, 1, call_with(b"callback"))
                    .await
            });
            entered.notified().await;
            // The one admitted handler is awaiting a callback on this same
            // connection. Excess Pending handlers must be shed, never parked.
            for _ in 0..10 {
                assert!(matches!(
                    client.send_call(ECHO_PROG, 1, call_with(b"excess")).await,
                    Err(RpcError::SystemError)
                ));
            }
            assert_eq!(
                client
                    .send_call(ECHO_PROG, 0, call_with(b"ready"))
                    .await
                    .unwrap(),
                b"ready"
            );
            release.add_permits(1);
            assert_eq!(call.await.unwrap().unwrap(), b"callback");
        }
    })
    .await
    .expect("handler cap blocked bidirectional progress");
}

#[tokio::test]
async fn zero_output_threshold_rejects_calls_with_replies_without_running_handlers() {
    let entered = Arc::new(AtomicUsize::new(0));
    let mut services = ServiceMap::new();
    services.add_service(Box::new(ReadinessService {
        entered: entered.clone(),
    }));
    let addr = start_server_with_config(
        ConnectionConfig::new(None, services).with_output_admission_threshold_bytes(Some(0)),
    )
    .await;
    let client = connect_client_with_config(&addr, ConnectionConfig::default())
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        for _ in 0..10 {
            assert!(matches!(
                client.send_call(ECHO_PROG, 0, CallBuf::new()).await,
                Err(RpcError::SystemError)
            ));
            assert!(!client.handle().is_poisoned());
        }
        assert_eq!(entered.load(Ordering::SeqCst), 0);
    })
    .await
    .expect("rejection replies must bypass the zero threshold");
}

#[tokio::test]
async fn zero_suspended_cap_allows_ready_handlers_only() {
    let mut services = ServiceMap::new();
    services.add_service(Box::new(ReadinessService {
        entered: Arc::new(AtomicUsize::new(0)),
    }));
    let addr = start_server_with_config(
        ConnectionConfig::new(None, services).with_max_suspended_handlers(Some(0)),
    )
    .await;
    let client = connect_client_with_config(&addr, ConnectionConfig::default())
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        assert!(matches!(
            client.send_call(ECHO_PROG, 1, CallBuf::new()).await,
            Err(RpcError::SystemError)
        ));
        assert_eq!(
            client
                .send_call(ECHO_PROG, 0, call_with(b"ready"))
                .await
                .unwrap(),
            b"ready"
        );
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn test_bidirectional_rpc() {
    // Server runs InlineCallbackService (prog=2, vers=1).
    // On proc 1, it calls peer().send_call(ECHO_PROG, 1, call) — calling
    // back to the CLIENT's EchoService on the same connection.
    let addr = start_server(Box::new(InlineCallbackService)).await;

    // Client registers EchoService so the server's callback has something to hit.
    let mut client_services = ServiceMap::new();
    client_services.add_service(Box::new(EchoService));
    let config = ConnectionConfig::new(None, client_services);
    let client = connect_client_with_config(&addr, config).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Call the server's InlineCallbackService (proc 1).
    // The server will call back to our EchoService, then return the result.
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        client.send_call(CALLBACK_PROG, 1, call_with(b"roundtrip")),
    )
    .await;

    assert!(result.is_ok(), "bidirectional call deadlocked (timeout)");
    assert_eq!(result.unwrap().unwrap(), b"roundtrip");
}

/// Repeated callbacks must progress without any dispatch configuration.
#[tokio::test]
async fn test_repeated_bidirectional_calls() {
    let mut server_services = ServiceMap::new();
    server_services.add_service(Box::new(InlineCallbackService));
    let config = ConnectionConfig::new(None, server_services);
    let addr = start_server_with_config(config).await;

    let mut client_services = ServiceMap::new();
    client_services.add_service(Box::new(EchoService));
    let config = ConnectionConfig::new(None, client_services);
    let client = connect_client_with_config(&addr, config).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    let result = tokio::time::timeout(
        Duration::from_secs(2),
        client.send_call(CALLBACK_PROG, 1, call_with(b"limit-test")),
    )
    .await;

    assert!(result.is_ok(), "bidirectional call deadlocked");
    assert_eq!(result.unwrap().unwrap(), b"limit-test");

    // Repeat a few times to ensure stability.
    for i in 0..5 {
        let payload = format!("iter-{i}");
        let resp = tokio::time::timeout(
            Duration::from_secs(2),
            client.send_call(CALLBACK_PROG, 1, call_with(payload.as_bytes())),
        )
        .await
        .expect("deadlock on repeat iteration")
        .unwrap();
        assert_eq!(resp, payload.as_bytes());
    }
}

/// Overlapping callbacks must not block routing of other calls or replies.
#[tokio::test]
async fn test_concurrent_bidirectional_calls() {
    let mut server_services = ServiceMap::new();
    server_services.add_service(Box::new(InlineCallbackService));
    server_services.add_service(Box::new(EchoService));
    let config = ConnectionConfig::new(None, server_services);
    let addr = start_server_with_config(config).await;

    let mut client_services = ServiceMap::new();
    client_services.add_service(Box::new(EchoService));
    let config = ConnectionConfig::new(None, client_services);
    let client = connect_client_with_config(&addr, config).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Exceed the old 64-handler limit without gating callback progress.
    let n = 128;
    let mut handles = Vec::new();
    for i in 0..n {
        let client = client.handle().clone();
        let payload = format!("concurrent-bidir-{i}");
        handles.push(tokio::spawn(async move {
            let result = tokio::time::timeout(
                Duration::from_secs(5),
                client.send_call(CALLBACK_PROG, 1, call_with(payload.as_bytes())),
            )
            .await;
            (payload, result)
        }));
    }
    for (i, h) in handles.into_iter().enumerate() {
        let (payload, result) = h.await.unwrap();
        assert!(
            result.is_ok(),
            "concurrent bidirectional call {i} deadlocked"
        );
        let resp = result.unwrap().unwrap();
        assert_eq!(
            resp,
            payload.as_bytes(),
            "call {i} payload mismatch: responses may be mixed across dispatches"
        );
    }
}

#[tokio::test(start_paused = true)]
async fn test_concurrent_throughput() {
    let delay = Duration::from_millis(100);
    let addr = start_server(Box::new(SleepService { delay })).await;
    let client = connect_client_with_config(&addr, ConnectionConfig::default())
        .await
        .unwrap();

    let n = 10;
    let start = tokio::time::Instant::now();
    let mut handles = Vec::new();
    for i in 0..n {
        let client = client.handle().clone();
        handles.push(tokio::spawn(async move {
            client
                .send_call(ECHO_PROG, 1, call_with(&[i as u8]))
                .await
                .unwrap()
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
    let elapsed = start.elapsed();

    // With concurrent dispatch: ~100ms (all sleep in parallel).
    // With serial dispatch: ~1000ms (10 × 100ms sequentially).
    // Use 500ms as threshold — well above concurrent, well below serial.
    // (start_paused = true makes tokio::time::sleep deterministic.)
    assert!(
        elapsed < Duration::from_millis(500),
        "expected concurrent completion in <500ms, took {elapsed:?} (serial dispatch?)",
    );
}

#[tokio::test]
async fn test_panic_recovery() {
    let addr = start_server(Box::new(PanickingService)).await;
    let client = connect_client_with_config(&addr, ConnectionConfig::default())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    // proc 0 = normal echo, should work
    let result = client.send_call(ECHO_PROG, 0, call_with(b"ok")).await;
    assert_eq!(result.unwrap(), b"ok");

    // proc 1 = panic — should get error response, not hang
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        client.send_call(ECHO_PROG, 1, call_with(b"boom")),
    )
    .await;
    assert!(result.is_ok(), "panic handler should not deadlock");
    assert_eq!(result.unwrap().unwrap_err(), RpcError::SystemError);

    // Connection should still be alive — send another normal request
    let result = client
        .send_call(ECHO_PROG, 0, call_with(b"still alive"))
        .await;
    assert_eq!(result.unwrap(), b"still alive");
}

// ---------------------------------------------------------------------------
// Inline fast path for ready handlers
// ---------------------------------------------------------------------------

/// A handler that never awaits is answered correctly on the probe poll.
#[tokio::test]
async fn test_ready_handler_answered_inline() {
    let entered = Arc::new(AtomicUsize::new(0));
    let addr = start_inline_server(Box::new(ReadinessService {
        entered: entered.clone(),
    }))
    .await;
    let client = connect_client_with_config(&addr, ConnectionConfig::default())
        .await
        .unwrap();

    for i in 0..20u8 {
        let payload = [i; 4];
        let response = client
            .send_call(ECHO_PROG, 0, call_with(&payload))
            .await
            .unwrap();
        assert_eq!(response, payload, "ready handler returned wrong payload");
    }
    assert_eq!(entered.load(Ordering::SeqCst), 20);
}

/// A handler that suspends still completes — it is moved into a task after the
/// probe poll returns `Pending`, and must not lose the work already done.
#[tokio::test]
async fn test_suspending_handler_completes_after_probe() {
    let entered = Arc::new(AtomicUsize::new(0));
    let addr = start_inline_server(Box::new(ReadinessService {
        entered: entered.clone(),
    }))
    .await;
    let client = connect_client_with_config(&addr, ConnectionConfig::default())
        .await
        .unwrap();

    // proc 1 yields before returning, so every call takes the spawn path.
    for i in 0..20u8 {
        let payload = [i; 4];
        let response = client
            .send_call(ECHO_PROG, 1, call_with(&payload))
            .await
            .unwrap();
        assert_eq!(
            response, payload,
            "suspended handler returned wrong payload"
        );
    }
    assert_eq!(entered.load(Ordering::SeqCst), 20);
}

/// Ready and suspending calls interleaved on one connection: each must get its
/// own reply, with no cross-talk between the inline and spawned paths.
#[tokio::test]
async fn test_mixed_ready_and_suspending_calls() {
    let entered = Arc::new(AtomicUsize::new(0));
    let addr = start_inline_server(Box::new(ReadinessService {
        entered: entered.clone(),
    }))
    .await;
    let client = connect_client_with_config(&addr, ConnectionConfig::default())
        .await
        .unwrap();

    let mut handles = Vec::new();
    for i in 0..40u8 {
        let handle = client.handle().clone();
        // Alternate ready (proc 0) and suspending (proc 1) handlers.
        let proc = u32::from(i % 2);
        handles.push(tokio::spawn(async move {
            let payload = [i; 4];
            let response = handle
                .send_call(ECHO_PROG, proc, call_with(&payload))
                .await
                .unwrap();
            assert_eq!(response, payload, "reply routed to the wrong xid");
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
    assert_eq!(entered.load(Ordering::SeqCst), 40);
}

/// A panic during the probe poll must not take down the dispatch loop: the
/// caller gets SystemError and the connection stays usable.
#[tokio::test]
async fn test_panic_on_probe_poll_is_isolated() {
    let addr = start_inline_server(Box::new(PanicBeforeAwaitService)).await;
    let client = connect_client_with_config(&addr, ConnectionConfig::default())
        .await
        .unwrap();

    let result = tokio::time::timeout(
        Duration::from_secs(2),
        client.send_call(ECHO_PROG, 1, call_with(b"boom")),
    )
    .await;
    assert!(result.is_ok(), "panic on probe poll hung the dispatch loop");
    assert!(result.unwrap().is_err(), "expected an error reply");

    // Connection must survive the panic.
    let response = client
        .send_call(ECHO_PROG, 0, call_with(b"still alive"))
        .await;
    assert_eq!(response.unwrap(), b"still alive");
}

/// Panicking handlers must not stall subsequent suspending handlers.
#[tokio::test]
async fn test_panic_does_not_stall_later_suspended_handlers() {
    let mut services = ServiceMap::new();
    services.add_service(Box::new(PanicBeforeAwaitService));
    let config = ConnectionConfig::new(None, services);
    let addr = start_server_with_config(config).await;
    let client = connect_client_with_config(&addr, ConnectionConfig::default())
        .await
        .unwrap();

    // Repeated panics must leave the reader usable.
    for i in 0..3 {
        let result = tokio::time::timeout(
            Duration::from_secs(2),
            client.send_call(ECHO_PROG, 1, call_with(b"boom")),
        )
        .await;
        let Ok(result) = result else {
            panic!("panicking call {i} never completed");
        };
        assert!(result.is_err(), "expected an error reply from the panic");
    }

    // proc 2 suspends and must still finish.
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        client.send_call(ECHO_PROG, 2, call_with(b"after-panic")),
    )
    .await;
    assert!(result.is_ok(), "suspending handler stalled after a panic");
    assert_eq!(result.unwrap().unwrap(), b"after-panic");
}

/// Tests that the server shuts down cleanly when the client disconnects
/// while handlers are still in-flight.
///
/// Dropping the client while slow handlers are executing exercises the
/// teardown path: the reader task sees EOF, the dispatch loop exits,
/// and the JoinSet drops, cancelling in-flight tasks.
#[tokio::test(start_paused = true)]
async fn test_client_disconnect_during_dispatch() {
    let delay = Duration::from_secs(10);
    let addr = start_server(Box::new(SleepService { delay })).await;
    let client = connect_client_with_config(&addr, ConnectionConfig::default())
        .await
        .unwrap();

    // Fire several calls that will be in-flight for a long time.
    let mut handles = Vec::new();
    for i in 0..5 {
        let client = client.handle().clone();
        handles.push(tokio::spawn(async move {
            client.send_call(ECHO_PROG, 1, call_with(&[i])).await
        }));
    }

    // Give the server time to receive and start dispatching all calls.
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Poison the client connection: drops the write half (sends TCP FIN)
    // and fails all pending requests. The spawned tasks still hold Arc
    // clones, but the connection is severed — the server sees EOF.
    client.poison().await;

    // All in-flight calls should resolve with errors (not hang).
    for h in handles {
        let result = tokio::time::timeout(Duration::from_secs(2), h).await;
        assert!(
            result.is_ok(),
            "in-flight call hung after client disconnect"
        );
        // The call should fail (SystemError from connection poisoning), not succeed.
        assert!(result.unwrap().unwrap().is_err());
    }

    // Verify the server can still accept new connections (it wasn't poisoned globally).
    let client2 = connect_client_with_config(&addr, ConnectionConfig::default())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    let result = client2.send_call(ECHO_PROG, 1, call_with(b"alive")).await;
    assert_eq!(result.unwrap(), b"alive");
}
