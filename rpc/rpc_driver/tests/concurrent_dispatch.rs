//! Tests for the concurrent-dispatch feature.
//!
//! These tests only compile when `concurrent-dispatch` is enabled:
//!   cargo test -p rpc_driver --features concurrent-dispatch

#![cfg(feature = "concurrent-dispatch")]

use std::time::Duration;

use async_trait::async_trait;
use rpc_driver::{
    ConnectionConfig, ProgVers, RpcError, RpcServer, connect_client_with_config,
    handler::RpcHandler, handler::ServiceMap,
};

const ECHO_PROG: ProgVers = ProgVers::new(1, 1);
const CALLBACK_PROG: ProgVers = ProgVers::new(2, 1);

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
                let response = peer.send_call(ECHO_PROG, 1, data).await?;
                Ok(response)
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

#[tokio::test]
async fn test_bidirectional_rpc() {
    // Server runs InlineCallbackService (prog=2, vers=1).
    // On proc 1, it calls peer().send_call(ECHO_PROG, 1, data) — calling
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
        client.send_call(CALLBACK_PROG, 1, b"roundtrip"),
    )
    .await;

    assert!(result.is_ok(), "bidirectional call deadlocked (timeout)");
    assert_eq!(result.unwrap().unwrap(), b"roundtrip");
}

/// Regression test: bidirectional RPC at exactly the concurrency limit.
///
/// With max_concurrent_dispatches=1 and a handler that calls peer().send_call()
/// inline, the dispatch loop must still route REPLY messages even though the
/// semaphore is fully acquired.
#[tokio::test]
async fn test_bidirectional_at_concurrency_limit() {
    // Server: InlineCallbackService (calls back to client's EchoService)
    // with max_concurrent_dispatches=1 — the tightest possible limit.
    let mut server_services = ServiceMap::new();
    server_services.add_service(Box::new(InlineCallbackService));
    let config = ConnectionConfig::new(None, server_services).with_max_concurrent_dispatches(1);
    let addr = start_server_with_config(config).await;

    let mut client_services = ServiceMap::new();
    client_services.add_service(Box::new(EchoService));
    let config = ConnectionConfig::new(None, client_services);
    let client = connect_client_with_config(&addr, config).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    // This would deadlock if REPLYs were gated by the concurrency limit:
    // the handler is waiting for a REPLY, but the dispatch loop can't route
    // it because the semaphore is fully acquired.
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        client.send_call(CALLBACK_PROG, 1, b"limit-test"),
    )
    .await;

    assert!(
        result.is_ok(),
        "bidirectional call at concurrency limit deadlocked"
    );
    assert_eq!(result.unwrap().unwrap(), b"limit-test");

    // Repeat a few times to ensure stability.
    for i in 0..5 {
        let payload = format!("iter-{i}");
        let resp = tokio::time::timeout(
            Duration::from_secs(2),
            client.send_call(CALLBACK_PROG, 1, payload.as_bytes()),
        )
        .await
        .expect("deadlock on repeat iteration")
        .unwrap();
        assert_eq!(resp, payload.as_bytes());
    }
}

/// Regression test: concurrent bidirectional RPCs at the concurrency limit.
///
/// Sends multiple overlapping bidirectional calls at max_concurrent_dispatches=2.
/// While handlers are in-flight awaiting their REPLYs, additional CALLs arrive
/// and wait for semaphore permits. The dispatch loop must still route REPLYs
/// or the in-flight handlers deadlock.
#[tokio::test]
async fn test_concurrent_bidirectional_at_capacity() {
    let mut server_services = ServiceMap::new();
    server_services.add_service(Box::new(InlineCallbackService));
    server_services.add_service(Box::new(EchoService));
    // max_concurrent=2: two handlers can be in-flight simultaneously.
    let config = ConnectionConfig::new(None, server_services).with_max_concurrent_dispatches(2);
    let addr = start_server_with_config(config).await;

    let mut client_services = ServiceMap::new();
    client_services.add_service(Box::new(EchoService));
    let config = ConnectionConfig::new(None, client_services);
    let client = connect_client_with_config(&addr, config).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Fire 4 concurrent bidirectional calls. With max_concurrent=2, at least
    // 2 will acquire permits and dispatch simultaneously; the others wait
    // for permits. Each handler calls peer().send_call() inline, so the
    // dispatch loop MUST route REPLYs to avoid deadlock.
    let n = 4;
    let mut handles = Vec::new();
    for i in 0..n {
        let client = client.clone();
        let payload = format!("concurrent-bidir-{i}");
        handles.push(tokio::spawn(async move {
            let result = tokio::time::timeout(
                Duration::from_secs(5),
                client.send_call(CALLBACK_PROG, 1, payload.as_bytes()),
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
        let client = client.clone();
        handles.push(tokio::spawn(async move {
            client.send_call(ECHO_PROG, 1, &[i as u8]).await.unwrap()
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
        "expected concurrent completion in <500ms, took {:?} (serial dispatch?)",
        elapsed,
    );
}

#[tokio::test(start_paused = true)]
async fn test_backpressure() {
    let delay = Duration::from_millis(200);
    let mut services = ServiceMap::new();
    services.add_service(Box::new(SleepService { delay }));
    let config = ConnectionConfig::new(None, services).with_max_concurrent_dispatches(2);
    let addr = start_server_with_config(config).await;

    let client = connect_client_with_config(&addr, ConnectionConfig::default())
        .await
        .unwrap();

    let n = 6;
    let start = tokio::time::Instant::now();
    let mut handles = Vec::new();
    for i in 0..n {
        let client = client.clone();
        handles.push(tokio::spawn(async move {
            client.send_call(ECHO_PROG, 1, &[i as u8]).await.unwrap()
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
    let elapsed = start.elapsed();

    // 6 requests, max_concurrent=2 → 3 batches × 200ms = ~600ms.
    // Should be ≥500ms (backpressure working) and <900ms (not fully serial = 1200ms).
    // (start_paused = true makes tokio::time::sleep deterministic.)
    assert!(
        elapsed >= Duration::from_millis(500),
        "completed too fast ({:?}) — backpressure not working",
        elapsed,
    );
    assert!(
        elapsed < Duration::from_millis(900),
        "took too long ({:?}) — dispatches may be fully serial",
        elapsed,
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
    let result = client.send_call(ECHO_PROG, 0, b"ok").await;
    assert_eq!(result.unwrap(), b"ok");

    // proc 1 = panic — should get error response, not hang
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        client.send_call(ECHO_PROG, 1, b"boom"),
    )
    .await;
    assert!(result.is_ok(), "panic handler should not deadlock");
    assert_eq!(result.unwrap().unwrap_err(), RpcError::SystemError);

    // Connection should still be alive — send another normal request
    let result = client.send_call(ECHO_PROG, 0, b"still alive").await;
    assert_eq!(result.unwrap(), b"still alive");
}

/// Tests that the pending-tasks cap triggers load shedding.
///
/// With max_concurrent=1, the pending cap is 5 (1 * 5). We fill the server
/// with slow handlers beyond the cap and verify that excess calls receive
/// SystemError.
#[tokio::test]
async fn test_load_shedding() {
    let delay = Duration::from_secs(10); // intentionally long — we don't wait for completion
    let mut services = ServiceMap::new();
    services.add_service(Box::new(SleepService { delay }));
    // max_concurrent=1, so pending cap = 1 * 5 = 5.
    // Total tasks before shedding = 5 (1 executing + 4 waiting for permit).
    let config = ConnectionConfig::new(None, services).with_max_concurrent_dispatches(1);
    let addr = start_server_with_config(config).await;

    let client = connect_client_with_config(&addr, ConnectionConfig::default())
        .await
        .unwrap();

    // Fire 8 concurrent calls. The first 5 are spawned (1 executes, 4 wait
    // for permits), and the remaining 3 should be shed with SystemError.
    let n = 8;
    let mut handles = Vec::new();
    for i in 0..n {
        let client = client.clone();
        handles.push(tokio::spawn(async move {
            tokio::time::timeout(Duration::from_secs(2), client.send_call(ECHO_PROG, 1, &[i])).await
        }));
    }

    // Give the server time to receive and process all 8 calls.
    tokio::time::sleep(Duration::from_millis(500)).await;

    let mut errors = 0;
    let mut timeouts = 0;
    let mut ok = 0;
    for h in handles {
        match h.await.unwrap() {
            Err(_) => timeouts += 1, // timeout — call is queued/in-flight with 10s handler
            Ok(Err(RpcError::SystemError)) => errors += 1, // load-shed
            Ok(Err(_)) => errors += 1,
            Ok(Ok(_)) => ok += 1,
        }
    }

    // cap=5, sent=8, so at least 3 should be shed.
    assert!(
        errors >= 3,
        "expected >=3 load-shed errors but got: ok={ok}, timeouts={timeouts}, errors={errors}"
    );
}

/// Tests that the server shuts down cleanly when the client disconnects
/// while handlers are still in-flight.
///
/// Dropping the client while slow handlers are executing exercises the
/// teardown path: the reader task sees EOF, the dispatch loop exits,
/// and the JoinSet drops (cancelling in-flight tasks). PendingGuard
/// must fire on cancellation to avoid leaking the pending counter.
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
        let client = client.clone();
        handles.push(tokio::spawn(async move {
            client.send_call(ECHO_PROG, 1, &[i]).await
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
    let result = client2.send_call(ECHO_PROG, 1, b"alive").await;
    assert_eq!(result.unwrap(), b"alive");
}
