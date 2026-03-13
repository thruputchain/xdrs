//! Connection pools for RPC clients.
//!
//! Two pool implementations:
//!
//! - [`FixedPool`] — Fixed-size pool with FIFO request dispatch. Each
//!   connection has a dedicated worker task that processes requests
//!   sequentially. Suited for the scheduler connection pattern.
//!
//! - [`DynamicPool`] — Lazy pool with min-pending load balancing. Opens
//!   connections on demand up to a maximum, routes each call to the
//!   connection with the fewest in-flight requests, and evicts idle
//!   connections after a configurable TTL. Suited for per-peer Dogwood
//!   connections.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;

use crate::{ConnectionHandle, ProgVers, RpcSender, connect_client, rpc_error::RpcError};

/// A request dispatched through the pool to a worker.
struct PoolRequest {
    progvers: ProgVers,
    proc_num: u32,
    data: Vec<u8>,
    reply: oneshot::Sender<Result<Vec<u8>, RpcError>>,
}

/// Fixed-size connection pool with FIFO request dispatch.
///
/// Wraps N [`ConnectionHandle`] connections to a single address. A shared
/// `tokio::sync::mpsc` channel distributes requests to connection workers
/// in FIFO order — one call at a time per connection.
///
/// This matches the Go `ConnectPoolToScheduler` pattern: a fixed number
/// of connections with sequential dispatch per connection.
#[derive(Debug)]
pub struct FixedPool {
    request_tx: mpsc::Sender<PoolRequest>,
    size: usize,
}

// PoolRequest contains a oneshot::Sender which is not Debug, so we need
// a manual impl for FixedPool only (request_tx is Sender<PoolRequest>).
// The Debug derive on FixedPool works because mpsc::Sender<T> implements
// Debug regardless of T.

impl FixedPool {
    /// Connect `pool_size` clients to `addr`, each with a dedicated worker task.
    ///
    /// All connections are established eagerly. If any connection fails, the
    /// entire pool creation fails and already-spawned workers exit
    /// asynchronously when the sender is dropped.
    pub async fn connect(addr: &SocketAddr, pool_size: usize) -> Result<Self, RpcError> {
        if pool_size == 0 {
            return Err(RpcError::SystemError);
        }

        let (tx, rx) = mpsc::channel::<PoolRequest>(pool_size.saturating_mul(16).max(16));
        let rx = Arc::new(tokio::sync::Mutex::new(rx));

        for _ in 0..pool_size {
            let client = connect_client(addr).await.map_err(|e| {
                eprintln!("pool: failed to connect to {addr}: {e}");
                RpcError::SystemError
            })?;
            let worker_rx = rx.clone();
            tokio::spawn(async move {
                loop {
                    // Acquire the shared receiver lock. The next idle worker
                    // wins the lock and takes the next request.
                    let req = {
                        let mut guard = worker_rx.lock().await;
                        guard.recv().await
                    };
                    // Lock is released here before we do the RPC call,
                    // so other workers can pick up requests concurrently.

                    let req = match req {
                        Some(r) => r,
                        None => break, // Channel closed, pool is shutting down.
                    };

                    let result = client
                        .send_call(req.progvers, req.proc_num, &req.data)
                        .await;
                    let _ = req.reply.send(result);
                }
            });
        }

        Ok(Self {
            request_tx: tx,
            size: pool_size,
        })
    }

    /// Returns the number of connections in the pool.
    pub fn size(&self) -> usize {
        self.size
    }
}

#[async_trait]
impl RpcSender for FixedPool {
    /// Send an RPC call through the pool.
    ///
    /// The request is dispatched to the next idle worker via FIFO ordering.
    async fn send_call(
        &self,
        progvers: ProgVers,
        proc: u32,
        data: &[u8],
    ) -> Result<Vec<u8>, RpcError> {
        let (tx, rx) = oneshot::channel();
        let request = PoolRequest {
            progvers,
            proc_num: proc,
            data: data.to_vec(),
            reply: tx,
        };
        self.request_tx
            .send(request)
            .await
            .map_err(|_| RpcError::SystemError)?;
        rx.await.map_err(|_| RpcError::SystemError)?
    }
}

/// A connection tracked by the dynamic pool.
struct PooledConnection {
    handle: Arc<ConnectionHandle>,
    last_used: Instant,
}

/// Dynamic connection pool with min-pending load balancing.
///
/// Opens connections lazily up to `max_connections`. Each call is routed
/// to the connection with the fewest pending requests. Idle connections
/// are evicted after `idle_ttl`.
///
/// Use [`DynamicPool::start_evictor`] to spawn the background eviction
/// task. Without it, idle connections are never reaped.
pub struct DynamicPool {
    addr: SocketAddr,
    max_connections: usize,
    idle_ttl: Duration,
    connections: tokio::sync::Mutex<Vec<PooledConnection>>,
}

// Manual Debug impl because PooledConnection contains ConnectionHandle (which
// is Debug) but we want to show pool-level stats, not connection internals.
impl std::fmt::Debug for DynamicPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DynamicPool")
            .field("addr", &self.addr)
            .field("max_connections", &self.max_connections)
            .field("idle_ttl", &self.idle_ttl)
            .finish_non_exhaustive()
    }
}

impl DynamicPool {
    /// Create a new dynamic pool targeting `addr`.
    ///
    /// No connections are opened until the first call. At most
    /// `max_connections` connections will be open simultaneously.
    /// Connections idle longer than `idle_ttl` are eligible for eviction.
    ///
    /// # Panics
    ///
    /// Panics if `max_connections` is 0.
    pub fn new(addr: SocketAddr, max_connections: usize, idle_ttl: Duration) -> Self {
        assert!(max_connections > 0, "max_connections must be at least 1");
        Self {
            addr,
            max_connections,
            idle_ttl,
            connections: tokio::sync::Mutex::new(Vec::new()),
        }
    }

    /// Select or create a connection with the fewest pending requests.
    async fn lease_connection(&self) -> Result<Arc<ConnectionHandle>, RpcError> {
        let mut conns = self.connections.lock().await;

        if conns.is_empty() {
            // No connections yet — open the first one.
            let handle = connect_client(&self.addr).await.map_err(|e| {
                eprintln!("dynamic pool: failed to connect to {}: {e}", self.addr);
                RpcError::SystemError
            })?;
            conns.push(PooledConnection {
                handle: handle.clone(),
                last_used: Instant::now(),
            });
            return Ok(handle);
        }

        // Find the connection with the fewest pending requests.
        let (min_idx, min_pending) = conns
            .iter()
            .enumerate()
            .map(|(i, c)| (i, c.handle.num_pending_requests()))
            .min_by_key(|&(_, p)| p)
            .expect("bug: conns is non-empty but min_by_key returned None");

        // If the best connection is idle, use it immediately.
        if min_pending == 0 {
            conns[min_idx].last_used = Instant::now();
            return Ok(conns[min_idx].handle.clone());
        }

        // All connections are busy. If we have room, open a new one.
        if conns.len() < self.max_connections {
            // Drop the lock before connecting? No — we need to hold it to
            // prevent racing with other lease_connection calls that would
            // also see "all busy" and open connections past the limit.
            let handle = connect_client(&self.addr).await.map_err(|e| {
                eprintln!("dynamic pool: failed to connect to {}: {e}", self.addr);
                RpcError::SystemError
            })?;
            conns.push(PooledConnection {
                handle: handle.clone(),
                last_used: Instant::now(),
            });
            return Ok(handle);
        }

        // At max capacity — return the least-loaded connection.
        conns[min_idx].last_used = Instant::now();
        Ok(conns[min_idx].handle.clone())
    }

    /// Update the `last_used` timestamp for a connection after a call completes.
    fn touch_connection(&self, handle: &Arc<ConnectionHandle>) {
        // Try to acquire the lock without blocking. If contended, skip the
        // touch — the timestamp will be close enough from the lease.
        if let Ok(mut conns) = self.connections.try_lock()
            && let Some(entry) = conns.iter_mut().find(|c| Arc::ptr_eq(&c.handle, handle))
        {
            entry.last_used = Instant::now();
        }
    }

    /// Returns the total number of pending (in-flight) requests across all connections.
    pub async fn pending_count(&self) -> usize {
        let conns = self.connections.lock().await;
        conns.iter().map(|c| c.handle.num_pending_requests()).sum()
    }

    /// Returns the number of active (non-evicted) connections.
    pub async fn active_connections(&self) -> usize {
        self.connections.lock().await.len()
    }

    /// Remove connections that have been idle longer than `idle_ttl`.
    ///
    /// Poisoned connections are cleaned up so the remote end sees a FIN.
    async fn evict_idle(&self) {
        let mut conns = self.connections.lock().await;
        let mut i = 0;
        while i < conns.len() {
            let idle_duration = conns[i].last_used.elapsed();
            let pending = conns[i].handle.num_pending_requests();
            if pending == 0 && idle_duration > self.idle_ttl {
                let removed = conns.swap_remove(i);
                // Poison in background to avoid holding the lock during async poison.
                tokio::spawn(async move {
                    removed.handle.poison().await;
                });
                // Don't increment i — swap_remove moved the last element here.
            } else {
                i += 1;
            }
        }
    }

    /// Spawn a background task that periodically evicts idle connections.
    ///
    /// The task runs until the returned [`tokio::task::JoinHandle`] is
    /// aborted or the pool is dropped. The check interval is half the
    /// `idle_ttl` to keep eviction reasonably prompt without busy-looping.
    pub fn start_evictor(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        let pool = Arc::clone(self);
        let interval = pool.idle_ttl / 2;
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(interval).await;
                pool.evict_idle().await;
            }
        })
    }
}

#[async_trait]
impl RpcSender for DynamicPool {
    /// Send an RPC call through the pool.
    ///
    /// Routed to the connection with the fewest pending requests.
    async fn send_call(
        &self,
        progvers: ProgVers,
        proc: u32,
        data: &[u8],
    ) -> Result<Vec<u8>, RpcError> {
        let conn = self.lease_connection().await?;
        let result = conn.send_call(progvers, proc, data).await;
        self.touch_connection(&conn);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RpcServer;
    use crate::handler::{RpcHandler, ServiceMap};
    use async_trait::async_trait;
    use tokio::net::TcpListener;

    /// Echo service that returns the input data unchanged.
    struct EchoService;

    #[async_trait]
    impl RpcHandler for EchoService {
        async fn dispatch(&self, proc: u32, data: &[u8]) -> Result<Vec<u8>, RpcError> {
            if proc != 1 {
                return Err(RpcError::InvalidProcedure);
            }
            Ok(data.to_vec())
        }

        fn prog_vers(&self) -> ProgVers {
            ProgVers::new(1, 1)
        }
    }

    /// Slow echo service that sleeps before responding.
    struct SlowEchoService {
        delay: std::time::Duration,
    }

    #[async_trait]
    impl RpcHandler for SlowEchoService {
        async fn dispatch(&self, proc: u32, data: &[u8]) -> Result<Vec<u8>, RpcError> {
            if proc != 1 {
                return Err(RpcError::InvalidProcedure);
            }
            tokio::time::sleep(self.delay).await;
            Ok(data.to_vec())
        }

        fn prog_vers(&self) -> ProgVers {
            ProgVers::new(1, 1)
        }
    }

    /// Helper: start an RPC server with the given service and return its address.
    async fn start_echo_server() -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let mut services = ServiceMap::new();
        services.add_service(Box::new(EchoService));
        let server = RpcServer::new(listener, services);
        server.start().await;
        addr
    }

    /// Helper: start a slow RPC server with the given delay.
    async fn start_slow_server(delay: std::time::Duration) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let mut services = ServiceMap::new();
        services.add_service(Box::new(SlowEchoService { delay }));
        let server = RpcServer::new(listener, services);
        server.start().await;
        addr
    }

    #[tokio::test]
    async fn fixed_pool_distributes_calls() {
        let addr = start_echo_server().await;

        let pool = FixedPool::connect(&addr, 3).await.unwrap();
        assert_eq!(pool.size(), 3);

        // Send 9 calls sequentially and verify all succeed with correct data.
        for i in 0..9u32 {
            let data = format!("call-{}", i);
            let result = pool
                .send_call(ProgVers::new(1, 1), 1, data.as_bytes())
                .await
                .unwrap();
            assert_eq!(result, data.as_bytes());
        }
    }

    #[tokio::test]
    async fn fixed_pool_concurrent_calls() {
        let delay = std::time::Duration::from_millis(100);
        let addr = start_slow_server(delay).await;

        let pool = Arc::new(FixedPool::connect(&addr, 3).await.unwrap());

        let start = tokio::time::Instant::now();

        // Send 3 calls concurrently through a pool of 3 connections.
        // Each call takes ~100ms. With 3 workers, total should be ~100ms.
        let mut handles = Vec::new();
        for i in 0..3u32 {
            let pool = pool.clone();
            handles.push(tokio::spawn(async move {
                let data = format!("slow-{}", i);
                pool.send_call(ProgVers::new(1, 1), 1, data.as_bytes())
                    .await
                    .unwrap()
            }));
        }

        for handle in handles {
            handle.await.unwrap();
        }

        let elapsed = start.elapsed();

        // With 3 workers handling 3 calls in parallel, wall time should be
        // close to one delay (~100ms), not three delays (~300ms).
        // Allow generous margin for CI variance.
        assert!(
            elapsed < std::time::Duration::from_millis(250),
            "Expected parallel execution (~100ms), got {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn fixed_pool_error_propagation() {
        let addr = start_echo_server().await;

        let pool = FixedPool::connect(&addr, 2).await.unwrap();

        // Call an invalid procedure — error should propagate through the pool.
        let result = pool.send_call(ProgVers::new(1, 1), 99, b"bad").await;
        assert_eq!(result, Err(RpcError::InvalidProcedure));

        // Pool should still work for valid calls after an error.
        let result = pool.send_call(ProgVers::new(1, 1), 1, b"ok").await.unwrap();
        assert_eq!(result, b"ok");
    }

    #[tokio::test]
    async fn fixed_pool_zero_size_rejected() {
        let addr = start_echo_server().await;
        let result = FixedPool::connect(&addr, 0).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn fixed_pool_single_connection() {
        let addr = start_echo_server().await;

        let pool = FixedPool::connect(&addr, 1).await.unwrap();
        assert_eq!(pool.size(), 1);

        // Even with one connection, sequential calls should work.
        for i in 0..5u32 {
            let data = format!("single-{}", i);
            let result = pool
                .send_call(ProgVers::new(1, 1), 1, data.as_bytes())
                .await
                .unwrap();
            assert_eq!(result, data.as_bytes());
        }
    }

    #[tokio::test]
    async fn fixed_pool_connect_failure() {
        // Try to connect to an address that's not listening.
        let addr: SocketAddr = "127.0.0.1:1".parse().unwrap();
        let result = FixedPool::connect(&addr, 3).await;
        assert!(result.is_err());
    }

    // --- DynamicPool tests ---

    #[tokio::test]
    async fn dynamic_pool_opens_connections_lazily() {
        let addr = start_echo_server().await;

        let pool = DynamicPool::new(addr, 3, Duration::from_secs(60));
        assert_eq!(pool.active_connections().await, 0);

        // First call opens a connection.
        let result = pool
            .send_call(ProgVers::new(1, 1), 1, b"hello")
            .await
            .unwrap();
        assert_eq!(result, b"hello");
        assert_eq!(pool.active_connections().await, 1);
    }

    #[tokio::test]
    async fn dynamic_pool_sequential_reuses_connection() {
        let addr = start_echo_server().await;

        let pool = DynamicPool::new(addr, 3, Duration::from_secs(60));

        // Sequential calls should reuse the same (idle) connection.
        for i in 0..5u32 {
            let data = format!("seq-{}", i);
            let result = pool
                .send_call(ProgVers::new(1, 1), 1, data.as_bytes())
                .await
                .unwrap();
            assert_eq!(result, data.as_bytes());
        }

        // Only one connection needed for sequential calls.
        assert_eq!(pool.active_connections().await, 1);
    }

    #[tokio::test]
    async fn dynamic_pool_balances_by_pending() {
        let delay = std::time::Duration::from_millis(100);
        let addr = start_slow_server(delay).await;

        let pool = Arc::new(DynamicPool::new(addr, 3, Duration::from_secs(60)));

        // Send 6 concurrent calls through a pool with max_connections=3.
        let mut handles = Vec::new();
        for i in 0..6u32 {
            let pool = pool.clone();
            handles.push(tokio::spawn(async move {
                let data = format!("bal-{}", i);
                pool.send_call(ProgVers::new(1, 1), 1, data.as_bytes())
                    .await
                    .unwrap()
            }));
        }

        // Give time for connections to be opened.
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        // Pool should have opened up to 3 connections for concurrent load.
        let active = pool.active_connections().await;
        assert!(
            active >= 2 && active <= 3,
            "Expected 2-3 active connections under load, got {active}"
        );

        for handle in handles {
            handle.await.unwrap();
        }

        // After all calls complete, pending should be 0.
        assert_eq!(pool.pending_count().await, 0);
    }

    #[tokio::test]
    async fn dynamic_pool_respects_max_connections() {
        let delay = std::time::Duration::from_millis(100);
        let addr = start_slow_server(delay).await;

        let pool = Arc::new(DynamicPool::new(addr, 2, Duration::from_secs(60)));

        // Send 4 concurrent calls but only allow 2 connections.
        let mut handles = Vec::new();
        for i in 0..4u32 {
            let pool = pool.clone();
            handles.push(tokio::spawn(async move {
                let data = format!("max-{}", i);
                pool.send_call(ProgVers::new(1, 1), 1, data.as_bytes())
                    .await
                    .unwrap()
            }));
        }

        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        // Should never exceed 2 connections.
        assert!(pool.active_connections().await <= 2);

        for handle in handles {
            handle.await.unwrap();
        }
    }

    #[tokio::test]
    async fn dynamic_pool_evicts_idle_connections() {
        let addr = start_echo_server().await;

        let pool = Arc::new(DynamicPool::new(addr, 3, Duration::from_millis(200)));

        // Make a call to open a connection.
        pool.send_call(ProgVers::new(1, 1), 1, b"open")
            .await
            .unwrap();
        assert_eq!(pool.active_connections().await, 1);

        // Start the evictor.
        let evictor = pool.start_evictor();

        // Wait for the idle TTL + evictor interval to pass.
        tokio::time::sleep(Duration::from_millis(400)).await;

        // Connection should have been evicted.
        assert_eq!(
            pool.active_connections().await,
            0,
            "Idle connection should have been evicted"
        );

        evictor.abort();
    }

    #[tokio::test]
    async fn dynamic_pool_does_not_evict_busy_connections() {
        let delay = std::time::Duration::from_millis(500);
        let addr = start_slow_server(delay).await;

        let pool = Arc::new(DynamicPool::new(addr, 3, Duration::from_millis(100)));

        // Start a slow call.
        let pool2 = pool.clone();
        let call_handle = tokio::spawn(async move {
            pool2
                .send_call(ProgVers::new(1, 1), 1, b"busy")
                .await
                .unwrap()
        });

        // Give the call time to start.
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        // Start the evictor with a very short TTL.
        let evictor = pool.start_evictor();

        // Wait past the idle TTL.
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Connection should NOT be evicted because it has pending requests.
        assert_eq!(
            pool.active_connections().await,
            1,
            "Busy connection should not be evicted"
        );

        evictor.abort();
        call_handle.await.unwrap();
    }

    #[tokio::test]
    async fn dynamic_pool_reports_pending_count() {
        let delay = std::time::Duration::from_millis(200);
        let addr = start_slow_server(delay).await;

        let pool = Arc::new(DynamicPool::new(addr, 3, Duration::from_secs(60)));

        // Before any calls, pending should be 0.
        assert_eq!(pool.pending_count().await, 0);

        // Send 3 concurrent slow calls.
        let mut handles = Vec::new();
        for i in 0..3u32 {
            let pool = pool.clone();
            handles.push(tokio::spawn(async move {
                let data = format!("pend-{}", i);
                pool.send_call(ProgVers::new(1, 1), 1, data.as_bytes())
                    .await
                    .unwrap()
            }));
        }

        // Let the calls start.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Should have some pending calls.
        let pending = pool.pending_count().await;
        assert!(
            pending >= 1,
            "Expected at least 1 pending request, got {pending}"
        );

        for handle in handles {
            handle.await.unwrap();
        }

        // After completion, pending should be 0.
        assert_eq!(pool.pending_count().await, 0);
    }

    #[tokio::test]
    async fn dynamic_pool_error_propagation() {
        let addr = start_echo_server().await;

        let pool = DynamicPool::new(addr, 2, Duration::from_secs(60));

        // Invalid procedure should propagate the error.
        let result = pool.send_call(ProgVers::new(1, 1), 99, b"bad").await;
        assert_eq!(result, Err(RpcError::InvalidProcedure));

        // Pool should still work after an error.
        let result = pool.send_call(ProgVers::new(1, 1), 1, b"ok").await.unwrap();
        assert_eq!(result, b"ok");
    }

    #[tokio::test]
    async fn dynamic_pool_connect_failure() {
        let addr: SocketAddr = "127.0.0.1:1".parse().unwrap();
        let pool = DynamicPool::new(addr, 3, Duration::from_secs(60));
        let result = pool.send_call(ProgVers::new(1, 1), 1, b"fail").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn dynamic_pool_reopens_after_eviction() {
        let addr = start_echo_server().await;

        let pool = Arc::new(DynamicPool::new(addr, 3, Duration::from_millis(100)));

        // Open a connection.
        pool.send_call(ProgVers::new(1, 1), 1, b"first")
            .await
            .unwrap();
        assert_eq!(pool.active_connections().await, 1);

        // Start evictor and wait for eviction.
        let evictor = pool.start_evictor();
        tokio::time::sleep(Duration::from_millis(250)).await;
        assert_eq!(pool.active_connections().await, 0);

        // New call should open a fresh connection.
        let result = pool
            .send_call(ProgVers::new(1, 1), 1, b"second")
            .await
            .unwrap();
        assert_eq!(result, b"second");
        assert_eq!(pool.active_connections().await, 1);

        evictor.abort();
    }

    #[tokio::test]
    async fn fixed_pool_as_rpc_sender() {
        let addr = start_echo_server().await;
        let pool = Arc::new(FixedPool::connect(&addr, 2).await.unwrap());

        // Use pool through the RpcSender trait object
        let sender: Arc<dyn RpcSender> = pool;
        let result = sender
            .send_call(ProgVers::new(1, 1), 1, b"via-trait")
            .await
            .unwrap();
        assert_eq!(result, b"via-trait");
    }

    #[tokio::test]
    async fn dynamic_pool_as_rpc_sender() {
        let addr = start_echo_server().await;
        let pool = Arc::new(DynamicPool::new(addr, 2, Duration::from_secs(60)));

        // Use pool through the RpcSender trait object
        let sender: Arc<dyn RpcSender> = pool;
        let result = sender
            .send_call(ProgVers::new(1, 1), 1, b"via-trait")
            .await
            .unwrap();
        assert_eq!(result, b"via-trait");
    }
}
