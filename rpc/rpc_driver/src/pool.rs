//! Connection pools for RPC clients.
//!
//! Three pool implementations, modeling different x23/hogd connection patterns:
//!
//! - [`FixedSerialPool`] — Fixed-size pool with FIFO request dispatch. Each
//!   connection has a dedicated worker task that processes requests
//!   sequentially (1 in-flight per connection). Models x23 sync RPC
//!   (executor→DB).
//!
//! - [`DynamicPool`] — Lazy pool with min-pending load balancing. Opens
//!   connections on demand up to a maximum, routes each call to the
//!   connection with the fewest in-flight requests, and evicts idle
//!   connections after a configurable TTL. Many in-flight per connection.
//!   Models x23 async RPC (coordinator→mempool).
//!
//! - [`CpuSetPool`] — LLC-topology-aware pool. Creates one connection per
//!   L3 cache group, routes by the caller's current CPU via `sched_getcpu`.
//!   Models x23 `RpcMulticlient` / `PerCpuSet`.

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
pub struct FixedSerialPool {
    request_tx: mpsc::Sender<PoolRequest>,
    size: usize,
}

impl FixedSerialPool {
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
                tracing::error!(%addr, error = %e, pool_type = "fixed_serial", "pool connection failed");
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
impl RpcSender for FixedSerialPool {
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
                tracing::error!(addr = %self.addr, error = %e, pool_type = "dynamic", "pool connection failed");
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
                tracing::error!(addr = %self.addr, error = %e, pool_type = "dynamic", "pool connection failed");
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

// ---------------------------------------------------------------------------
// CpuSetPool — LLC-topology-aware connection pool
// ---------------------------------------------------------------------------

/// LLC-topology-aware connection pool.
///
/// Creates one connection per Last-Level Cache (L3) group, then routes each
/// RPC to the connection matching the caller's current CPU. This keeps RPC
/// request/response data in the same L3 cache, avoiding cross-CCX traffic
/// on multi-socket or chiplet architectures (AMD EPYC, Zen desktop).
///
/// Mirrors the x23 `RpcMulticlient` / `PerCpuSet` pattern.
///
/// Falls back to `cpu_id % num_connections` if LLC topology cannot be read
/// (non-Linux, container restrictions, etc.).
///
/// # Example
///
/// ```no_run
/// # async fn example() -> Result<(), rpc_driver::RpcError> {
/// use rpc_driver::{CpuSetPool, ProgVers, RpcSender};
///
/// let pool = CpuSetPool::connect(&"127.0.0.1:9876".parse().unwrap()).await?;
/// println!("opened {} connections (one per LLC)", pool.size());
/// let resp = pool.send_call(ProgVers::new(100, 1), 0, b"hello").await?;
/// # Ok(())
/// # }
/// ```
pub struct CpuSetPool {
    /// One connection per LLC bucket.
    connections: Vec<Arc<ConnectionHandle>>,
    /// Maps logical CPU id → index into `connections`.
    cpu_map: Vec<usize>,
}

impl std::fmt::Debug for CpuSetPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CpuSetPool")
            .field("connections", &self.connections.len())
            .field("cpus_mapped", &self.cpu_map.len())
            .finish()
    }
}

impl CpuSetPool {
    /// Connect to `addr` with one connection per LLC group, auto-detected
    /// from sysfs. Falls back to a single connection if topology is
    /// unavailable (non-Linux, container restrictions).
    pub async fn connect(addr: &SocketAddr) -> Result<Self, RpcError> {
        let (num_buckets, cpu_map) = detect_llc_topology();
        Self::connect_with_map(addr, num_buckets, cpu_map).await
    }

    /// Connect with an explicit number of connections and CPU→connection mapping.
    ///
    /// Useful for testing or when you want to override the auto-detected topology
    /// (e.g., one connection per physical core instead of per LLC).
    pub async fn connect_with_map(
        addr: &SocketAddr,
        num_connections: usize,
        cpu_map: Vec<usize>,
    ) -> Result<Self, RpcError> {
        if num_connections == 0 {
            return Err(RpcError::SystemError);
        }
        if cpu_map
            .iter()
            .any(|&b| b != usize::MAX && b >= num_connections)
        {
            return Err(RpcError::SystemError);
        }

        let mut connections = Vec::with_capacity(num_connections);
        for _ in 0..num_connections {
            let handle = connect_client(addr).await.map_err(|e| {
                tracing::error!(%addr, error = %e, pool_type = "cpu_set", "pool connection failed");
                RpcError::SystemError
            })?;
            connections.push(handle);
        }

        Ok(Self {
            connections,
            cpu_map,
        })
    }

    /// Returns the number of connections (one per LLC bucket).
    pub fn size(&self) -> usize {
        self.connections.len()
    }

    /// Get the connection for the current CPU.
    #[inline]
    fn connection_for_current_cpu(&self) -> &Arc<ConnectionHandle> {
        let cpu_id = current_cpu() as usize;
        let bucket = if cpu_id < self.cpu_map.len() {
            let b = self.cpu_map[cpu_id];
            if b < self.connections.len() {
                b
            } else {
                // Unmapped CPU (offline/no L3) — modular fallback
                cpu_id % self.connections.len()
            }
        } else {
            // CPU id out of range (hot-plugged?) — modular fallback
            cpu_id % self.connections.len()
        };
        &self.connections[bucket]
    }
}

#[async_trait]
impl RpcSender for CpuSetPool {
    async fn send_call(
        &self,
        progvers: ProgVers,
        proc: u32,
        data: &[u8],
    ) -> Result<Vec<u8>, RpcError> {
        self.connection_for_current_cpu()
            .send_call(progvers, proc, data)
            .await
    }
}

/// Get the current CPU id. Returns 0 on non-Linux platforms.
#[inline]
fn current_cpu() -> u32 {
    #[cfg(target_os = "linux")]
    {
        // SAFETY: sched_getcpu() is always safe to call, returns -1 on error.
        let cpu = unsafe { libc::sched_getcpu() };
        if cpu < 0 { 0 } else { cpu as u32 }
    }
    #[cfg(not(target_os = "linux"))]
    {
        0
    }
}

/// Detect LLC (L3) topology from sysfs.
///
/// Returns `(num_buckets, cpu_map)` where `cpu_map[cpu_id] = bucket_index`.
/// Falls back to a single bucket (one connection) if sysfs is unavailable.
fn detect_llc_topology() -> (usize, Vec<usize>) {
    #[cfg(target_os = "linux")]
    {
        if let Some(result) = detect_llc_topology_linux() {
            return result;
        }
    }
    // Fallback: one connection total (no topology info)
    let num_cpus = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let cpu_map = vec![0; num_cpus];
    (1, cpu_map)
}

/// Linux-specific LLC topology detection from sysfs.
#[cfg(target_os = "linux")]
fn detect_llc_topology_linux() -> Option<(usize, Vec<usize>)> {
    use std::collections::BTreeMap;
    use std::fs;

    let num_cpus = std::thread::available_parallelism().map(|n| n.get()).ok()?;

    // Read each CPU's L3 shared_cpu_list to determine LLC groups.
    // CPUs sharing the same list string belong to the same LLC.
    let mut llc_groups: BTreeMap<String, usize> = BTreeMap::new();
    let mut cpu_map = vec![usize::MAX; num_cpus];
    let mut next_bucket = 0;

    for (cpu_id, slot) in cpu_map.iter_mut().enumerate() {
        let path = format!("/sys/devices/system/cpu/cpu{cpu_id}/cache/index3/shared_cpu_list");
        let shared = match fs::read_to_string(&path) {
            Ok(s) => s.trim().to_string(),
            Err(_) => continue, // offline or no L3 — left as sentinel
        };

        *slot = *llc_groups.entry(shared).or_insert_with(|| {
            let b = next_bucket;
            next_bucket += 1;
            b
        });
    }

    if next_bucket == 0 {
        return None;
    }

    tracing::info!(groups = next_bucket, cpus = num_cpus, "detected LLC groups");
    Some((next_bucket, cpu_map))
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

        let pool = FixedSerialPool::connect(&addr, 3).await.unwrap();
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

        let pool = Arc::new(FixedSerialPool::connect(&addr, 3).await.unwrap());

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

        let pool = FixedSerialPool::connect(&addr, 2).await.unwrap();

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
        let result = FixedSerialPool::connect(&addr, 0).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn fixed_pool_single_connection() {
        let addr = start_echo_server().await;

        let pool = FixedSerialPool::connect(&addr, 1).await.unwrap();
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
        let result = FixedSerialPool::connect(&addr, 3).await;
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
            (2..=3).contains(&active),
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
        let pool = Arc::new(FixedSerialPool::connect(&addr, 2).await.unwrap());

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

    // -- CpuSetPool tests -------------------------------------------------------

    #[tokio::test]
    async fn cpu_pool_auto_detect() {
        let addr = start_echo_server().await;
        let pool = CpuSetPool::connect(&addr).await.unwrap();
        assert!(pool.size() >= 1);

        // Basic echo
        let result = pool
            .send_call(ProgVers::new(1, 1), 1, b"cpu-pool")
            .await
            .unwrap();
        assert_eq!(result, b"cpu-pool");
    }

    #[tokio::test]
    async fn cpu_pool_explicit_map() {
        let addr = start_echo_server().await;
        // 2 connections, CPUs 0-3 → bucket 0, CPUs 4-7 → bucket 1
        let cpu_map = vec![0, 0, 0, 0, 1, 1, 1, 1];
        let pool = CpuSetPool::connect_with_map(&addr, 2, cpu_map)
            .await
            .unwrap();
        assert_eq!(pool.size(), 2);

        for i in 0..10u32 {
            let data = format!("call-{i}");
            let result = pool
                .send_call(ProgVers::new(1, 1), 1, data.as_bytes())
                .await
                .unwrap();
            assert_eq!(result, data.as_bytes());
        }
    }

    #[tokio::test]
    async fn cpu_pool_as_rpc_sender() {
        let addr = start_echo_server().await;
        let pool = Arc::new(CpuSetPool::connect(&addr).await.unwrap());

        let sender: Arc<dyn RpcSender> = pool;
        let result = sender
            .send_call(ProgVers::new(1, 1), 1, b"via-trait")
            .await
            .unwrap();
        assert_eq!(result, b"via-trait");
    }
}
