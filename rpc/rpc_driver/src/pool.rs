//! Connection pools for RPC clients.
//!
//! Four pool implementations, following x23's client connection patterns:
//!
//! - [`FixedSerialPool`] — Fixed-size pool with FIFO request dispatch. Each
//!   connection has a dedicated worker task that processes requests
//!   sequentially (1 in-flight per connection). Models x23 sync RPC.
//!
//! - [`DynamicPool`] — Lazy pool with min-pending load balancing. Opens
//!   connections on demand up to a maximum, routes each call to the
//!   connection with the fewest in-flight requests, and evicts idle
//!   connections after a configurable TTL. Many in-flight per connection.
//!   Models x23 async RPC.
//!
//! - [`CpuSetPool`] — LLC-topology-aware pool. Creates one connection per
//!   L3 cache group, routes by the caller's current CPU via `sched_getcpu`.
//!   Models x23 `RpcMulticlient` / `PerCpuSet`.
//!
//! - [`MultiplexPool`] — Fixed-size pool of xid-multiplexed connections.
//!   Every connection carries many in-flight calls; each call is sent from
//!   the caller's own task on the least-loaded connection, with no worker
//!   task, queue, or payload copy in between. Replaces connections the peer
//!   closes. Models x23 async RPC over a handful of connections.

use std::net::SocketAddr;
use std::sync::{
    Arc, Mutex as StdMutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::{Notify, OwnedSemaphorePermit, Semaphore, mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio::time::Instant;

#[cfg(test)]
use crate::connect_client;
use crate::{
    CallBuf, ConnectionConfig, ConnectionHandle, ProgVers, RpcError, RpcResponse, RpcSender,
    connect_client_with_config,
};
#[cfg(test)]
use std::future::Future;

#[cfg(test)]
type ConnectFuture = std::pin::Pin<
    Box<dyn Future<Output = Result<crate::connection::ClientConnection, std::io::Error>> + Send>,
>;
#[cfg(test)]
type ConnectOverride = Arc<dyn Fn() -> ConnectFuture + Send + Sync>;

/// Opens connections to one address with one stable configuration.
///
/// Fixed-pool workers retain this factory so replacement connections receive
/// the same configuration as the connections created during construction.
#[derive(Clone)]
struct ConnectionFactory {
    addr: SocketAddr,
    config: ConnectionConfig,
    #[cfg(test)]
    connect_override: Option<ConnectOverride>,
}

impl ConnectionFactory {
    fn new(addr: SocketAddr, config: ConnectionConfig) -> Self {
        Self {
            addr,
            config,
            #[cfg(test)]
            connect_override: None,
        }
    }

    async fn connect(&self) -> Result<crate::connection::ClientConnection, std::io::Error> {
        #[cfg(test)]
        if let Some(connect) = &self.connect_override {
            return connect().await;
        }
        connect_client_with_config(&self.addr, self.config.clone()).await
    }

    #[cfg(test)]
    fn with_override<F, Fut>(addr: SocketAddr, connect: F) -> Self
    where
        F: FnMut() -> Fut + Send + 'static,
        Fut: Future<Output = Result<crate::connection::ClientConnection, std::io::Error>>
            + Send
            + 'static,
    {
        let connect = Arc::new(StdMutex::new(connect));
        let connect_override = Arc::new(move || {
            let future = connect.lock().unwrap()();
            Box::pin(future) as ConnectFuture
        });
        Self {
            addr,
            config: ConnectionConfig::default(),
            connect_override: Some(connect_override),
        }
    }
}

/// A request dispatched through the pool to a worker.
struct PoolRequest {
    progvers: ProgVers,
    proc_num: u32,
    call: CallBuf,
    queue_permit: OwnedSemaphorePermit,
    /// `Some` for normal calls, `None` for fire-and-forget.
    reply: Option<oneshot::Sender<Result<RpcResponse, RpcError>>>,
}

#[derive(Debug)]
struct FixedSerialPoolInner {
    closed: AtomicBool,
    request_tx: StdMutex<Option<mpsc::UnboundedSender<PoolRequest>>>,
    queue_slots: Arc<Semaphore>,
    worker_handles: StdMutex<Vec<JoinHandle<()>>>,
    connections: Vec<Arc<StdMutex<Option<Arc<ConnectionHandle>>>>>,
}

struct FixedWorkerConnectionGuard(Arc<StdMutex<Option<Arc<ConnectionHandle>>>>);

impl Drop for FixedWorkerConnectionGuard {
    fn drop(&mut self) {
        if let Some(connection) = self.0.lock().unwrap().take() {
            connection.begin_poison();
        }
    }
}

/// Fixed-size connection pool with FIFO request dispatch.
///
/// Wraps N [`ConnectionHandle`] connections to a single address. A shared
/// `tokio::sync::mpsc` channel distributes requests to connection workers
/// in FIFO order — one call at a time per connection. Workers skip canceled
/// queued calls before sending them. By default, canceling an active call
/// or detecting that its connection was poisoned closes and replaces that
/// worker's connection before it accepts more work. The request that observes
/// an unexpected connection loss still returns an error; later requests use
/// the replacement connection. [`CanceledCallPolicy::PoisonConnection`]
/// instead terminates the affected worker, leaving any other workers available
/// at reduced capacity until the owner discards the pool.
#[derive(Debug)]
pub struct FixedSerialPool {
    inner: Arc<FixedSerialPoolInner>,
    size: usize,
}

/// Action taken when an active fixed-pool request is canceled or its connection
/// becomes poisoned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanceledCallPolicy {
    /// Replace the affected connection before the worker accepts more work.
    ReplaceConnection,
    /// Poison the affected connection and terminate its worker.
    ///
    /// Owners should discard the pool after canceling a call or observing a
    /// connection failure. This policy is intended for protocols where
    /// reconnects require an external handshake or quarantine before the
    /// client can become visible again.
    PoisonConnection,
}

/// Close connections that never made it into a pool: poison them all, then
/// wait for their shutdowns to complete.
///
/// Connections not yet installed in a pool are kept as [`ClientConnection`]
/// so their RAII drop poisons them if the caller is canceled between awaits;
/// this is the explicit-failure-path counterpart. Pools call
/// [`ClientConnection::into_handle`] only at the point of installation.
///
/// [`ClientConnection`]: crate::connection::ClientConnection
/// [`ClientConnection::into_handle`]: crate::connection::ClientConnection::into_handle
async fn close_all(connections: Vec<crate::connection::ClientConnection>) {
    let shutdowns = connections
        .into_iter()
        .map(|client| client.into_handle().begin_poison())
        .collect::<Vec<_>>();
    finish_shutdowns(shutdowns).await;
}

/// Claim responsibility for shutting down a pool.
fn begin_pool_shutdown(closed: &AtomicBool) -> bool {
    !closed.swap(true, Ordering::SeqCst)
}

async fn finish_shutdowns(shutdowns: Vec<crate::connection::ConnectionShutdown>) {
    for shutdown in shutdowns {
        shutdown.finish().await;
    }
}

/// Retire a fixed worker's current connection and apply its recovery policy.
///
/// Returns `true` after installing a replacement, or `false` when the worker
/// should terminate under [`CanceledCallPolicy::PoisonConnection`].
async fn recover_fixed_worker_connection(
    connection_slot: &Arc<StdMutex<Option<Arc<ConnectionHandle>>>>,
    client: &mut Arc<ConnectionHandle>,
    connection_factory: &ConnectionFactory,
    canceled_call_policy: CanceledCallPolicy,
    reason: &'static str,
) -> bool {
    connection_slot.lock().unwrap().take();
    client.begin_poison().finish().await;
    if canceled_call_policy == CanceledCallPolicy::PoisonConnection {
        return false;
    }

    let mut retry_delay = Duration::from_millis(100);
    let mut first_failure = true;
    loop {
        match connection_factory.connect().await {
            Ok(connection) => {
                let replacement = connection.into_handle();
                *connection_slot.lock().unwrap() = Some(replacement.clone());
                *client = replacement;
                return true;
            }
            Err(error) => {
                if first_failure {
                    tracing::warn!(
                        addr = %connection_factory.addr,
                        %error,
                        pool_type = "fixed_serial",
                        reason,
                        "failed to replace fixed-pool connection"
                    );
                    first_failure = false;
                } else {
                    tracing::debug!(
                        addr = %connection_factory.addr,
                        %error,
                        pool_type = "fixed_serial",
                        reason,
                        "fixed-pool connection replacement still pending"
                    );
                }
                tokio::time::sleep(retry_delay).await;
                retry_delay = retry_delay.saturating_mul(2).min(Duration::from_secs(5));
            }
        }
    }
}

impl FixedSerialPoolInner {
    fn begin_shutdown(
        &self,
    ) -> (
        Vec<JoinHandle<()>>,
        Vec<crate::connection::ConnectionShutdown>,
    ) {
        if !begin_pool_shutdown(&self.closed) {
            return (Vec::new(), Vec::new());
        }

        self.queue_slots.close();
        self.request_tx.lock().unwrap().take();

        let connection_shutdowns = self
            .connections
            .iter()
            .filter_map(|slot| slot.lock().unwrap().take())
            .map(|conn| conn.begin_poison())
            .collect();

        let mut workers = self.worker_handles.lock().unwrap();
        for join in workers.iter() {
            join.abort();
        }
        (std::mem::take(&mut *workers), connection_shutdowns)
    }
}

impl FixedSerialPool {
    /// Connect `pool_size` clients to `addr`, each with a dedicated worker task.
    ///
    /// All connections are established eagerly. If any connection fails, the
    /// entire pool creation fails. Connections use
    /// [`ConnectionConfig::default`].
    ///
    /// # Errors
    ///
    /// Returns [`RpcError::InvalidPoolConfiguration`] when `pool_size` is zero
    /// and [`RpcError::ConnectFailed`] when a connection cannot be opened.
    pub async fn connect(addr: &SocketAddr, pool_size: usize) -> Result<Self, RpcError> {
        Self::connect_with_config_and_canceled_call_policy(
            addr,
            pool_size,
            ConnectionConfig::default(),
            CanceledCallPolicy::ReplaceConnection,
        )
        .await
    }

    /// Connect `pool_size` clients using `config`.
    ///
    /// The configuration is also applied to replacement connections opened
    /// after an active call is canceled.
    ///
    /// # Errors
    ///
    /// Returns [`RpcError::InvalidPoolConfiguration`] when `pool_size` is zero
    /// and [`RpcError::ConnectFailed`] when a connection cannot be opened.
    pub async fn connect_with_config(
        addr: &SocketAddr,
        pool_size: usize,
        config: ConnectionConfig,
    ) -> Result<Self, RpcError> {
        Self::connect_with_config_and_canceled_call_policy(
            addr,
            pool_size,
            config,
            CanceledCallPolicy::ReplaceConnection,
        )
        .await
    }

    /// Connect with an explicit policy for active calls whose caller cancels.
    ///
    /// Connections use [`ConnectionConfig::default`].
    ///
    /// # Errors
    ///
    /// Returns [`RpcError::InvalidPoolConfiguration`] when `pool_size` is zero
    /// and [`RpcError::ConnectFailed`] when a connection cannot be opened.
    pub async fn connect_with_canceled_call_policy(
        addr: &SocketAddr,
        pool_size: usize,
        canceled_call_policy: CanceledCallPolicy,
    ) -> Result<Self, RpcError> {
        Self::connect_with_config_and_canceled_call_policy(
            addr,
            pool_size,
            ConnectionConfig::default(),
            canceled_call_policy,
        )
        .await
    }

    /// Connect with a custom connection configuration and cancellation policy.
    ///
    /// `config` is applied both during initial pool construction and whenever
    /// [`CanceledCallPolicy::ReplaceConnection`] replaces a connection.
    ///
    /// # Errors
    ///
    /// Returns [`RpcError::InvalidPoolConfiguration`] when `pool_size` is zero
    /// and [`RpcError::ConnectFailed`] when a connection cannot be opened.
    pub async fn connect_with_config_and_canceled_call_policy(
        addr: &SocketAddr,
        pool_size: usize,
        config: ConnectionConfig,
        canceled_call_policy: CanceledCallPolicy,
    ) -> Result<Self, RpcError> {
        Self::connect_with_factory(
            pool_size,
            canceled_call_policy,
            ConnectionFactory::new(*addr, config),
        )
        .await
    }

    #[cfg(test)]
    async fn connect_with<F, Fut>(
        addr: &SocketAddr,
        pool_size: usize,
        canceled_call_policy: CanceledCallPolicy,
        connect: F,
    ) -> Result<Self, RpcError>
    where
        F: FnMut() -> Fut + Send + 'static,
        Fut: Future<Output = Result<crate::connection::ClientConnection, std::io::Error>>
            + Send
            + 'static,
    {
        Self::connect_with_factory(
            pool_size,
            canceled_call_policy,
            ConnectionFactory::with_override(*addr, connect),
        )
        .await
    }

    async fn connect_with_factory(
        pool_size: usize,
        canceled_call_policy: CanceledCallPolicy,
        connection_factory: ConnectionFactory,
    ) -> Result<Self, RpcError> {
        if pool_size == 0 {
            return Err(RpcError::InvalidPoolConfiguration);
        }

        let queue_capacity = pool_size.saturating_mul(16).max(16);
        let queue_slots = Arc::new(Semaphore::new(queue_capacity));
        let (tx, rx) = mpsc::unbounded_channel::<PoolRequest>();
        let rx = Arc::new(tokio::sync::Mutex::new(rx));

        // `pending` holds `ClientConnection`s, whose RAII drop poisons the
        // partial connections if construction is canceled mid-loop.
        let mut pending = Vec::with_capacity(pool_size);
        for _ in 0..pool_size {
            match connection_factory.connect().await {
                Ok(client) => pending.push(client),
                Err(e) => {
                    close_all(pending).await;
                    tracing::error!(
                        addr = %connection_factory.addr,
                        error = %e,
                        pool_type = "fixed_serial",
                        "pool connection failed"
                    );
                    return Err(RpcError::ConnectFailed);
                }
            }
        }
        let connections = pending
            .into_iter()
            .map(|client| Arc::new(StdMutex::new(Some(client.into_handle()))))
            .collect::<Vec<_>>();

        let inner = Arc::new(FixedSerialPoolInner {
            closed: AtomicBool::new(false),
            request_tx: StdMutex::new(Some(tx)),
            queue_slots,
            worker_handles: StdMutex::new(Vec::with_capacity(pool_size)),
            connections: connections.clone(),
        });

        for connection_slot in connections {
            let worker_rx = rx.clone();
            let worker_factory = connection_factory.clone();
            let mut client = connection_slot
                .lock()
                .unwrap()
                .clone()
                .expect("new fixed pool worker should have a connection");
            let join = tokio::spawn(async move {
                let _connection_guard = FixedWorkerConnectionGuard(connection_slot.clone());
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
                    let PoolRequest {
                        progvers,
                        proc_num,
                        call,
                        queue_permit,
                        reply,
                    } = req;
                    drop(queue_permit);

                    // The read task can poison an idle connection after its peer
                    // closes. Recover before dispatch so the next queued request
                    // does not have to fail merely to trigger replacement.
                    if client.is_poisoned()
                        && !recover_fixed_worker_connection(
                            &connection_slot,
                            &mut client,
                            &worker_factory,
                            canceled_call_policy,
                            "poison detected before request dispatch",
                        )
                        .await
                    {
                        break;
                    }

                    match reply {
                        Some(mut reply_tx) => {
                            if reply_tx.is_closed() {
                                continue;
                            }
                            let result = tokio::select! {
                                result = client.send_call(progvers, proc_num, call) => {
                                    Some(result)
                                }
                                () = reply_tx.closed() => None,
                            };
                            if let Some(result) = result {
                                let connection_poisoned = client.is_poisoned();
                                let _ = reply_tx.send(result);
                                if connection_poisoned
                                    && !recover_fixed_worker_connection(
                                        &connection_slot,
                                        &mut client,
                                        &worker_factory,
                                        canceled_call_policy,
                                        "connection poisoned during RPC call",
                                    )
                                    .await
                                {
                                    break;
                                }
                            } else if !recover_fixed_worker_connection(
                                &connection_slot,
                                &mut client,
                                &worker_factory,
                                canceled_call_policy,
                                "active RPC call canceled",
                            )
                            .await
                            {
                                break;
                            }
                        }
                        None => {
                            if let Err(e) =
                                client.send_fire_and_forget(progvers, proc_num, call).await
                            {
                                tracing::warn!(error = %e, "fire-and-forget send failed");
                            }
                            if client.is_poisoned()
                                && !recover_fixed_worker_connection(
                                    &connection_slot,
                                    &mut client,
                                    &worker_factory,
                                    canceled_call_policy,
                                    "connection poisoned during fire-and-forget call",
                                )
                                .await
                            {
                                break;
                            }
                        }
                    }
                }
            });
            inner.worker_handles.lock().unwrap().push(join);
        }

        Ok(Self {
            inner,
            size: pool_size,
        })
    }

    async fn enqueue_request(
        &self,
        progvers: ProgVers,
        proc: u32,
        call: CallBuf,
        reply: Option<oneshot::Sender<Result<RpcResponse, RpcError>>>,
    ) -> Result<(), RpcError> {
        let queue_permit = self
            .inner
            .queue_slots
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| RpcError::PoolClosed)?;
        let request_tx = self.inner.request_tx.lock().unwrap();
        let request_tx = request_tx.as_ref().ok_or(RpcError::PoolClosed)?;
        request_tx
            .send(PoolRequest {
                progvers,
                proc_num: proc,
                call,
                queue_permit,
                reply,
            })
            .map_err(|_| RpcError::PoolClosed)
    }

    /// Returns the configured pool size.
    ///
    /// This is not a live connection count. A pool using
    /// [`CanceledCallPolicy::PoisonConnection`] can have fewer active workers
    /// after a caller cancels an in-flight request or a connection becomes
    /// poisoned.
    pub fn size(&self) -> usize {
        self.size
    }

    /// Shut down the pool and wait for its workers and connections to stop.
    ///
    /// New calls fail with [`RpcError::PoolClosed`]. Calling `shutdown` more
    /// than once is safe.
    pub async fn shutdown(&self) {
        let (workers, connection_shutdowns) = self.inner.begin_shutdown();
        for join in workers {
            let _ = join.await;
        }
        finish_shutdowns(connection_shutdowns).await;
    }
}

impl Drop for FixedSerialPool {
    fn drop(&mut self) {
        let _ = self.inner.begin_shutdown();
    }
}

#[async_trait]
/// `send_call_decoded` keeps the trait's copying default here: calls run on
/// the pool's worker tasks, so a borrowed decoder would have to cross tasks.
impl RpcSender for FixedSerialPool {
    fn call_buf(&self) -> CallBuf {
        CallBuf::from_spare(crate::connection::take_buffer())
    }

    /// Send an RPC call through the pool.
    ///
    /// The request is dispatched to the next idle worker via FIFO ordering.
    async fn send_call(
        &self,
        progvers: ProgVers,
        proc: u32,
        call: CallBuf,
    ) -> Result<RpcResponse, RpcError> {
        let (reply_tx, rx) = oneshot::channel();
        self.enqueue_request(progvers, proc, call, Some(reply_tx))
            .await?;
        rx.await.map_err(|_| {
            if self.inner.closed.load(Ordering::SeqCst) {
                RpcError::PoolClosed
            } else {
                RpcError::Canceled
            }
        })?
    }

    async fn send_fire_and_forget(
        &self,
        progvers: ProgVers,
        proc: u32,
        call: CallBuf,
    ) -> Result<(), RpcError> {
        self.enqueue_request(progvers, proc, call, None).await
    }
}

/// A connection tracked by the dynamic pool.
struct PooledConnection {
    handle: Arc<ConnectionHandle>,
    last_used: Instant,
}

/// Reservation of a [`DynamicPool`] connection slot while a connect is in
/// flight.
///
/// Dropping the reservation — including when the leasing caller is canceled
/// at an await — releases the slot and wakes lease waiters, so `connecting`
/// cannot drift out of sync and silently shrink the pool's capacity.
struct ConnectingSlot<'a> {
    pool: &'a DynamicPool,
}

impl<'a> ConnectingSlot<'a> {
    fn reserve(pool: &'a DynamicPool) -> Self {
        pool.connecting.fetch_add(1, Ordering::SeqCst);
        Self { pool }
    }
}

impl Drop for ConnectingSlot<'_> {
    fn drop(&mut self) {
        self.pool.connecting.fetch_sub(1, Ordering::SeqCst);
        self.pool.connection_ready.notify_waiters();
    }
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
    max_connections: usize,
    idle_ttl: Duration,
    connection_factory: ConnectionFactory,
    closed: AtomicBool,
    connecting: std::sync::atomic::AtomicUsize,
    connections: tokio::sync::Mutex<Vec<PooledConnection>>,
    connection_ready: Notify,
    evictor_abort: StdMutex<Option<tokio::task::AbortHandle>>,
}

// Manual Debug impl because PooledConnection contains ConnectionHandle (which
// is Debug) but we want to show pool-level stats, not connection internals.
impl std::fmt::Debug for DynamicPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DynamicPool")
            .field("addr", &self.connection_factory.addr)
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
    /// `config` is applied to every connection the pool opens. Pass `None`
    /// to use [`ConnectionConfig::default`].
    ///
    /// # Panics
    ///
    /// Panics if `max_connections` is 0 or `idle_ttl` is zero.
    pub fn new(
        addr: SocketAddr,
        max_connections: usize,
        idle_ttl: Duration,
        config: Option<ConnectionConfig>,
    ) -> Self {
        assert!(max_connections > 0, "max_connections must be at least 1");
        assert!(!idle_ttl.is_zero(), "idle_ttl must be greater than zero");
        Self {
            max_connections,
            idle_ttl,
            connection_factory: ConnectionFactory::new(addr, config.unwrap_or_default()),
            closed: AtomicBool::new(false),
            connecting: std::sync::atomic::AtomicUsize::new(0),
            connections: tokio::sync::Mutex::new(Vec::new()),
            connection_ready: Notify::new(),
            evictor_abort: StdMutex::new(None),
        }
    }

    fn take_evictor_abort(&self) -> Option<tokio::task::AbortHandle> {
        self.evictor_abort.lock().unwrap().take()
    }

    fn begin_drop_shutdown(&mut self) {
        if !begin_pool_shutdown(&self.closed) {
            return;
        }

        if let Some(abort) = self.evictor_abort.get_mut().unwrap().take() {
            abort.abort();
        }

        let conns = self.connections.get_mut();
        for conn in conns.drain(..) {
            conn.handle.begin_poison();
        }
    }

    /// Open a new connection to the pool's address.
    async fn open_connection(&self) -> Result<crate::connection::ClientConnection, std::io::Error> {
        self.connection_factory.connect().await
    }

    /// Select or create a connection with the fewest pending requests.
    async fn lease_connection(&self) -> Result<Arc<ConnectionHandle>, RpcError> {
        loop {
            if self.closed.load(Ordering::SeqCst) {
                return Err(RpcError::PoolClosed);
            }

            let mut connect_slot = None;
            let mut should_wait = false;

            {
                let mut conns = self.connections.lock().await;

                if self.closed.load(Ordering::SeqCst) {
                    return Err(RpcError::PoolClosed);
                }

                // Remove poisoned connections before any selection logic.
                let mut i = 0;
                while i < conns.len() {
                    if conns[i].handle.is_poisoned() {
                        conns.swap_remove(i);
                    } else {
                        i += 1;
                    }
                }

                if !conns.is_empty() {
                    let (min_idx, min_pending) = conns
                        .iter()
                        .enumerate()
                        .map(|(i, c)| (i, c.handle.num_pending_requests()))
                        .min_by_key(|&(_, p)| p)
                        .expect("bug: conns is non-empty but min_by_key returned None");

                    if min_pending == 0 {
                        conns[min_idx].last_used = Instant::now();
                        return Ok(conns[min_idx].handle.clone());
                    }

                    let total_connections = conns.len() + self.connecting.load(Ordering::SeqCst);
                    if total_connections < self.max_connections {
                        connect_slot = Some(ConnectingSlot::reserve(self));
                    } else {
                        conns[min_idx].last_used = Instant::now();
                        return Ok(conns[min_idx].handle.clone());
                    }
                } else {
                    let total_connections = self.connecting.load(Ordering::SeqCst);
                    if total_connections < self.max_connections {
                        connect_slot = Some(ConnectingSlot::reserve(self));
                    } else {
                        should_wait = true;
                    }
                }
            }

            if should_wait {
                // Register interest before re-checking, because
                // `notify_waiters` only wakes futures that are already
                // enabled: a wakeup fired after `enable` is delivered, and
                // one fired before it is compensated by the re-check.
                let notified = self.connection_ready.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                {
                    let conns = self.connections.lock().await;
                    // `closed` must be part of the re-check: a shutdown that
                    // notified before `enable` would otherwise be lost, and a
                    // forever-pending connect attempt could hold the pool at
                    // capacity so no further wakeup arrives.
                    if self.closed.load(Ordering::SeqCst) {
                        return Err(RpcError::PoolClosed);
                    }
                    let total_connections = conns.len() + self.connecting.load(Ordering::SeqCst);
                    if !conns.is_empty() || total_connections < self.max_connections {
                        continue;
                    }
                }
                notified.await;
                continue;
            }

            if let Some(_slot) = connect_slot {
                // `_slot` releases the reservation and wakes lease waiters on
                // every exit from this block, including cancellation at any
                // await; `client`'s RAII drop poisons the new connection
                // unless it is installed in `conns` first.
                let client = match self.open_connection().await {
                    Ok(client) => client,
                    Err(e) => {
                        tracing::error!(
                            addr = %self.connection_factory.addr,
                            error = %e,
                            pool_type = "dynamic",
                            "pool connection failed"
                        );
                        return Err(if self.closed.load(Ordering::SeqCst) {
                            RpcError::PoolClosed
                        } else {
                            RpcError::ConnectFailed
                        });
                    }
                };

                let mut conns = self.connections.lock().await;
                if self.closed.load(Ordering::SeqCst) {
                    drop(conns);
                    client.shutdown().await;
                    return Err(RpcError::PoolClosed);
                }

                let handle = client.into_handle();
                conns.push(PooledConnection {
                    handle: handle.clone(),
                    last_used: Instant::now(),
                });
                return Ok(handle);
            }
        }
    }

    async fn connection_for_request(&self) -> Result<Arc<ConnectionHandle>, RpcError> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(RpcError::PoolClosed);
        }
        let conn = self.lease_connection().await?;
        if self.closed.load(Ordering::SeqCst) {
            return Err(RpcError::PoolClosed);
        }
        Ok(conn)
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

    /// Remove connections that have been idle longer than `idle_ttl`, or that
    /// have been poisoned (remote peer closed the connection).
    async fn evict_idle(&self) {
        let mut conns = self.connections.lock().await;
        let mut i = 0;
        while i < conns.len() {
            let poisoned = conns[i].handle.is_poisoned();
            let idle_duration = conns[i].last_used.elapsed();
            let pending = conns[i].handle.num_pending_requests();
            // Poisoned connections are evicted unconditionally: the read task has
            // already called poison(), waking all in-flight waiters. Those futures
            // will resolve to SystemError and decrement pending_count on their own.
            if poisoned || (pending == 0 && idle_duration > self.idle_ttl) {
                let removed = conns.swap_remove(i);
                if !poisoned {
                    // Poison in background to avoid holding the lock during
                    // async poison. Already-poisoned connections don't need this
                    // — the read task already called poison() when it detected EOF.
                    tokio::spawn(async move {
                        removed.handle.poison().await;
                    });
                }
                // Don't increment i — swap_remove moved the last element here.
            } else {
                i += 1;
            }
        }
    }

    /// Spawn a background task that periodically evicts idle connections.
    ///
    /// The task runs until the returned [`tokio::task::JoinHandle`] is
    /// aborted, the pool is shut down, or the pool is dropped. Starting a new
    /// evictor aborts the previous one. The check interval is half the
    /// nonzero `idle_ttl`.
    pub fn start_evictor(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        let weak = Arc::downgrade(self);
        let interval = self.idle_ttl / 2;
        let join = tokio::spawn(async move {
            loop {
                tokio::time::sleep(interval).await;
                let Some(pool) = weak.upgrade() else {
                    break;
                };
                if pool.closed.load(Ordering::SeqCst) {
                    break;
                }
                pool.evict_idle().await;
            }
        });
        let abort = join.abort_handle();
        if let Some(prev) = self.evictor_abort.lock().unwrap().replace(abort) {
            prev.abort();
        }
        join
    }

    /// Shut down the pool and wait for all installed connections to stop.
    ///
    /// New and waiting leases fail with [`RpcError::PoolClosed`]. Calling
    /// `shutdown` more than once is safe.
    pub async fn shutdown(&self) {
        if !begin_pool_shutdown(&self.closed) {
            return;
        }

        self.connection_ready.notify_waiters();
        if let Some(abort) = self.take_evictor_abort() {
            abort.abort();
        }

        let shutdowns = {
            let mut conns = self.connections.lock().await;
            conns
                .drain(..)
                .map(|conn| conn.handle.begin_poison())
                .collect::<Vec<_>>()
        };

        finish_shutdowns(shutdowns).await;
    }
}

impl Drop for DynamicPool {
    fn drop(&mut self) {
        self.begin_drop_shutdown();
    }
}

#[async_trait]
impl RpcSender for DynamicPool {
    fn call_buf(&self) -> CallBuf {
        CallBuf::from_spare(crate::connection::take_buffer())
    }

    /// Send an RPC call through the pool.
    ///
    /// Routed to the connection with the fewest pending requests.
    async fn send_call(
        &self,
        progvers: ProgVers,
        proc: u32,
        call: CallBuf,
    ) -> Result<RpcResponse, RpcError> {
        let conn = self.connection_for_request().await?;
        let result = conn.send_call(progvers, proc, call).await;
        self.touch_connection(&conn);
        result
    }

    /// Forwarded to the chosen connection, which decodes on its reader.
    async unsafe fn send_call_decoded(
        &self,
        progvers: ProgVers,
        proc: u32,
        call: CallBuf,
        decode: &mut (dyn for<'reply> FnMut(&'reply [u8]) -> Result<(), RpcError> + Send),
    ) -> Result<(), RpcError> {
        let conn = self.connection_for_request().await?;
        // SAFETY: the caller's contract, forwarded; the call runs in this future.
        let result = unsafe { conn.send_call_decoded(progvers, proc, call, decode).await };
        self.touch_connection(&conn);
        result
    }

    async fn send_fire_and_forget(
        &self,
        progvers: ProgVers,
        proc: u32,
        call: CallBuf,
    ) -> Result<(), RpcError> {
        let conn = self.connection_for_request().await?;
        let result = conn.send_fire_and_forget(progvers, proc, call).await;
        self.touch_connection(&conn);
        result
    }
}

// ---------------------------------------------------------------------------
// MultiplexPool — fixed set of xid-multiplexed connections
// ---------------------------------------------------------------------------

/// One connection slot of a [`MultiplexPool`].
struct MultiplexSlot {
    handle: std::sync::RwLock<Arc<ConnectionHandle>>,
    /// Calls this pool currently has in flight on the slot.
    in_flight: std::sync::atomic::AtomicUsize,
    /// Set while a replacement connection is being opened.
    replacing: AtomicBool,
}

struct MultiplexInner {
    slots: Vec<MultiplexSlot>,
    /// Rotates the starting point of the least-loaded scan so idle
    /// connections are used in turn rather than always the first.
    cursor: std::sync::atomic::AtomicUsize,
    connection_factory: ConnectionFactory,
    closed: AtomicBool,
    /// Wakes replacement tasks waiting on a connect or a backoff sleep when
    /// the pool closes.
    closed_notify: Notify,
    replacements: StdMutex<Vec<JoinHandle<()>>>,
}

/// Fixed-size pool of xid-multiplexed connections.
///
/// The wire protocol matches replies to calls by xid, so one connection can
/// carry any number of calls at once. This pool never serializes callers:
/// each call picks the connection with the fewest calls in flight and sends
/// on it directly from the caller's task. There is no worker task, request
/// queue, completion channel, or payload copy between a caller and the
/// socket, which is the shape of x23's asynchronous RPC client.
///
/// All connections are opened eagerly. When a peer closes one, calls in
/// flight on it fail with [`RpcError::SystemError`], the pool opens a
/// replacement in the background with backoff, and routing prefers the
/// remaining healthy connections meanwhile: a call routed to a connection
/// the peer has closed moves to a healthy one instead of failing. Only when
/// every connection is down does a call fail. A caller that drops its call
/// future leaves the connection usable: the reply for that xid is discarded
/// when it arrives.
///
/// Use a small number of connections, one per core the peer serves with or
/// so, to spread per-connection read-loop work; more only adds sockets.
#[derive(Debug)]
pub struct MultiplexPool {
    inner: Arc<MultiplexInner>,
}

impl std::fmt::Debug for MultiplexInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MultiplexInner")
            .field("addr", &self.connection_factory.addr)
            .field("connections", &self.slots.len())
            .field("closed", &self.closed.load(Ordering::SeqCst))
            .finish()
    }
}

impl MultiplexPool {
    /// Open `connections` connections to `addr` with [`ConnectionConfig::default`].
    ///
    /// # Errors
    ///
    /// Returns [`RpcError::InvalidPoolConfiguration`] when `connections` is
    /// zero and [`RpcError::ConnectFailed`] when a connection cannot be opened.
    pub async fn connect(addr: &SocketAddr, connections: usize) -> Result<Self, RpcError> {
        Self::connect_with_config(addr, connections, ConnectionConfig::default()).await
    }

    /// Open `connections` connections to `addr` using `config`, which also
    /// applies to replacement connections.
    ///
    /// # Errors
    ///
    /// Returns [`RpcError::InvalidPoolConfiguration`] when `connections` is
    /// zero and [`RpcError::ConnectFailed`] when a connection cannot be opened.
    pub async fn connect_with_config(
        addr: &SocketAddr,
        connections: usize,
        config: ConnectionConfig,
    ) -> Result<Self, RpcError> {
        Self::connect_with_factory(connections, ConnectionFactory::new(*addr, config)).await
    }

    #[cfg(test)]
    async fn connect_with<F, Fut>(
        addr: &SocketAddr,
        connections: usize,
        connect: F,
    ) -> Result<Self, RpcError>
    where
        F: FnMut() -> Fut + Send + 'static,
        Fut: Future<Output = Result<crate::connection::ClientConnection, std::io::Error>>
            + Send
            + 'static,
    {
        Self::connect_with_factory(
            connections,
            ConnectionFactory::with_override(*addr, connect),
        )
        .await
    }

    async fn connect_with_factory(
        connections: usize,
        connection_factory: ConnectionFactory,
    ) -> Result<Self, RpcError> {
        if connections == 0 {
            return Err(RpcError::InvalidPoolConfiguration);
        }

        // `pending` holds `ClientConnection`s, whose RAII drop poisons the
        // partial connections if construction is canceled mid-loop.
        let mut pending = Vec::with_capacity(connections);
        for _ in 0..connections {
            match connection_factory.connect().await {
                Ok(client) => pending.push(client),
                Err(e) => {
                    close_all(pending).await;
                    tracing::error!(
                        addr = %connection_factory.addr,
                        error = %e,
                        pool_type = "multiplex",
                        "pool connection failed"
                    );
                    return Err(RpcError::ConnectFailed);
                }
            }
        }

        let slots = pending
            .into_iter()
            .map(|client| MultiplexSlot {
                handle: std::sync::RwLock::new(client.into_handle()),
                in_flight: std::sync::atomic::AtomicUsize::new(0),
                replacing: AtomicBool::new(false),
            })
            .collect();

        Ok(Self {
            inner: Arc::new(MultiplexInner {
                slots,
                cursor: std::sync::atomic::AtomicUsize::new(0),
                connection_factory,
                closed: AtomicBool::new(false),
                closed_notify: Notify::new(),
                replacements: StdMutex::new(Vec::new()),
            }),
        })
    }

    /// Returns the configured connection count.
    pub fn size(&self) -> usize {
        self.inner.slots.len()
    }

    /// Returns the number of calls in flight across all connections.
    pub fn pending_count(&self) -> usize {
        self.inner
            .slots
            .iter()
            .map(|slot| slot.in_flight.load(Ordering::Relaxed))
            .sum()
    }

    /// Shut down the pool and wait for its connections to stop.
    ///
    /// New calls fail with [`RpcError::PoolClosed`]. A replacement in
    /// progress is stopped, and one that completes concurrently is closed
    /// before this returns. Calling `shutdown` more than once is safe.
    pub async fn shutdown(&self) {
        let (replacements, connection_shutdowns) = self.inner.begin_shutdown();
        finish_shutdowns(connection_shutdowns).await;
        for join in replacements {
            let _ = join.await;
        }
    }
}

impl MultiplexInner {
    fn begin_shutdown(
        &self,
    ) -> (
        Vec<JoinHandle<()>>,
        Vec<crate::connection::ConnectionShutdown>,
    ) {
        if !begin_pool_shutdown(&self.closed) {
            return (Vec::new(), Vec::new());
        }
        self.closed_notify.notify_waiters();
        // Replacement tasks install only under the slot's write lock after
        // re-checking `closed`, so every connection installed from here on
        // is one this snapshot already covers, and a task that loses that
        // race closes its own connection before it exits.
        let connection_shutdowns = self
            .slots
            .iter()
            .map(|slot| slot.handle.read().unwrap().begin_poison())
            .collect();
        let replacements = std::mem::take(&mut *self.replacements.lock().unwrap());
        (replacements, connection_shutdowns)
    }

    /// Run `future` unless the pool closes first.
    async fn until_closed<F: Future>(&self, future: F) -> Option<F::Output> {
        let notified = self.closed_notify.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if self.closed.load(Ordering::SeqCst) {
            return None;
        }
        tokio::select! {
            biased;
            _ = &mut notified => None,
            output = future => Some(output),
        }
    }

    /// Reserve a call on the slot with the fewest calls in flight, skipping
    /// connections known to be down while a healthy one exists.
    ///
    /// The reservation is a compare-and-swap against the count the scan
    /// observed, so concurrent callers that saw the same least-loaded slot
    /// serialize on it: the loser rescans and lands on the next one. A slot
    /// whose connection the peer closed is noticed here, handed to a
    /// background replacement, and skipped like any other replacing slot.
    fn lease(self: &Arc<Self>) -> Result<Lease<'_>, RpcError> {
        let n = self.slots.len();
        loop {
            if self.closed.load(Ordering::SeqCst) {
                return Err(RpcError::PoolClosed);
            }
            let start = self.cursor.fetch_add(1, Ordering::Relaxed) % n;
            let mut best: Option<(usize, usize)> = None;
            let mut fallback: Option<usize> = None;
            for offset in 0..n {
                let index = (start + offset) % n;
                let slot = &self.slots[index];
                if slot.replacing.load(Ordering::Acquire) {
                    fallback.get_or_insert(index);
                    continue;
                }
                let in_flight = slot.in_flight.load(Ordering::Relaxed);
                if best.is_none_or(|(_, least)| in_flight < least) {
                    best = Some((index, in_flight));
                    if in_flight == 0 {
                        break;
                    }
                }
            }
            let index = match best {
                Some((index, observed)) => {
                    if self.slots[index]
                        .in_flight
                        .compare_exchange(
                            observed,
                            observed + 1,
                            Ordering::AcqRel,
                            Ordering::Relaxed,
                        )
                        .is_err()
                    {
                        // Another caller took this slot first; rescan.
                        continue;
                    }
                    index
                }
                None => {
                    // Every connection is being replaced. Send on the first
                    // one anyway so the call fails fast with SystemError
                    // rather than waiting for a replacement.
                    let index = fallback.expect("bug: pool has at least one slot");
                    self.slots[index].in_flight.fetch_add(1, Ordering::Relaxed);
                    index
                }
            };
            let slot = &self.slots[index];
            let handle = slot.handle.read().unwrap().clone();
            let lease = Lease {
                inner: self,
                index,
                handle,
            };
            if best.is_some() && lease.handle.is_poisoned() {
                // The peer closed this connection while it was idle. Start
                // its replacement and route this call elsewhere.
                self.replace(index, &lease.handle);
                drop(lease);
                continue;
            }
            return Ok(lease);
        }
    }

    /// Replace the connection in `slot` once, in the background, if `handle`
    /// is still the one installed there.
    fn replace(self: &Arc<Self>, index: usize, handle: &Arc<ConnectionHandle>) {
        let slot = &self.slots[index];
        if !Arc::ptr_eq(&*slot.handle.read().unwrap(), handle)
            || slot
                .replacing
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return;
        }
        if self.closed.load(Ordering::SeqCst) {
            slot.replacing.store(false, Ordering::Release);
            return;
        }
        let inner = self.clone();
        let old = handle.clone();
        let task = tokio::spawn(async move {
            old.begin_poison().finish().await;
            let mut retry_delay = Duration::from_millis(100);
            let mut first_failure = true;
            loop {
                let Some(result) = inner.until_closed(inner.connection_factory.connect()).await
                else {
                    return;
                };
                match result {
                    Ok(connection) => {
                        let slot = &inner.slots[index];
                        let mut installed = slot.handle.write().unwrap();
                        if inner.closed.load(Ordering::SeqCst) {
                            // Shutdown already snapshotted this slot's old
                            // handle; this connection is ours to close.
                            drop(installed);
                            connection.into_handle().poison().await;
                            return;
                        }
                        let previous = std::mem::replace(&mut *installed, connection.into_handle());
                        drop(installed);
                        slot.replacing.store(false, Ordering::Release);
                        drop(previous);
                        return;
                    }
                    Err(error) => {
                        if first_failure {
                            tracing::warn!(
                                addr = %inner.connection_factory.addr,
                                %error,
                                pool_type = "multiplex",
                                "failed to replace pool connection"
                            );
                            first_failure = false;
                        } else {
                            tracing::debug!(
                                addr = %inner.connection_factory.addr,
                                %error,
                                pool_type = "multiplex",
                                "pool connection replacement still pending"
                            );
                        }
                        if inner
                            .until_closed(tokio::time::sleep(retry_delay))
                            .await
                            .is_none()
                        {
                            return;
                        }
                        retry_delay = retry_delay.saturating_mul(2).min(Duration::from_secs(5));
                    }
                }
            }
        });
        let mut replacements = self.replacements.lock().unwrap();
        replacements.retain(|join| !join.is_finished());
        replacements.push(task);
    }
}

/// A call's claim on one pool connection; releases the in-flight count on
/// drop, including when the caller is canceled.
struct Lease<'a> {
    inner: &'a MultiplexInner,
    index: usize,
    handle: Arc<ConnectionHandle>,
}

impl Drop for Lease<'_> {
    fn drop(&mut self) {
        self.inner.slots[self.index]
            .in_flight
            .fetch_sub(1, Ordering::Relaxed);
    }
}

impl Drop for MultiplexPool {
    fn drop(&mut self) {
        let _ = self.inner.begin_shutdown();
    }
}

#[async_trait]
impl RpcSender for MultiplexPool {
    fn call_buf(&self) -> CallBuf {
        CallBuf::from_spare(crate::connection::take_buffer())
    }

    /// Send an RPC call on the connection with the fewest calls in flight.
    async fn send_call(
        &self,
        progvers: ProgVers,
        proc: u32,
        call: CallBuf,
    ) -> Result<RpcResponse, RpcError> {
        let lease = self.inner.lease()?;
        let result = lease.handle.send_call(progvers, proc, call).await;
        if result.is_err() && lease.handle.is_poisoned() {
            self.inner.replace(lease.index, &lease.handle);
        }
        result
    }

    /// Forwarded to the leased connection, which decodes on its reader.
    async unsafe fn send_call_decoded(
        &self,
        progvers: ProgVers,
        proc: u32,
        call: CallBuf,
        decode: &mut (dyn for<'reply> FnMut(&'reply [u8]) -> Result<(), RpcError> + Send),
    ) -> Result<(), RpcError> {
        let lease = self.inner.lease()?;
        // SAFETY: the caller's contract, forwarded; the call runs in this future.
        let result = unsafe {
            lease
                .handle
                .send_call_decoded(progvers, proc, call, decode)
                .await
        };
        if result.is_err() && lease.handle.is_poisoned() {
            self.inner.replace(lease.index, &lease.handle);
        }
        result
    }

    async fn send_fire_and_forget(
        &self,
        progvers: ProgVers,
        proc: u32,
        call: CallBuf,
    ) -> Result<(), RpcError> {
        let lease = self.inner.lease()?;
        let result = lease
            .handle
            .send_fire_and_forget(progvers, proc, call)
            .await;
        if result.is_err() && lease.handle.is_poisoned() {
            self.inner.replace(lease.index, &lease.handle);
        }
        result
    }
}

// ---------------------------------------------------------------------------
// CpuSetPool — LLC-topology-aware connection pool
// ---------------------------------------------------------------------------

/// LLC-topology-aware connection pool.
///
/// Creates one connection per Last-Level Cache (L3) group, then routes each
/// RPC to the connection matching the caller's current CPU. On Linux, only
/// CPUs in the calling thread's allowed affinity mask participate in discovery.
/// Routing alone does not pin the connection's reader/writer tasks or the
/// caller's continuation; runtime scheduling must also preserve locality.
///
/// Mirrors the x23 `RpcMulticlient` / `PerCpuSet` pattern.
///
/// Falls back to `cpu_id % num_connections` if LLC topology cannot be read
/// (non-Linux, container restrictions, etc.).
///
/// # Example
///
/// ```no_run
/// # async fn example() -> Result<(), thruput_rpc_driver::RpcError> {
/// use thruput_rpc_driver::{CpuSetPool, ProgVers, RpcSender};
///
/// let pool = CpuSetPool::connect(&"127.0.0.1:9876".parse().unwrap()).await?;
/// println!("opened {} CPU-affine connections", pool.size());
/// let mut call = pool.call_buf();
/// call.extend_from_slice(b"hello");
/// let resp = pool.send_call(ProgVers::new(100, 1), 0, call).await?;
/// # Ok(())
/// # }
/// ```
pub struct CpuSetPool {
    /// One connection per LLC bucket.
    connections: StdMutex<Vec<Arc<ConnectionHandle>>>,
    /// Maps logical CPU id → index into `connections`.
    cpu_map: Vec<usize>,
    closed: AtomicBool,
}

impl std::fmt::Debug for CpuSetPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CpuSetPool")
            .field("connections", &self.connections.lock().unwrap().len())
            .field("cpus_mapped", &self.cpu_map.len())
            .finish()
    }
}

impl CpuSetPool {
    /// Connect to `addr` with one connection per LLC group, auto-detected
    /// from sysfs. Falls back to a single connection if topology is
    /// unavailable (non-Linux, container restrictions). Connections use
    /// [`ConnectionConfig::default`].
    ///
    /// # Errors
    ///
    /// Returns [`RpcError::ConnectFailed`] when a connection cannot be opened.
    pub async fn connect(addr: &SocketAddr) -> Result<Self, RpcError> {
        Self::connect_with_config(addr, ConnectionConfig::default()).await
    }

    /// Connect using `config`, with one connection per auto-detected LLC group.
    ///
    /// Falls back to a single connection if topology is unavailable
    /// (non-Linux, container restrictions).
    ///
    /// # Errors
    ///
    /// Returns [`RpcError::ConnectFailed`] when a connection cannot be opened.
    pub async fn connect_with_config(
        addr: &SocketAddr,
        config: ConnectionConfig,
    ) -> Result<Self, RpcError> {
        let (num_buckets, cpu_map) = detect_llc_topology();
        Self::connect_with_map_and_config(addr, num_buckets, cpu_map, config).await
    }

    /// Connect with an explicit number of connections and CPU→connection mapping.
    ///
    /// Useful for testing or when you want to override the auto-detected topology
    /// (e.g., one connection per physical core instead of per LLC). Connections
    /// use [`ConnectionConfig::default`].
    ///
    /// # Errors
    ///
    /// Returns [`RpcError::InvalidPoolConfiguration`] when the connection count
    /// or CPU map is invalid and [`RpcError::ConnectFailed`] when a connection
    /// cannot be opened.
    pub async fn connect_with_map(
        addr: &SocketAddr,
        num_connections: usize,
        cpu_map: Vec<usize>,
    ) -> Result<Self, RpcError> {
        Self::connect_with_map_and_config(
            addr,
            num_connections,
            cpu_map,
            ConnectionConfig::default(),
        )
        .await
    }

    /// Connect using `config` with an explicit connection count and CPU map.
    ///
    /// `cpu_map` maps logical CPU identifiers to connection indices. Entries
    /// may be [`usize::MAX`] to request modular fallback routing.
    ///
    /// # Errors
    ///
    /// Returns [`RpcError::InvalidPoolConfiguration`] when the connection count
    /// or CPU map is invalid and [`RpcError::ConnectFailed`] when a connection
    /// cannot be opened.
    pub async fn connect_with_map_and_config(
        addr: &SocketAddr,
        num_connections: usize,
        cpu_map: Vec<usize>,
        config: ConnectionConfig,
    ) -> Result<Self, RpcError> {
        Self::connect_with_factory(
            num_connections,
            cpu_map,
            ConnectionFactory::new(*addr, config),
        )
        .await
    }

    #[cfg(test)]
    async fn connect_with<F, Fut>(
        addr: &SocketAddr,
        num_connections: usize,
        cpu_map: Vec<usize>,
        connect: F,
    ) -> Result<Self, RpcError>
    where
        F: FnMut() -> Fut + Send + 'static,
        Fut: Future<Output = Result<crate::connection::ClientConnection, std::io::Error>>
            + Send
            + 'static,
    {
        Self::connect_with_factory(
            num_connections,
            cpu_map,
            ConnectionFactory::with_override(*addr, connect),
        )
        .await
    }

    async fn connect_with_factory(
        num_connections: usize,
        cpu_map: Vec<usize>,
        connection_factory: ConnectionFactory,
    ) -> Result<Self, RpcError> {
        if num_connections == 0 {
            return Err(RpcError::InvalidPoolConfiguration);
        }
        if cpu_map
            .iter()
            .any(|&b| b != usize::MAX && b >= num_connections)
        {
            return Err(RpcError::InvalidPoolConfiguration);
        }

        // `pending` holds `ClientConnection`s, whose RAII drop poisons the
        // partial connections if construction is canceled mid-loop.
        let mut pending = Vec::with_capacity(num_connections);
        for _ in 0..num_connections {
            match connection_factory.connect().await {
                Ok(client) => pending.push(client),
                Err(e) => {
                    close_all(pending).await;
                    tracing::error!(
                        addr = %connection_factory.addr,
                        error = %e,
                        pool_type = "cpu_set",
                        "pool connection failed"
                    );
                    return Err(RpcError::ConnectFailed);
                }
            }
        }

        Ok(Self {
            connections: StdMutex::new(pending.into_iter().map(|c| c.into_handle()).collect()),
            cpu_map,
            closed: AtomicBool::new(false),
        })
    }

    /// Returns the current number of installed connections.
    ///
    /// This is normally one per detected LLC bucket, or the explicit count
    /// supplied to [`CpuSetPool::connect_with_map`]. It is zero after shutdown.
    pub fn size(&self) -> usize {
        self.connections.lock().unwrap().len()
    }

    /// Get the connection for the current CPU.
    #[inline]
    fn connection_for_current_cpu(&self) -> Result<Arc<ConnectionHandle>, RpcError> {
        let connections = self.connections.lock().unwrap();
        if connections.is_empty() {
            return Err(RpcError::PoolClosed);
        }

        let cpu_id = current_cpu() as usize;
        let bucket = if cpu_id < self.cpu_map.len() {
            let b = self.cpu_map[cpu_id];
            if b < connections.len() {
                b
            } else {
                // Unmapped CPU (offline/no L3) — modular fallback
                cpu_id % connections.len()
            }
        } else {
            // CPU id out of range (hot-plugged?) — modular fallback
            cpu_id % connections.len()
        };
        Ok(connections[bucket].clone())
    }

    fn connection_for_request(&self) -> Result<Arc<ConnectionHandle>, RpcError> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(RpcError::PoolClosed);
        }
        let conn = self.connection_for_current_cpu()?;
        if self.closed.load(Ordering::SeqCst) {
            return Err(RpcError::PoolClosed);
        }
        Ok(conn)
    }

    fn begin_shutdown(&self) -> Vec<crate::connection::ConnectionShutdown> {
        if !begin_pool_shutdown(&self.closed) {
            return Vec::new();
        }

        let mut connections = self.connections.lock().unwrap();
        connections
            .drain(..)
            .map(|conn| conn.begin_poison())
            .collect()
    }

    /// Shut down the pool and wait for all connections to stop.
    ///
    /// New calls fail with [`RpcError::PoolClosed`]. Calling `shutdown` more
    /// than once is safe.
    pub async fn shutdown(&self) {
        finish_shutdowns(self.begin_shutdown()).await;
    }
}

impl Drop for CpuSetPool {
    fn drop(&mut self) {
        let _ = self.begin_shutdown();
    }
}

#[async_trait]
impl RpcSender for CpuSetPool {
    fn call_buf(&self) -> CallBuf {
        CallBuf::from_spare(crate::connection::take_buffer())
    }

    async fn send_call(
        &self,
        progvers: ProgVers,
        proc: u32,
        call: CallBuf,
    ) -> Result<RpcResponse, RpcError> {
        let conn = self.connection_for_request()?;
        conn.send_call(progvers, proc, call).await
    }

    /// Forwarded to the chosen connection, which decodes on its reader.
    async unsafe fn send_call_decoded(
        &self,
        progvers: ProgVers,
        proc: u32,
        call: CallBuf,
        decode: &mut (dyn for<'reply> FnMut(&'reply [u8]) -> Result<(), RpcError> + Send),
    ) -> Result<(), RpcError> {
        let conn = self.connection_for_request()?;
        // SAFETY: the caller's contract, forwarded; the call runs in this future.
        unsafe { conn.send_call_decoded(progvers, proc, call, decode).await }
    }

    async fn send_fire_and_forget(
        &self,
        progvers: ProgVers,
        proc: u32,
        call: CallBuf,
    ) -> Result<(), RpcError> {
        let conn = self.connection_for_request()?;
        conn.send_fire_and_forget(progvers, proc, call).await
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
    let num_cpus = std::thread::available_parallelism().map_or(1, |n| n.get());
    let cpu_map = vec![0; num_cpus];
    (1, cpu_map)
}

/// Linux-specific LLC topology detection from sysfs.
#[cfg(target_os = "linux")]
fn detect_llc_topology_linux() -> Option<(usize, Vec<usize>)> {
    use std::fs;

    // available_parallelism() is a count, not a list of logical CPU IDs.
    // In a cpuset such as 72-79, inspecting cpu0..cpu7 would choose unrelated
    // LLCs and leave every CPU the caller can actually use unmapped.
    let mut affinity = unsafe { std::mem::zeroed::<libc::cpu_set_t>() };
    // SAFETY: affinity is writable, correctly aligned, and its size is exact.
    if unsafe { libc::sched_getaffinity(0, std::mem::size_of_val(&affinity), &mut affinity) } != 0 {
        // Includes kernels whose CPU mask exceeds cpu_set_t. Preserve the
        // conservative single-connection fallback instead of guessing IDs.
        return None;
    }
    let cpus: Vec<usize> = (0..libc::CPU_SETSIZE as usize)
        .filter(|&cpu| {
            // SAFETY: cpu is below CPU_SETSIZE and affinity is initialized.
            unsafe { libc::CPU_ISSET(cpu, &affinity) }
        })
        .collect();
    llc_topology_for_cpus(&cpus, |cpu| {
        fs::read_to_string(format!(
            "/sys/devices/system/cpu/cpu{cpu}/cache/index3/shared_cpu_list"
        ))
        .ok()
        .map(|s| s.trim().to_string())
    })
}

#[cfg(target_os = "linux")]
fn llc_topology_for_cpus(
    cpus: &[usize],
    mut read_group: impl FnMut(usize) -> Option<String>,
) -> Option<(usize, Vec<usize>)> {
    let mut llc_groups = std::collections::BTreeMap::new();
    let mut cpu_map = vec![usize::MAX; cpus.iter().max()? + 1];
    for &cpu in cpus {
        let Some(group) = read_group(cpu).filter(|s| !s.is_empty()) else {
            continue;
        };
        let next = llc_groups.len();
        cpu_map[cpu] = *llc_groups.entry(group).or_insert(next);
    }
    if llc_groups.is_empty() {
        return None;
    }
    tracing::info!(
        groups = llc_groups.len(),
        cpus = cpus.len(),
        "detected LLC groups"
    );
    Some((llc_groups.len(), cpu_map))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn llc_topology_uses_allowed_cpu_ids_instead_of_cpu_count() {
        let allowed = [8, 9, 72, 73];
        let mut inspected = Vec::new();
        let (count, map) = llc_topology_for_cpus(&allowed, |cpu| {
            inspected.push(cpu);
            Some(if cpu < 10 { "8-15" } else { "72-79" }.to_string())
        })
        .unwrap();
        assert_eq!(inspected, allowed);
        assert_eq!(count, 2);
        assert_eq!(map.len(), 74);
        assert_eq!(map[8], map[9]);
        assert_eq!(map[72], map[73]);
        assert_ne!(map[8], map[72]);
        assert_eq!(map[0], usize::MAX);
        assert_eq!(map[71], usize::MAX);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn llc_topology_handles_restricted_and_unreadable_groups() {
        let (count, map) = llc_topology_for_cpus(&[72, 73], |_| Some("72-79".into())).unwrap();
        assert_eq!(count, 1);
        assert_eq!(map[72], 0);
        assert_eq!(map[73], 0);
        assert!(llc_topology_for_cpus(&[], |_| Some("0-7".into())).is_none());
        assert!(llc_topology_for_cpus(&[72], |_| None).is_none());
        let (count, map) =
            llc_topology_for_cpus(&[8, 72], |cpu| (cpu == 72).then(|| "72-79".into())).unwrap();
        assert_eq!(count, 1);
        assert_eq!(map[8], usize::MAX);
        assert_eq!(map[72], 0);
    }

    use crate::RpcServer;
    use crate::handler::{RpcHandler, ServiceMap};
    use async_trait::async_trait;
    use std::sync::atomic::AtomicUsize;
    use tokio::io::AsyncReadExt;
    use tokio::net::TcpListener;

    fn call_with(payload: &[u8]) -> CallBuf {
        let mut call = CallBuf::new();
        call.extend_from_slice(payload);
        call
    }

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

    struct CancellationEchoService {
        blocked_started: Arc<Notify>,
        blocked_release: Arc<Notify>,
        calls: Arc<StdMutex<Vec<Vec<u8>>>>,
    }

    struct HoldingEchoService {
        started: tokio::sync::mpsc::UnboundedSender<()>,
        release: Arc<Semaphore>,
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

    #[async_trait]
    impl RpcHandler for CancellationEchoService {
        async fn dispatch(&self, proc: u32, data: &[u8]) -> Result<Vec<u8>, RpcError> {
            if proc != 1 {
                return Err(RpcError::InvalidProcedure);
            }
            self.calls.lock().unwrap().push(data.to_vec());
            if data == b"blocked" {
                self.blocked_started.notify_one();
                self.blocked_release.notified().await;
                Ok(data.to_vec())
            } else {
                Ok(data.to_vec())
            }
        }

        fn prog_vers(&self) -> ProgVers {
            ProgVers::new(1, 1)
        }
    }

    #[async_trait]
    impl RpcHandler for HoldingEchoService {
        async fn dispatch(&self, proc: u32, data: &[u8]) -> Result<Vec<u8>, RpcError> {
            if proc != 1 {
                return Err(RpcError::InvalidProcedure);
            }
            self.started
                .send(())
                .expect("balancing test should still be waiting for calls");
            self.release
                .acquire()
                .await
                .expect("balancing test release semaphore should remain open")
                .forget();
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

    /// Start a server that reports each call it receives on the returned
    /// channel and holds the reply until a permit is added to the returned
    /// semaphore.
    ///
    /// Tests can wait for the server to observe each call before starting
    /// another, making the pool's pending-count-based routing deterministic.
    async fn start_holding_server() -> (
        SocketAddr,
        tokio::sync::mpsc::UnboundedReceiver<()>,
        Arc<Semaphore>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (started_tx, started_rx) = tokio::sync::mpsc::unbounded_channel();
        let release = Arc::new(Semaphore::new(0));
        let mut services = ServiceMap::new();
        services.add_service(Box::new(HoldingEchoService {
            started: started_tx,
            release: release.clone(),
        }));
        let server = RpcServer::new(listener, services);
        server.start().await;
        (addr, started_rx, release)
    }

    /// Wait until the holding server has observed one more call.
    ///
    /// The deadline only bounds a genuine hang; it is generous so that a
    /// loaded machine cannot turn scheduling delay into a failure.
    async fn await_call_started(started_rx: &mut tokio::sync::mpsc::UnboundedReceiver<()>) {
        tokio::time::timeout(Duration::from_secs(30), started_rx.recv())
            .await
            .expect("server should observe the pending call")
            .expect("holding service should remain connected");
    }

    async fn start_cancellation_server() -> (
        SocketAddr,
        Arc<Notify>,
        Arc<Notify>,
        Arc<StdMutex<Vec<Vec<u8>>>>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let blocked_started = Arc::new(Notify::new());
        let blocked_release = Arc::new(Notify::new());
        let calls = Arc::new(StdMutex::new(Vec::new()));
        let mut services = ServiceMap::new();
        services.add_service(Box::new(CancellationEchoService {
            blocked_started: blocked_started.clone(),
            blocked_release: blocked_release.clone(),
            calls: calls.clone(),
        }));
        let server = RpcServer::new(listener, services);
        server.start().await;
        (addr, blocked_started, blocked_release, calls)
    }

    /// Accept and close the pool's initial connection, then serve echo
    /// requests on replacement connections.
    async fn start_drop_then_echo_server() -> (SocketAddr, Arc<Notify>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let initial_connection_dropped = Arc::new(Notify::new());
        let dropped = initial_connection_dropped.clone();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            drop(stream);
            dropped.notify_one();

            let mut services = ServiceMap::new();
            services.add_service(Box::new(EchoService));
            let server = RpcServer::new(listener, services);
            drop(server.start().await);
        });
        (addr, initial_connection_dropped)
    }

    fn limited_client_config(max_message_size: u32) -> ConnectionConfig {
        ConnectionConfig::new(Some(max_message_size), ServiceMap::new())
    }

    /// Spawn a raw TCP server that accepts one connection and reports the
    /// result of its first read (`Ok(0)` = clean EOF, i.e. the peer closed).
    /// The returned sender releases the server task once the test is done
    /// observing.
    async fn start_eof_probe_server() -> (
        SocketAddr,
        oneshot::Receiver<std::io::Result<usize>>,
        oneshot::Sender<()>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (closed_tx, closed_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut byte = [0u8; 1];
            closed_tx.send(stream.read(&mut byte).await).unwrap();
            release_rx.await.unwrap();
        });
        (addr, closed_rx, release_tx, server)
    }

    #[tokio::test]
    async fn fixed_pool_distributes_calls() {
        let addr = start_echo_server().await;

        let pool = FixedSerialPool::connect(&addr, 3).await.unwrap();
        assert_eq!(pool.size(), 3);

        // Send 9 calls sequentially and verify all succeed with correct data.
        for i in 0..9u32 {
            let data = format!("call-{i}");
            let result = pool
                .send_call(ProgVers::new(1, 1), 1, call_with(data.as_bytes()))
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
                let data = format!("slow-{i}");
                pool.send_call(ProgVers::new(1, 1), 1, call_with(data.as_bytes()))
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
            "Expected parallel execution (~100ms), got {elapsed:?}"
        );
    }

    #[tokio::test]
    async fn fixed_pool_error_propagation() {
        let addr = start_echo_server().await;

        let pool = FixedSerialPool::connect(&addr, 2).await.unwrap();

        // Call an invalid procedure — error should propagate through the pool.
        let result = pool
            .send_call(ProgVers::new(1, 1), 99, call_with(b"bad"))
            .await;
        assert_eq!(result, Err(RpcError::InvalidProcedure));

        // Pool should still work for valid calls after an error.
        let result = pool
            .send_call(ProgVers::new(1, 1), 1, call_with(b"ok"))
            .await
            .unwrap();
        assert_eq!(result, b"ok");
    }

    #[tokio::test]
    async fn fixed_pool_replaces_connection_after_peer_disconnect() {
        let (addr, initial_connection_dropped) = start_drop_then_echo_server().await;
        let pool = FixedSerialPool::connect(&addr, 1).await.unwrap();
        initial_connection_dropped.notified().await;

        let response = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                match pool
                    .send_call(ProgVers::new(1, 1), 1, call_with(b"after-restart"))
                    .await
                {
                    Ok(response) => break response,
                    Err(RpcError::SystemError) => tokio::task::yield_now().await,
                    Err(error) => panic!("unexpected error while replacing connection: {error}"),
                }
            }
        })
        .await
        .expect("fixed pool should replace a peer-disconnected connection");

        assert_eq!(response, b"after-restart");
    }

    #[tokio::test]
    async fn fixed_pool_poison_policy_terminates_after_peer_disconnect() {
        let (addr, initial_connection_dropped) = start_drop_then_echo_server().await;
        let pool = FixedSerialPool::connect_with_canceled_call_policy(
            &addr,
            1,
            CanceledCallPolicy::PoisonConnection,
        )
        .await
        .unwrap();
        initial_connection_dropped.notified().await;

        let result = tokio::time::timeout(
            Duration::from_secs(1),
            pool.send_call(ProgVers::new(1, 1), 1, call_with(b"after-disconnect")),
        )
        .await
        .expect("peer disconnect should resolve the active request");
        assert!(matches!(
            result,
            Err(RpcError::SystemError | RpcError::Canceled)
        ));

        tokio::time::timeout(Duration::from_secs(1), async {
            while !pool.inner.worker_handles.lock().unwrap()[0].is_finished() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("poison policy should terminate the disconnected worker");

        assert_eq!(
            pool.send_call(ProgVers::new(1, 1), 1, call_with(b"after-worker-exit"))
                .await,
            Err(RpcError::PoolClosed)
        );
    }

    #[tokio::test]
    async fn fixed_pool_zero_size_rejected() {
        let addr = start_echo_server().await;
        let result = FixedSerialPool::connect(&addr, 0).await;
        assert_eq!(result.unwrap_err(), RpcError::InvalidPoolConfiguration);
    }

    #[tokio::test]
    async fn fixed_pool_single_connection() {
        let addr = start_echo_server().await;

        let pool = FixedSerialPool::connect(&addr, 1).await.unwrap();
        assert_eq!(pool.size(), 1);

        // Even with one connection, sequential calls should work.
        for i in 0..5u32 {
            let data = format!("single-{i}");
            let result = pool
                .send_call(ProgVers::new(1, 1), 1, call_with(data.as_bytes()))
                .await
                .unwrap();
            assert_eq!(result, data.as_bytes());
        }
    }

    #[tokio::test]
    async fn fixed_pool_applies_connection_config() {
        let addr = start_echo_server().await;
        let pool = FixedSerialPool::connect_with_config(&addr, 1, limited_client_config(128))
            .await
            .unwrap();

        let oversized_response = vec![0; 256];
        let result = tokio::time::timeout(
            Duration::from_secs(1),
            pool.send_call(ProgVers::new(1, 1), 1, call_with(&oversized_response)),
        )
        .await
        .expect("configured message limit should reject the response promptly");

        assert_eq!(result, Err(RpcError::SystemError));
    }

    #[tokio::test]
    async fn fixed_pool_canceled_active_call_releases_worker() {
        let (addr, blocked_started, blocked_release, calls) = start_cancellation_server().await;
        let pool = Arc::new(FixedSerialPool::connect(&addr, 1).await.unwrap());
        let blocked_pool = pool.clone();
        let blocked = tokio::spawn(async move {
            blocked_pool
                .send_call(ProgVers::new(1, 1), 1, call_with(b"blocked"))
                .await
        });
        blocked_started.notified().await;

        blocked.abort();
        assert!(blocked.await.unwrap_err().is_cancelled());
        let response = tokio::time::timeout(
            Duration::from_secs(1),
            pool.send_call(ProgVers::new(1, 1), 1, call_with(b"after")),
        )
        .await
        .expect("a canceled active call should release its worker")
        .unwrap();

        assert_eq!(response, b"after");
        assert_eq!(
            &*calls.lock().unwrap(),
            &[b"blocked".to_vec(), b"after".to_vec()]
        );
        blocked_release.notify_one();
    }

    #[tokio::test]
    async fn fixed_pool_replacement_applies_connection_config() {
        let (addr, blocked_started, blocked_release, calls) = start_cancellation_server().await;
        let pool = Arc::new(
            FixedSerialPool::connect_with_config_and_canceled_call_policy(
                &addr,
                1,
                limited_client_config(128),
                CanceledCallPolicy::ReplaceConnection,
            )
            .await
            .unwrap(),
        );
        let blocked_pool = pool.clone();
        let blocked = tokio::spawn(async move {
            blocked_pool
                .send_call(ProgVers::new(1, 1), 1, call_with(b"blocked"))
                .await
        });
        blocked_started.notified().await;

        blocked.abort();
        assert!(blocked.await.unwrap_err().is_cancelled());

        let oversized_response = vec![0; 256];
        let result = tokio::time::timeout(
            Duration::from_secs(1),
            pool.send_call(ProgVers::new(1, 1), 1, call_with(&oversized_response)),
        )
        .await
        .expect("replacement connection should retain the configured message limit");

        assert_eq!(result, Err(RpcError::SystemError));
        assert_eq!(&*calls.lock().unwrap(), &[b"blocked".to_vec()]);
        blocked_release.notify_one();
    }

    #[tokio::test]
    async fn fixed_pool_skips_canceled_queued_call() {
        let (addr, blocked_started, blocked_release, calls) = start_cancellation_server().await;
        let pool = Arc::new(FixedSerialPool::connect(&addr, 1).await.unwrap());
        let blocked_pool = pool.clone();
        let blocked = tokio::spawn(async move {
            blocked_pool
                .send_call(ProgVers::new(1, 1), 1, call_with(b"blocked"))
                .await
        });
        blocked_started.notified().await;

        let stale_pool = pool.clone();
        let stale = tokio::spawn(async move {
            stale_pool
                .send_call(ProgVers::new(1, 1), 1, call_with(b"stale"))
                .await
        });
        tokio::time::timeout(Duration::from_secs(1), async {
            while pool.inner.queue_slots.available_permits() == 16 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("stale call should enter the pool queue");
        stale.abort();
        assert!(stale.await.unwrap_err().is_cancelled());
        blocked.abort();
        assert!(blocked.await.unwrap_err().is_cancelled());

        let response = tokio::time::timeout(
            Duration::from_secs(1),
            pool.send_call(ProgVers::new(1, 1), 1, call_with(b"after")),
        )
        .await
        .expect("canceled queued work should not block a later call")
        .unwrap();
        assert_eq!(response, b"after");
        assert_eq!(
            &*calls.lock().unwrap(),
            &[b"blocked".to_vec(), b"after".to_vec()]
        );
        blocked_release.notify_one();
    }

    #[tokio::test]
    async fn fixed_pool_can_fail_closed_after_active_cancellation() {
        let (addr, blocked_started, blocked_release, calls) = start_cancellation_server().await;
        let pool = Arc::new(
            FixedSerialPool::connect_with_canceled_call_policy(
                &addr,
                1,
                CanceledCallPolicy::PoisonConnection,
            )
            .await
            .unwrap(),
        );
        let blocked_pool = pool.clone();
        let blocked = tokio::spawn(async move {
            blocked_pool
                .send_call(ProgVers::new(1, 1), 1, call_with(b"blocked"))
                .await
        });
        blocked_started.notified().await;

        let queued_pool = pool.clone();
        let queued = tokio::spawn(async move {
            queued_pool
                .send_call(ProgVers::new(1, 1), 1, call_with(b"queued"))
                .await
        });
        tokio::time::timeout(Duration::from_secs(1), async {
            while pool.inner.queue_slots.available_permits() == 16 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("second call should enter the pool queue");

        blocked.abort();
        assert!(blocked.await.unwrap_err().is_cancelled());
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), queued)
                .await
                .expect("queued call should be abandoned when its worker terminates")
                .unwrap(),
            Err(RpcError::Canceled)
        );
        tokio::time::timeout(Duration::from_secs(1), async {
            while !pool.inner.worker_handles.lock().unwrap()[0].is_finished() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("poisoned worker should terminate");
        let worker = pool.inner.worker_handles.lock().unwrap().pop().unwrap();
        worker.await.expect("poisoned worker should exit cleanly");
        assert_eq!(
            pool.send_call(ProgVers::new(1, 1), 1, call_with(b"after"))
                .await,
            Err(RpcError::PoolClosed)
        );
        assert_eq!(&*calls.lock().unwrap(), &[b"blocked".to_vec()]);
        blocked_release.notify_one();
    }

    #[tokio::test]
    async fn fixed_pool_connect_failure() {
        // Try to connect to an address that's not listening.
        let addr: SocketAddr = "127.0.0.1:1".parse().unwrap();
        let result = FixedSerialPool::connect(&addr, 3).await;
        assert_eq!(result.unwrap_err(), RpcError::ConnectFailed);
    }

    #[tokio::test]
    async fn fixed_pool_connect_cancellation_closes_partial_connections() {
        let (addr, closed_rx, release_server_tx, server) = start_eof_probe_server().await;

        let attempts = Arc::new(AtomicUsize::new(0));
        let connect_attempts = attempts.clone();
        let (first_handle_tx, first_handle_rx) = oneshot::channel();
        let mut first_handle_tx = Some(first_handle_tx);
        let (second_attempt_tx, second_attempt_rx) = oneshot::channel();
        let mut second_attempt_tx = Some(second_attempt_tx);
        let construction = tokio::spawn(async move {
            FixedSerialPool::connect_with(
                &addr,
                2,
                CanceledCallPolicy::ReplaceConnection,
                move || {
                    let attempt = connect_attempts.fetch_add(1, Ordering::SeqCst);
                    let first_handle_tx = if attempt == 0 {
                        first_handle_tx.take()
                    } else {
                        None
                    };
                    let second_attempt_tx = if attempt == 1 {
                        second_attempt_tx.take()
                    } else {
                        None
                    };
                    async move {
                        if attempt == 0 {
                            let client = connect_client(&addr).await?;
                            first_handle_tx
                                .unwrap()
                                .send(Arc::downgrade(client.handle()))
                                .unwrap();
                            Ok(client)
                        } else {
                            second_attempt_tx.unwrap().send(()).unwrap();
                            std::future::pending().await
                        }
                    }
                },
            )
            .await
        });

        let first_handle = tokio::time::timeout(Duration::from_secs(1), first_handle_rx)
            .await
            .expect("first connection should complete")
            .unwrap();
        tokio::time::timeout(Duration::from_secs(1), second_attempt_rx)
            .await
            .expect("second connection attempt should start")
            .unwrap();
        construction.abort();
        assert!(
            tokio::time::timeout(Duration::from_secs(1), construction)
                .await
                .expect("pool construction should stop after cancellation")
                .unwrap_err()
                .is_cancelled()
        );

        let read_result = tokio::time::timeout(Duration::from_secs(1), closed_rx)
            .await
            .expect("partial connection should close when pool construction is canceled")
            .unwrap();
        assert_eq!(read_result.unwrap(), 0);
        tokio::time::timeout(Duration::from_secs(1), async {
            while first_handle.upgrade().is_some() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("partial connection handle should be released without peer cooperation");

        release_server_tx.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(1), server)
            .await
            .expect("server task should stop")
            .unwrap();
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn fixed_pool_connect_failure_closes_partial_connections() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut byte = [0u8; 1];
            stream.read(&mut byte).await.unwrap()
        });

        let attempts = Arc::new(AtomicUsize::new(0));
        let connect_attempts = attempts.clone();
        let (first_handle_tx, first_handle_rx) = oneshot::channel();
        let mut first_handle_tx = Some(first_handle_tx);
        let result = tokio::time::timeout(
            Duration::from_secs(1),
            FixedSerialPool::connect_with(
                &addr,
                2,
                CanceledCallPolicy::ReplaceConnection,
                move || {
                    let attempt = connect_attempts.fetch_add(1, Ordering::SeqCst);
                    let first_handle_tx = if attempt == 0 {
                        first_handle_tx.take()
                    } else {
                        None
                    };
                    async move {
                        if attempt == 0 {
                            let client = connect_client(&addr).await?;
                            first_handle_tx
                                .unwrap()
                                .send(Arc::downgrade(client.handle()))
                                .unwrap();
                            Ok(client)
                        } else {
                            Err(std::io::Error::other("injected connection failure"))
                        }
                    }
                },
            ),
        )
        .await
        .expect("pool construction should return after connection failure");

        assert_eq!(result.unwrap_err(), RpcError::ConnectFailed);
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        let first_handle = first_handle_rx.await.unwrap();
        assert!(first_handle.upgrade().is_none());
        let bytes_read = tokio::time::timeout(Duration::from_secs(1), server)
            .await
            .expect("partial connection should close after connection failure")
            .unwrap();
        assert_eq!(bytes_read, 0);
    }

    #[tokio::test]
    async fn fixed_pool_shutdown_rejects_new_calls() {
        let addr = start_echo_server().await;
        let pool = FixedSerialPool::connect(&addr, 2).await.unwrap();

        let ok = pool
            .send_call(ProgVers::new(1, 1), 1, call_with(b"before"))
            .await
            .unwrap();
        assert_eq!(&*ok, b"before");

        pool.shutdown().await;

        let err = pool
            .send_call(ProgVers::new(1, 1), 1, call_with(b"after"))
            .await
            .unwrap_err();
        assert_eq!(err, RpcError::PoolClosed);

        let err = pool
            .send_fire_and_forget(ProgVers::new(1, 1), 1, call_with(b"after-faf"))
            .await
            .unwrap_err();
        assert_eq!(err, RpcError::PoolClosed);
    }

    #[tokio::test]
    async fn dynamic_pool_shutdown_rejects_new_calls() {
        let addr = start_echo_server().await;
        let pool = DynamicPool::new(addr, 2, Duration::from_secs(60), None);

        let ok = pool
            .send_call(ProgVers::new(1, 1), 1, call_with(b"before"))
            .await
            .unwrap();
        assert_eq!(&*ok, b"before");

        pool.shutdown().await;

        let err = pool
            .send_call(ProgVers::new(1, 1), 1, call_with(b"after"))
            .await
            .unwrap_err();
        assert_eq!(err, RpcError::PoolClosed);

        let err = pool
            .send_fire_and_forget(ProgVers::new(1, 1), 1, call_with(b"after-faf"))
            .await
            .unwrap_err();
        assert_eq!(err, RpcError::PoolClosed);
    }

    #[tokio::test]
    async fn cpu_pool_shutdown_rejects_new_calls() {
        let addr = start_echo_server().await;
        let pool = CpuSetPool::connect_with_map(&addr, 2, vec![0, 1])
            .await
            .unwrap();

        let ok = pool
            .send_call(ProgVers::new(1, 1), 1, call_with(b"before"))
            .await
            .unwrap();
        assert_eq!(&*ok, b"before");

        pool.shutdown().await;

        let err = pool
            .send_call(ProgVers::new(1, 1), 1, call_with(b"after"))
            .await
            .unwrap_err();
        assert_eq!(err, RpcError::PoolClosed);

        let err = pool
            .send_fire_and_forget(ProgVers::new(1, 1), 1, call_with(b"after-faf"))
            .await
            .unwrap_err();
        assert_eq!(err, RpcError::PoolClosed);
    }

    #[tokio::test]
    async fn shutdown_is_idempotent_for_all_pools() {
        let addr = start_echo_server().await;

        let fixed = FixedSerialPool::connect(&addr, 1).await.unwrap();
        fixed.shutdown().await;
        fixed.shutdown().await;

        let dynamic = DynamicPool::new(addr, 1, Duration::from_secs(60), None);
        dynamic.shutdown().await;
        dynamic.shutdown().await;

        let cpu = CpuSetPool::connect_with_map(&addr, 1, vec![0])
            .await
            .unwrap();
        cpu.shutdown().await;
        cpu.shutdown().await;
    }

    // --- DynamicPool tests ---

    #[test]
    #[should_panic(expected = "idle_ttl must be greater than zero")]
    fn dynamic_pool_rejects_zero_idle_ttl() {
        let addr = "127.0.0.1:1".parse().unwrap();
        let _ = DynamicPool::new(addr, 1, Duration::ZERO, None);
    }

    #[tokio::test]
    async fn dynamic_pool_opens_connections_lazily() {
        let addr = start_echo_server().await;

        let pool = DynamicPool::new(addr, 3, Duration::from_secs(60), None);
        assert_eq!(pool.active_connections().await, 0);

        let result = pool
            .send_call(ProgVers::new(1, 1), 1, call_with(b"hello"))
            .await
            .unwrap();
        assert_eq!(result, b"hello");
        assert_eq!(pool.active_connections().await, 1);
    }

    #[tokio::test]
    async fn dynamic_pool_sequential_reuses_connection() {
        let addr = start_echo_server().await;

        let pool = DynamicPool::new(addr, 3, Duration::from_secs(60), None);

        // Sequential calls should reuse the same (idle) connection.
        for i in 0..5u32 {
            let data = format!("seq-{i}");
            let result = pool
                .send_call(ProgVers::new(1, 1), 1, call_with(data.as_bytes()))
                .await
                .unwrap();
            assert_eq!(result, data.as_bytes());
        }

        // Only one connection needed for sequential calls.
        assert_eq!(pool.active_connections().await, 1);
    }

    #[tokio::test]
    async fn dynamic_pool_balances_by_pending() {
        let (addr, mut started_rx, release) = start_holding_server().await;

        let pool = Arc::new(DynamicPool::new(addr, 3, Duration::from_secs(60), None));

        // Hold one call open on each connection. Starting calls sequentially
        // after the server observes the previous one makes the pending count,
        // and therefore the expected connection growth, deterministic.
        let mut handles = Vec::new();
        for i in 0..3u32 {
            let call_pool = pool.clone();
            handles.push(tokio::spawn(async move {
                let data = format!("bal-{i}");
                call_pool
                    .send_call(ProgVers::new(1, 1), 1, call_with(data.as_bytes()))
                    .await
                    .unwrap()
            }));
            await_call_started(&mut started_rx).await;
            assert_eq!(pool.active_connections().await, i as usize + 1);
        }

        release.add_permits(handles.len());
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

        let pool = Arc::new(DynamicPool::new(addr, 2, Duration::from_secs(60), None));

        // Send 4 concurrent calls but only allow 2 connections.
        let mut handles = Vec::new();
        for i in 0..4u32 {
            let pool = pool.clone();
            handles.push(tokio::spawn(async move {
                let data = format!("max-{i}");
                pool.send_call(ProgVers::new(1, 1), 1, call_with(data.as_bytes()))
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

        let pool = Arc::new(DynamicPool::new(addr, 3, Duration::from_millis(200), None));

        // Make a call to open a connection.
        pool.send_call(ProgVers::new(1, 1), 1, call_with(b"open"))
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

        let pool = Arc::new(DynamicPool::new(addr, 3, Duration::from_millis(100), None));

        // Start a slow call.
        let pool2 = pool.clone();
        let call_handle = tokio::spawn(async move {
            pool2
                .send_call(ProgVers::new(1, 1), 1, call_with(b"busy"))
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
        let (addr, mut started_rx, release) = start_holding_server().await;

        let pool = Arc::new(DynamicPool::new(addr, 3, Duration::from_secs(60), None));

        // Before any calls, pending should be 0.
        assert_eq!(pool.pending_count().await, 0);

        // Hold three calls open. A call is counted as pending before it is
        // written to the wire and stays counted until its reply arrives, so
        // once the server has observed it the count is exact: no sleep and
        // no dependence on how quickly the machine schedules the tasks.
        let mut handles = Vec::new();
        for i in 0..3u32 {
            let call_pool = pool.clone();
            handles.push(tokio::spawn(async move {
                let data = format!("pend-{i}");
                call_pool
                    .send_call(ProgVers::new(1, 1), 1, call_with(data.as_bytes()))
                    .await
                    .unwrap()
            }));
            await_call_started(&mut started_rx).await;
            assert_eq!(pool.pending_count().await, i as usize + 1);
        }

        release.add_permits(handles.len());
        for handle in handles {
            handle.await.unwrap();
        }

        // After completion, pending should be 0.
        assert_eq!(pool.pending_count().await, 0);
    }

    #[tokio::test]
    async fn dynamic_pool_error_propagation() {
        let addr = start_echo_server().await;

        let pool = DynamicPool::new(addr, 2, Duration::from_secs(60), None);

        // Invalid procedure should propagate the error.
        let result = pool
            .send_call(ProgVers::new(1, 1), 99, call_with(b"bad"))
            .await;
        assert_eq!(result, Err(RpcError::InvalidProcedure));

        // Pool should still work after an error.
        let result = pool
            .send_call(ProgVers::new(1, 1), 1, call_with(b"ok"))
            .await
            .unwrap();
        assert_eq!(result, b"ok");
    }

    #[tokio::test]
    async fn dynamic_pool_connect_failure() {
        let addr: SocketAddr = "127.0.0.1:1".parse().unwrap();
        let pool = DynamicPool::new(addr, 3, Duration::from_secs(60), None);
        let result = pool
            .send_call(ProgVers::new(1, 1), 1, call_with(b"fail"))
            .await;
        assert_eq!(result, Err(RpcError::ConnectFailed));
        assert_eq!(pool.connecting.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn dynamic_pool_canceled_lease_releases_capacity() {
        let addr = start_echo_server().await;

        let attempts = Arc::new(AtomicUsize::new(0));
        let connect_attempts = attempts.clone();
        let in_connect = Arc::new(Notify::new());
        let connect_started = in_connect.clone();
        let mut pool = DynamicPool::new(addr, 1, Duration::from_secs(60), None);
        pool.connection_factory = ConnectionFactory::with_override(addr, move || {
            let attempt = connect_attempts.fetch_add(1, Ordering::SeqCst);
            let connect_started = connect_started.clone();
            let fut: ConnectFuture = Box::pin(async move {
                if attempt == 0 {
                    connect_started.notify_one();
                    std::future::pending().await
                } else {
                    connect_client(&addr).await
                }
            });
            fut
        });
        let pool = Arc::new(pool);

        let leasing_pool = pool.clone();
        let leasing = tokio::spawn(async move {
            leasing_pool
                .send_call(ProgVers::new(1, 1), 1, call_with(b"canceled"))
                .await
        });
        tokio::time::timeout(Duration::from_secs(1), in_connect.notified())
            .await
            .expect("first lease should start connecting");
        assert_eq!(pool.connecting.load(Ordering::SeqCst), 1);

        leasing.abort();
        assert!(leasing.await.unwrap_err().is_cancelled());
        assert_eq!(
            pool.connecting.load(Ordering::SeqCst),
            0,
            "canceled lease should release its connecting slot"
        );

        // With max_connections = 1, a leaked slot would park this call forever.
        let response = tokio::time::timeout(
            Duration::from_secs(1),
            pool.send_call(ProgVers::new(1, 1), 1, call_with(b"after")),
        )
        .await
        .expect("canceled lease should not exhaust pool capacity")
        .unwrap();
        assert_eq!(response, b"after");
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn dynamic_pool_shutdown_unblocks_waiting_lease() {
        let addr: SocketAddr = "127.0.0.1:1".parse().unwrap();

        let in_connect = Arc::new(Notify::new());
        let connect_started = in_connect.clone();
        let mut pool = DynamicPool::new(addr, 1, Duration::from_secs(60), None);
        pool.connection_factory = ConnectionFactory::with_override(addr, move || {
            let connect_started = connect_started.clone();
            let fut: ConnectFuture = Box::pin(async move {
                connect_started.notify_one();
                std::future::pending().await
            });
            fut
        });
        let pool = Arc::new(pool);

        // First caller reserves the only slot and parks in connect forever.
        let connecting_pool = pool.clone();
        let connecting = tokio::spawn(async move {
            connecting_pool
                .send_call(ProgVers::new(1, 1), 1, call_with(b"stuck"))
                .await
        });
        tokio::time::timeout(Duration::from_secs(1), in_connect.notified())
            .await
            .expect("first lease should start connecting");

        // Second caller finds the pool empty and at capacity, so it waits.
        let waiting_pool = pool.clone();
        let waiting = tokio::spawn(async move {
            waiting_pool
                .send_call(ProgVers::new(1, 1), 1, call_with(b"waiting"))
                .await
        });
        // Let the waiter reach its wait state before shutting down. The
        // assertion below must hold for any interleaving; this just makes
        // the test exercise the interesting one.
        tokio::task::yield_now().await;

        pool.shutdown().await;

        // The stuck connect attempt holds the pool at capacity forever, so
        // only the shutdown path can unblock the waiter.
        let result = tokio::time::timeout(Duration::from_secs(1), waiting)
            .await
            .expect("shutdown should unblock a waiting lease")
            .unwrap();
        assert_eq!(result, Err(RpcError::PoolClosed));
        connecting.abort();
    }

    #[tokio::test]
    async fn dynamic_pool_canceled_lease_closes_pending_connection() {
        let (addr, closed_rx, release_server_tx, server) = start_eof_probe_server().await;

        let gate = Arc::new(Notify::new());
        let connect_gate = gate.clone();
        let (handle_tx, handle_rx) = oneshot::channel();
        let handle_tx = Arc::new(StdMutex::new(Some(handle_tx)));
        let mut pool = DynamicPool::new(addr, 1, Duration::from_secs(60), None);
        pool.connection_factory = ConnectionFactory::with_override(addr, move || {
            let gate = connect_gate.clone();
            let handle_tx = handle_tx.clone();
            let fut: ConnectFuture = Box::pin(async move {
                gate.notified().await;
                let client = connect_client(&addr).await?;
                if let Some(tx) = handle_tx.lock().unwrap().take() {
                    let _ = tx.send(Arc::downgrade(client.handle()));
                }
                Ok(client)
            });
            fut
        });
        let pool = Arc::new(pool);

        let leasing_pool = pool.clone();
        let leasing = tokio::spawn(async move {
            leasing_pool
                .send_call(ProgVers::new(1, 1), 1, call_with(b"canceled"))
                .await
        });

        // Wait for the lease to reserve its slot, then grab the connections
        // lock so the lease parks before it can install the new handle.
        tokio::time::timeout(Duration::from_secs(1), async {
            while pool.connecting.load(Ordering::SeqCst) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("first lease should reserve a connecting slot");
        let conns_guard = pool.connections.lock().await;
        gate.notify_one();
        let pending_handle = tokio::time::timeout(Duration::from_secs(1), handle_rx)
            .await
            .expect("lease should open its connection")
            .unwrap();

        leasing.abort();
        assert!(leasing.await.unwrap_err().is_cancelled());
        drop(conns_guard);

        let read_result = tokio::time::timeout(Duration::from_secs(1), closed_rx)
            .await
            .expect("pending connection should close when the lease is canceled")
            .unwrap();
        assert_eq!(read_result.unwrap(), 0);
        tokio::time::timeout(Duration::from_secs(1), async {
            while pending_handle.upgrade().is_some() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("pending connection handle should be released");
        assert_eq!(pool.connecting.load(Ordering::SeqCst), 0);
        assert_eq!(pool.active_connections().await, 0);

        release_server_tx.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(1), server)
            .await
            .expect("server task should stop")
            .unwrap();
    }

    #[tokio::test]
    async fn dynamic_pool_reopens_after_eviction() {
        let addr = start_echo_server().await;

        let pool = Arc::new(DynamicPool::new(addr, 3, Duration::from_millis(100), None));

        // Open a connection.
        pool.send_call(ProgVers::new(1, 1), 1, call_with(b"first"))
            .await
            .unwrap();
        assert_eq!(pool.active_connections().await, 1);

        // Start evictor and wait for eviction.
        let evictor = pool.start_evictor();
        tokio::time::sleep(Duration::from_millis(250)).await;
        assert_eq!(pool.active_connections().await, 0);

        // New call should open a fresh connection.
        let result = pool
            .send_call(ProgVers::new(1, 1), 1, call_with(b"second"))
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
            .send_call(ProgVers::new(1, 1), 1, call_with(b"via-trait"))
            .await
            .unwrap();
        assert_eq!(result, b"via-trait");
    }

    #[tokio::test]
    async fn dynamic_pool_as_rpc_sender() {
        let addr = start_echo_server().await;
        let pool = Arc::new(DynamicPool::new(addr, 2, Duration::from_secs(60), None));

        // Use pool through the RpcSender trait object
        let sender: Arc<dyn RpcSender> = pool;
        let result = sender
            .send_call(ProgVers::new(1, 1), 1, call_with(b"via-trait"))
            .await
            .unwrap();
        assert_eq!(result, b"via-trait");
    }

    // -- CpuSetPool tests -------------------------------------------------------

    #[tokio::test]
    async fn cpu_pool_invalid_configuration_rejected() {
        let addr: SocketAddr = "127.0.0.1:1".parse().unwrap();

        assert_eq!(
            CpuSetPool::connect_with_map(&addr, 0, Vec::new())
                .await
                .unwrap_err(),
            RpcError::InvalidPoolConfiguration
        );
        assert_eq!(
            CpuSetPool::connect_with_map(&addr, 1, vec![1])
                .await
                .unwrap_err(),
            RpcError::InvalidPoolConfiguration
        );
    }

    #[tokio::test]
    async fn cpu_pool_auto_detect() {
        let addr = start_echo_server().await;
        let pool = CpuSetPool::connect(&addr).await.unwrap();
        assert!(pool.size() >= 1);

        // Basic echo
        let result = pool
            .send_call(ProgVers::new(1, 1), 1, call_with(b"cpu-pool"))
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
                .send_call(ProgVers::new(1, 1), 1, call_with(data.as_bytes()))
                .await
                .unwrap();
            assert_eq!(result, data.as_bytes());
        }
    }

    #[tokio::test]
    async fn cpu_pool_applies_connection_config() {
        let addr = start_echo_server().await;
        let pool =
            CpuSetPool::connect_with_map_and_config(&addr, 1, vec![0], limited_client_config(128))
                .await
                .unwrap();

        let oversized_response = vec![0; 256];
        let result = tokio::time::timeout(
            Duration::from_secs(1),
            pool.send_call(ProgVers::new(1, 1), 1, call_with(&oversized_response)),
        )
        .await
        .expect("configured message limit should reject the response promptly");

        assert_eq!(result, Err(RpcError::SystemError));
    }

    #[tokio::test]
    async fn cpu_pool_connect_cancellation_closes_partial_connections() {
        let (addr, closed_rx, release_server_tx, server) = start_eof_probe_server().await;

        let attempts = Arc::new(AtomicUsize::new(0));
        let connect_attempts = attempts.clone();
        let (first_handle_tx, first_handle_rx) = oneshot::channel();
        let mut first_handle_tx = Some(first_handle_tx);
        let (second_attempt_tx, second_attempt_rx) = oneshot::channel();
        let mut second_attempt_tx = Some(second_attempt_tx);
        let construction = tokio::spawn(async move {
            CpuSetPool::connect_with(&addr, 2, vec![0, 1], move || {
                let attempt = connect_attempts.fetch_add(1, Ordering::SeqCst);
                let first_handle_tx = if attempt == 0 {
                    first_handle_tx.take()
                } else {
                    None
                };
                let second_attempt_tx = if attempt == 1 {
                    second_attempt_tx.take()
                } else {
                    None
                };
                async move {
                    if attempt == 0 {
                        let client = connect_client(&addr).await?;
                        first_handle_tx
                            .unwrap()
                            .send(Arc::downgrade(client.handle()))
                            .unwrap();
                        Ok(client)
                    } else {
                        second_attempt_tx.unwrap().send(()).unwrap();
                        std::future::pending().await
                    }
                }
            })
            .await
        });

        let first_handle = tokio::time::timeout(Duration::from_secs(1), first_handle_rx)
            .await
            .expect("first connection should complete")
            .unwrap();
        tokio::time::timeout(Duration::from_secs(1), second_attempt_rx)
            .await
            .expect("second connection attempt should start")
            .unwrap();
        construction.abort();
        assert!(
            tokio::time::timeout(Duration::from_secs(1), construction)
                .await
                .expect("pool construction should stop after cancellation")
                .unwrap_err()
                .is_cancelled()
        );

        let read_result = tokio::time::timeout(Duration::from_secs(1), closed_rx)
            .await
            .expect("partial connection should close when pool construction is canceled")
            .unwrap();
        assert_eq!(read_result.unwrap(), 0);
        tokio::time::timeout(Duration::from_secs(1), async {
            while first_handle.upgrade().is_some() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("partial connection handle should be released without peer cooperation");

        release_server_tx.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(1), server)
            .await
            .expect("server task should stop")
            .unwrap();
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn cpu_pool_connect_failure_closes_partial_connections() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut byte = [0u8; 1];
            stream.read(&mut byte).await.unwrap()
        });

        let attempts = Arc::new(AtomicUsize::new(0));
        let connect_attempts = attempts.clone();
        let (first_handle_tx, first_handle_rx) = oneshot::channel();
        let mut first_handle_tx = Some(first_handle_tx);
        let result = tokio::time::timeout(
            Duration::from_secs(1),
            CpuSetPool::connect_with(&addr, 2, vec![0, 1], move || {
                let attempt = connect_attempts.fetch_add(1, Ordering::SeqCst);
                let first_handle_tx = if attempt == 0 {
                    first_handle_tx.take()
                } else {
                    None
                };
                async move {
                    if attempt == 0 {
                        let client = connect_client(&addr).await?;
                        first_handle_tx
                            .unwrap()
                            .send(Arc::downgrade(client.handle()))
                            .unwrap();
                        Ok(client)
                    } else {
                        Err(std::io::Error::other("injected connection failure"))
                    }
                }
            }),
        )
        .await
        .expect("pool construction should return after connection failure");

        assert_eq!(result.unwrap_err(), RpcError::ConnectFailed);
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        let first_handle = first_handle_rx.await.unwrap();
        assert!(first_handle.upgrade().is_none());
        let bytes_read = tokio::time::timeout(Duration::from_secs(1), server)
            .await
            .expect("partial connection should close after connection failure")
            .unwrap();
        assert_eq!(bytes_read, 0);
    }

    #[tokio::test]
    async fn cpu_pool_as_rpc_sender() {
        let addr = start_echo_server().await;
        let pool = Arc::new(CpuSetPool::connect(&addr).await.unwrap());

        let sender: Arc<dyn RpcSender> = pool;
        let result = sender
            .send_call(ProgVers::new(1, 1), 1, call_with(b"via-trait"))
            .await
            .unwrap();
        assert_eq!(result, b"via-trait");
    }

    // -- fire-and-forget tests ---------------------------------------------------

    #[tokio::test]
    async fn fixed_pool_fire_and_forget() {
        let addr = start_echo_server().await;
        let pool = FixedSerialPool::connect(&addr, 2).await.unwrap();

        pool.send_fire_and_forget(ProgVers::new(1, 1), 1, call_with(b"faf-data"))
            .await
            .unwrap();

        let result = pool
            .send_call(ProgVers::new(1, 1), 1, call_with(b"after-faf"))
            .await
            .unwrap();
        assert_eq!(result, b"after-faf");
    }

    #[tokio::test]
    async fn dynamic_pool_fire_and_forget() {
        let addr = start_echo_server().await;
        let pool = Arc::new(DynamicPool::new(addr, 3, Duration::from_secs(60), None));

        pool.send_fire_and_forget(ProgVers::new(1, 1), 1, call_with(b"faf-data"))
            .await
            .unwrap();

        // Poll until the reply arrives and pending count drops to 0.
        for _ in 0..20 {
            if pool.pending_count().await == 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert_eq!(pool.pending_count().await, 0);

        // Normal send_call should still work afterwards.
        let result = pool
            .send_call(ProgVers::new(1, 1), 1, call_with(b"after-faf"))
            .await
            .unwrap();
        assert_eq!(result, b"after-faf");
    }

    #[tokio::test]
    async fn fire_and_forget_via_trait_object() {
        let addr = start_echo_server().await;
        let pool = Arc::new(FixedSerialPool::connect(&addr, 2).await.unwrap());

        // Cast to trait object.
        let sender: Arc<dyn RpcSender> = pool;

        // Fire-and-forget through trait object.
        sender
            .send_fire_and_forget(ProgVers::new(1, 1), 1, call_with(b"trait-faf"))
            .await
            .unwrap();

        // Normal send_call should still work through the trait object.
        let result = sender
            .send_call(ProgVers::new(1, 1), 1, call_with(b"trait-call"))
            .await
            .unwrap();
        assert_eq!(result, b"trait-call");
    }

    #[tokio::test]
    async fn multiplex_pool_distributes_calls() {
        let addr = start_echo_server().await;
        let pool = MultiplexPool::connect(&addr, 3).await.unwrap();
        assert_eq!(pool.size(), 3);

        for i in 0..9u32 {
            let data = format!("call-{i}");
            let result = pool
                .send_call(ProgVers::new(1, 1), 1, call_with(data.as_bytes()))
                .await
                .unwrap();
            assert_eq!(result, data.as_bytes());
        }
        assert_eq!(pool.pending_count(), 0);
    }

    #[tokio::test]
    async fn multiplex_pool_balances_by_in_flight() {
        let (started_tx, mut started_rx) = tokio::sync::mpsc::unbounded_channel();
        let release = Arc::new(Semaphore::new(0));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let mut services = ServiceMap::new();
        services.add_service(Box::new(HoldingEchoService {
            started: started_tx,
            release: release.clone(),
        }));
        let config = ConnectionConfig::new(None, services);
        RpcServer::new_with_config(listener, config).start().await;

        let pool = Arc::new(MultiplexPool::connect(&addr, 2).await.unwrap());
        let mut calls = Vec::new();
        for _ in 0..4 {
            let pool = pool.clone();
            calls.push(tokio::spawn(async move {
                pool.send_call(ProgVers::new(1, 1), 1, call_with(b"held"))
                    .await
            }));
        }
        // Every call is counted on some slot while it waits.
        tokio::time::timeout(Duration::from_secs(2), async {
            while pool.pending_count() < 4 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let per_slot: Vec<usize> = pool
            .inner
            .slots
            .iter()
            .map(|slot| slot.in_flight.load(Ordering::Relaxed))
            .collect();
        assert_eq!(
            per_slot,
            vec![2, 2],
            "calls should spread across connections"
        );

        // Release whichever calls the server has started so far, then all.
        for _ in 0..4 {
            tokio::time::timeout(Duration::from_secs(2), started_rx.recv())
                .await
                .unwrap()
                .unwrap();
            release.add_permits(1);
        }
        for call in calls {
            assert_eq!(call.await.unwrap().unwrap(), b"held");
        }
        assert_eq!(pool.pending_count(), 0);
    }

    #[tokio::test]
    async fn multiplex_pool_replaces_connection_after_peer_disconnect() {
        let (addr, initial_connection_dropped) = start_drop_then_echo_server().await;
        let pool = MultiplexPool::connect(&addr, 1).await.unwrap();
        initial_connection_dropped.notified().await;

        let response = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                match pool
                    .send_call(ProgVers::new(1, 1), 1, call_with(b"after-restart"))
                    .await
                {
                    Ok(response) => break response,
                    Err(RpcError::SystemError) => tokio::task::yield_now().await,
                    Err(error) => panic!("unexpected error while replacing connection: {error}"),
                }
            }
        })
        .await
        .expect("multiplex pool should replace a peer-disconnected connection");
        assert_eq!(response, b"after-restart");
        assert!(!pool.inner.slots[0].replacing.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn multiplex_pool_canceled_call_keeps_connection() {
        let (addr, blocked_started, blocked_release, calls) = start_cancellation_server().await;
        let pool = Arc::new(MultiplexPool::connect(&addr, 1).await.unwrap());
        let initial = pool.inner.slots[0].handle.read().unwrap().clone();

        let blocked = {
            let pool = pool.clone();
            tokio::spawn(async move {
                pool.send_call(ProgVers::new(1, 1), 1, call_with(b"blocked"))
                    .await
            })
        };
        blocked_started.notified().await;
        blocked.abort();
        assert!(blocked.await.unwrap_err().is_cancelled());
        assert_eq!(pool.pending_count(), 0);

        // Let the canceled call's handler finish; its late reply is
        // discarded. The same connection keeps serving.
        blocked_release.notify_one();
        let result = pool
            .send_call(ProgVers::new(1, 1), 1, call_with(b"after-cancel"))
            .await
            .unwrap();
        assert_eq!(result, b"after-cancel");
        assert!(Arc::ptr_eq(
            &initial,
            &pool.inner.slots[0].handle.read().unwrap()
        ));
        assert_eq!(calls.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn multiplex_pool_zero_size_rejected() {
        let addr = start_echo_server().await;
        let result = MultiplexPool::connect(&addr, 0).await;
        assert!(matches!(result, Err(RpcError::InvalidPoolConfiguration)));
    }

    #[tokio::test]
    async fn multiplex_pool_connect_failure_closes_partial_connections() {
        let (addr, closed_rx, release_tx, server) = start_eof_probe_server().await;
        let attempts = Arc::new(AtomicUsize::new(0));
        let attempt_counter = attempts.clone();
        let result = MultiplexPool::connect_with(&addr, 2, move || {
            let attempt = attempt_counter.fetch_add(1, Ordering::SeqCst);
            async move {
                if attempt == 0 {
                    connect_client(&addr).await
                } else {
                    Err(std::io::Error::other("second connection refused"))
                }
            }
        })
        .await;
        assert!(matches!(result, Err(RpcError::ConnectFailed)));
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), closed_rx)
                .await
                .unwrap()
                .unwrap()
                .unwrap(),
            0,
            "the connection opened before the failure should be closed"
        );
        release_tx.send(()).unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn multiplex_pool_shutdown_rejects_new_calls() {
        let addr = start_echo_server().await;
        let pool = MultiplexPool::connect(&addr, 2).await.unwrap();
        pool.shutdown().await;
        pool.shutdown().await;
        assert_eq!(
            pool.send_call(ProgVers::new(1, 1), 1, call_with(b"closed"))
                .await,
            Err(RpcError::PoolClosed)
        );
    }

    #[tokio::test]
    async fn multiplex_pool_fire_and_forget_and_trait_object() {
        let addr = start_echo_server().await;
        let sender: Arc<dyn RpcSender> = Arc::new(MultiplexPool::connect(&addr, 2).await.unwrap());
        sender
            .send_fire_and_forget(ProgVers::new(1, 1), 1, call_with(b"faf-data"))
            .await
            .unwrap();
        let result = sender
            .send_call(ProgVers::new(1, 1), 1, call_with(b"after-faf"))
            .await
            .unwrap();
        assert_eq!(result, b"after-faf");
    }

    /// Wait until `condition` holds, yielding between checks.
    async fn wait_until(mut condition: impl FnMut() -> bool, what: &str) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while !condition() {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for {what}"));
    }

    #[tokio::test]
    async fn multiplex_pool_routes_around_idle_peer_disconnect() {
        let (addr, initial_connection_dropped) = start_drop_then_echo_server().await;
        let pool = MultiplexPool::connect(&addr, 2).await.unwrap();
        initial_connection_dropped.notified().await;
        let poisoned = |slot: &MultiplexSlot| slot.handle.read().unwrap().is_poisoned();
        wait_until(
            || pool.inner.slots.iter().any(poisoned),
            "the read loop to notice the closed connection",
        )
        .await;

        // No call fails: the one routed to the dead connection moves to the
        // healthy one and the dead one is replaced in the background.
        for i in 0..8u32 {
            let data = format!("idle-{i}");
            let result = pool
                .send_call(ProgVers::new(1, 1), 1, call_with(data.as_bytes()))
                .await
                .unwrap();
            assert_eq!(result, data.as_bytes());
        }
        wait_until(
            || {
                pool.inner
                    .slots
                    .iter()
                    .all(|slot| !slot.replacing.load(Ordering::Acquire) && !poisoned(slot))
            },
            "the replacement connection",
        )
        .await;
        assert_eq!(pool.pending_count(), 0);
        pool.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn multiplex_pool_balances_under_concurrent_callers() {
        let (started_tx, mut started_rx) = tokio::sync::mpsc::unbounded_channel();
        let release = Arc::new(Semaphore::new(0));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let mut services = ServiceMap::new();
        services.add_service(Box::new(HoldingEchoService {
            started: started_tx,
            release: release.clone(),
        }));
        let config = ConnectionConfig::new(None, services);
        RpcServer::new_with_config(listener, config).start().await;

        const CONNECTIONS: usize = 4;
        const CALLS: usize = 64;
        let pool = Arc::new(MultiplexPool::connect(&addr, CONNECTIONS).await.unwrap());
        let go = Arc::new(tokio::sync::Barrier::new(CALLS));
        let mut calls = Vec::new();
        for _ in 0..CALLS {
            let pool = pool.clone();
            let go = go.clone();
            calls.push(tokio::spawn(async move {
                go.wait().await;
                pool.send_call(ProgVers::new(1, 1), 1, call_with(b"held"))
                    .await
            }));
        }
        wait_until(
            || pool.pending_count() == CALLS,
            "every call to be in flight",
        )
        .await;
        // Reservations are compare-and-swap against the observed count, so
        // callers racing for the same least-loaded slot spread out exactly.
        let per_slot: Vec<usize> = pool
            .inner
            .slots
            .iter()
            .map(|slot| slot.in_flight.load(Ordering::Relaxed))
            .collect();
        assert_eq!(per_slot, vec![CALLS / CONNECTIONS; CONNECTIONS]);

        for _ in 0..CALLS {
            tokio::time::timeout(Duration::from_secs(5), started_rx.recv())
                .await
                .unwrap()
                .unwrap();
            release.add_permits(1);
        }
        for call in calls {
            assert_eq!(call.await.unwrap().unwrap(), b"held");
        }
        assert_eq!(pool.pending_count(), 0);
        pool.shutdown().await;
    }

    #[tokio::test]
    async fn multiplex_pool_shutdown_stops_pending_replacement() {
        let (addr, initial_connection_dropped) = start_drop_then_echo_server().await;
        let attempts = Arc::new(AtomicUsize::new(0));
        let attempt_counter = attempts.clone();
        // The first connect succeeds (and is then dropped by the peer);
        // every replacement attempt fails, so the task sits in backoff.
        let pool = MultiplexPool::connect_with(&addr, 1, move || {
            let attempt = attempt_counter.fetch_add(1, Ordering::SeqCst);
            async move {
                if attempt == 0 {
                    connect_client(&addr).await
                } else {
                    Err(std::io::Error::other("replacement refused"))
                }
            }
        })
        .await
        .unwrap();
        initial_connection_dropped.notified().await;
        wait_until(
            || pool.inner.slots[0].handle.read().unwrap().is_poisoned(),
            "the read loop to notice the closed connection",
        )
        .await;

        // With no healthy connection the call fails fast and starts the
        // replacement.
        assert_eq!(
            pool.send_call(ProgVers::new(1, 1), 1, call_with(b"down"))
                .await,
            Err(RpcError::SystemError)
        );
        wait_until(
            || attempts.load(Ordering::SeqCst) >= 2,
            "the replacement task to start retrying",
        )
        .await;

        // Shutdown returns without waiting out the backoff and the task is
        // gone with it.
        tokio::time::timeout(Duration::from_secs(2), pool.shutdown())
            .await
            .expect("shutdown should not wait for the replacement backoff");
        assert!(pool.inner.replacements.lock().unwrap().is_empty());
        let attempts_after_shutdown = attempts.load(Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(attempts.load(Ordering::SeqCst), attempts_after_shutdown);
    }
}
