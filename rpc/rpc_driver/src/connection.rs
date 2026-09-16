//! Connection state and RPC call handling.

use std::collections::HashMap;
use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::task::Waker;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::Instrument;

use async_trait::async_trait;

use crate::{ProgVers, RpcError, rpc_msg, tcp_connection::WriteHalf};

const MAX_BATCH: usize = 64;

/// XDR header is ~40 bytes (xid + msg_type + rpcvers + prog + vers + proc + 2x opaque_auth).
const XDR_HEADER_ESTIMATE: usize = 48;

/// Most spent record buffers the pool keeps; enough for a few hundred calls
/// in flight to cycle without allocating.
const MAX_POOLED_BUFFERS: usize = 256;

/// Largest allocation the pool keeps. A rare huge record does not pin its
/// memory for the connection's lifetime.
const MAX_POOLED_CAPACITY: usize = 1 << 20;

// ---------------------------------------------------------------------------
// Buffer pool — recycles Vec<u8> allocations across RPC messages
// ---------------------------------------------------------------------------

/// Pool of spent record buffers the writer task hands back for reuse: callers
/// draw their next [`CallBuf`] from it and the server reply path its framing
/// buffer, so a steady stream of records allocates nothing. Bounded to
/// [`MAX_POOLED_BUFFERS`] buffers of at most [`MAX_POOLED_CAPACITY`] bytes.
///
/// Every connection has one; a connection pool shares one across its
/// connections.
pub(crate) struct BufPool(StdMutex<Vec<Vec<u8>>>);

impl std::fmt::Debug for BufPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let len = self.0.lock().map_or(0, |v| v.len());
        f.debug_tuple("BufPool").field(&len).finish()
    }
}

impl BufPool {
    pub(crate) fn new() -> Self {
        Self(StdMutex::new(Vec::new()))
    }

    /// Take a cleared buffer from the pool, or allocate a new one.
    pub(crate) fn take(&self) -> Vec<u8> {
        self.0.lock().unwrap().pop().unwrap_or_default()
    }

    /// A call buffer backed by a pooled allocation when one is available.
    pub(crate) fn take_call_buf(&self) -> CallBuf {
        CallBuf::from_spare(self.take())
    }

    /// Return a writer batch with one lock acquisition, rather than contending
    /// with callers once per record in the batch. Buffers beyond the count
    /// cap or over the capacity cap are dropped.
    fn put_batch(&self, batch: &mut Vec<Vec<u8>>) {
        let mut pooled = self.0.lock().unwrap();
        for mut buf in batch.drain(..) {
            if pooled.len() >= MAX_POOLED_BUFFERS || buf.capacity() > MAX_POOLED_CAPACITY {
                continue;
            }
            buf.clear();
            pooled.push(buf);
        }
    }
}

/// Dedicated writer task: owns the WriteHalf, drains the channel, and
/// flushes batches via write_vectored. Spent buffers are returned to the pool.
async fn writer_task(
    mut rx: mpsc::Receiver<Vec<u8>>,
    mut write_half: WriteHalf,
    pool: Arc<BufPool>,
) {
    let mut batch: Vec<Vec<u8>> = Vec::with_capacity(MAX_BATCH);

    while let Some(first) = rx.recv().await {
        batch.push(first);

        // Opportunistic drain — cap to avoid unbounded iovec arrays.
        while batch.len() < MAX_BATCH {
            match rx.try_recv() {
                Ok(msg) => batch.push(msg),
                Err(_) => break,
            }
        }

        let err = write_batch(&mut write_half, &batch).await.err();

        // Recycle all buffers back to the pool regardless of write outcome.
        pool.put_batch(&mut batch);

        if let Some(e) = err {
            tracing::error!(error = %e, "writer task error");
            break;
        }
    }
    // rx.recv() returned None — all senders dropped.
    // WriteHalf drops here → TCP FIN.
}

/// write_all_vectored: flush all messages in one (or few) syscalls.
async fn write_batch(wh: &mut WriteHalf, msgs: &[Vec<u8>]) -> Result<(), io::Error> {
    debug_assert!(msgs.len() <= MAX_BATCH);
    let mut slices_arr = [io::IoSlice::new(&[]); MAX_BATCH];
    let mut total: usize = 0;
    for (i, m) in msgs.iter().enumerate() {
        slices_arr[i] = io::IoSlice::new(m);
        total += m.len();
    }
    let mut remaining: &mut [io::IoSlice<'_>] = &mut slices_arr[..msgs.len()];

    while total > 0 {
        let n = wh.write_vectored(remaining).await?;
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::WriteZero, "write returned 0"));
        }
        total -= n;
        io::IoSlice::advance_slices(&mut remaining, n);
    }
    Ok(())
}

/// Frame an XDR-encoded RPC header + payload into a single-part record.
///
/// Layout: `[4-byte length with high bit set | XDR header | payload]`
///
/// The provided `buf` is reused if it has sufficient capacity (avoids
/// allocation after warmup when a compatible buffer is available in the
/// [`BufPool`]).
///
/// Used by both the client call path and the server response path.
#[inline]
pub(crate) fn frame_rpc_record(
    buf: Vec<u8>,
    header: &rpc_msg::rpc_msg,
    payload: &[u8],
) -> Result<Vec<u8>, ()> {
    let mut writer = xdrs::txs::buf_writer::BufWriter::from(buf);
    writer.clear();
    writer.reserve(XDR_HEADER_ESTIMATE + payload.len());

    // Placeholder for record length (patched below).
    xdrs::write_xdr(&0u32, &mut writer).map_err(drop)?;
    xdrs::write_xdr(header, &mut writer).map_err(drop)?;

    let header_len = writer.len() - 4;
    let total_len = header_len
        .checked_add(payload.len())
        .and_then(|sum| u32::try_from(sum).ok())
        .filter(|&len| len <= 0x7FFF_FFFF)
        .ok_or(())?;

    let msg_len: u32 = total_len | 0x8000_0000;
    {
        let bytes: &mut Vec<u8> = writer.as_mut();
        bytes[..4].copy_from_slice(&msg_len.to_be_bytes());
        bytes.extend_from_slice(payload);
    }
    Ok(Vec::from(writer))
}

/// Frame an outgoing call in place.
///
/// Fills the prefix a [`CallBuf`] reserves with the record mark and the
/// RFC 5531 CALL header (`AUTH_NONE` credentials and verifier), and returns
/// the buffer as the complete single-part record. The payload is neither
/// copied nor moved. Fails only when the record would exceed the 31-bit
/// record-mark length.
#[inline]
pub(crate) fn frame_call(
    call: CallBuf,
    xid: u32,
    progvers: ProgVers,
    proc: u32,
) -> Result<Vec<u8>, ()> {
    const CALL_HEADER_WORDS: usize = 10;
    const { assert!(CallBuf::PREFIX_LEN == 4 + 4 * CALL_HEADER_WORDS) };

    let mut record = call.into_vec();
    let body_len = record.len() - 4;
    let msg_len = u32::try_from(body_len)
        .ok()
        .filter(|&len| len <= 0x7FFF_FFFF)
        .ok_or(())?
        | 0x8000_0000;

    let words: [u32; 1 + CALL_HEADER_WORDS] = [
        msg_len,
        xid,
        rpc_msg::msg_type::CALL.0 as u32,
        RPC_VERSION,
        progvers.prog(),
        progvers.vers(),
        proc,
        rpc_msg::auth_flavor::AUTH_NONE.0 as u32, // cred.flavor
        0,                                        // cred.body length
        rpc_msg::auth_flavor::AUTH_NONE.0 as u32, // verf.flavor
        0,                                        // verf.body length
    ];
    for (slot, word) in record[..CallBuf::PREFIX_LEN].chunks_exact_mut(4).zip(words) {
        slot.copy_from_slice(&word.to_be_bytes());
    }
    Ok(record)
}

/// The ONC RPC version every call header carries.
const RPC_VERSION: u32 = 2;

// CallBuf, RpcResponse and the RpcSender trait live in the stable `rpc_api`
// crate; re-exported here at their historical paths for source
// compatibility. This module implements RpcSender for [`ConnectionHandle`]
// and [`ClientConnection`]; the pools in [`crate::pool`] implement it too.
pub use rpc_api::{CallBuf, RpcResponse, RpcSender};

#[async_trait]
impl RpcSender for ConnectionHandle {
    fn call_buf(&self) -> CallBuf {
        ConnectionHandle::call_buf(self)
    }

    async fn send_call(
        &self,
        progvers: ProgVers,
        proc: u32,
        call: CallBuf,
    ) -> Result<RpcResponse, RpcError> {
        ConnectionHandle::send_call(self, progvers, proc, call).await
    }

    async fn send_fire_and_forget(
        &self,
        progvers: ProgVers,
        proc: u32,
        call: CallBuf,
    ) -> Result<(), RpcError> {
        ConnectionHandle::send_fire_and_forget(self, progvers, proc, call).await
    }
}

/// Result delivered to a pending RPC call.
///
/// Success carries `(record_buf, payload_offset)` — the raw record Vec and
/// the byte offset where the payload begins. The consumer slices at read
/// time, avoiding an O(n) `drain()` or `to_vec()` in the reader task.
pub(crate) enum ReplyResult {
    /// Success: record buffer + offset to payload start.
    Ok(Vec<u8>, usize),
    /// Error from the remote peer.
    Err(RpcError),
}

/// State of a pending RPC call slot, stored directly in the HashMap.
///
/// Replaces `oneshot::channel()` — the HashMap entry IS the rendezvous
/// point, eliminating one heap allocation per RPC call.
pub(crate) enum CallbackSlot {
    /// Entry registered, caller hasn't polled yet.
    Registered,
    /// Caller is parked, waker registered.
    Waiting(Waker),
    /// Reader has delivered the result.
    Ready(ReplyResult),
    /// Fire-and-forget: no caller waiting. Reader cleans up on reply.
    FireAndForget,
}

impl std::fmt::Debug for CallbackSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Registered => write!(f, "Registered"),
            Self::Waiting(_) => write!(f, "Waiting(..)"),
            Self::Ready(_) => write!(f, "Ready(..)"),
            Self::FireAndForget => write!(f, "FireAndForget"),
        }
    }
}

#[derive(Debug)]
pub(crate) struct ConnectionState {
    pub(crate) pending_requests: HashMap<u32, CallbackSlot>,
    last_xid: u32,
    poisoned: bool,
}

impl ConnectionState {
    pub(crate) fn new() -> Self {
        Self {
            pending_requests: HashMap::new(),
            last_xid: 0,
            poisoned: false,
        }
    }

    #[inline(always)]
    pub(crate) fn register_call(&mut self) -> Result<u32, RpcError> {
        if self.poisoned {
            return Err(RpcError::SystemError);
        }

        loop {
            self.last_xid = self.last_xid.wrapping_add(1);
            if let std::collections::hash_map::Entry::Vacant(e) =
                self.pending_requests.entry(self.last_xid)
            {
                e.insert(CallbackSlot::Registered);
                return Ok(self.last_xid);
            }
        }
    }

    #[inline(always)]
    pub(crate) fn register_fire_and_forget(&mut self) -> Result<u32, RpcError> {
        if self.poisoned {
            return Err(RpcError::SystemError);
        }

        loop {
            self.last_xid = self.last_xid.wrapping_add(1);
            if let std::collections::hash_map::Entry::Vacant(e) =
                self.pending_requests.entry(self.last_xid)
            {
                e.insert(CallbackSlot::FireAndForget);
                return Ok(self.last_xid);
            }
        }
    }

    #[inline(always)]
    pub(crate) fn handle_reply(&mut self, xid: u32, reply: ReplyResult) -> bool {
        if let Some(slot) = self.pending_requests.get_mut(&xid) {
            if matches!(slot, CallbackSlot::FireAndForget) {
                self.pending_requests.remove(&xid);
                return true;
            }
            let prev = std::mem::replace(slot, CallbackSlot::Ready(reply));
            if let CallbackSlot::Waiting(waker) = prev {
                waker.wake();
            }
            return false;
        }
        tracing::warn!(xid, "received reply for unknown xid");
        false
    }

    pub(crate) fn poison(&mut self) -> usize {
        self.poisoned = true;
        let mut faf_count = 0usize;
        for (_xid, slot) in self.pending_requests.drain() {
            match slot {
                CallbackSlot::FireAndForget => faf_count += 1,
                CallbackSlot::Waiting(waker) => waker.wake(),
                _ => {}
            }
        }
        faf_count
    }
}

impl Drop for ConnectionState {
    fn drop(&mut self) {
        self.poison();
    }
}

pub(crate) struct ConnectionShutdown {
    writer: Option<JoinHandle<()>>,
    client_read_runner: Option<crate::tasks::ClientReadRunner>,
}

impl ConnectionShutdown {
    pub(crate) fn start(&mut self) {
        if let Some(join) = &self.writer {
            join.abort();
        }
        if let Some(runner) = self.client_read_runner.as_mut() {
            runner.begin_shutdown();
        }
    }

    pub(crate) async fn finish(mut self) {
        self.start();
        if let Some(join) = self.writer.take() {
            let _ = join.await;
        }
        if let Some(mut runner) = self.client_read_runner.take() {
            runner.shutdown().await;
        }
    }
}

/// Handle for making RPC calls over an established connection.
///
/// Manages pending requests and provides methods for sending calls
/// and coordinated shutdown via poisoning.
#[derive(Debug)]
pub struct ConnectionHandle {
    // Hot — accessed on every RPC:
    pub(crate) connection_state: StdMutex<ConnectionState>,
    write_tx: mpsc::Sender<Vec<u8>>,
    pub(crate) buf_pool: Arc<BufPool>,
    /// Atomic pending-request counter — avoids locking `connection_state`
    /// for load-balancing queries in DynamicPool.
    pending_count: AtomicUsize,
    // Cold — accessed rarely:
    pub(crate) connection_span: tracing::Span,
    #[cfg_attr(not(feature = "concurrent-dispatch"), allow(dead_code))]
    pub(crate) max_concurrent_dispatches: usize,
    #[cfg_attr(not(feature = "concurrent-dispatch"), allow(dead_code))]
    pub(crate) read_channel_capacity: usize,
    #[cfg_attr(not(feature = "concurrent-dispatch"), allow(dead_code))]
    pub(crate) inline_ready_dispatch: bool,
    #[cfg_attr(not(feature = "concurrent-dispatch"), allow(dead_code))]
    pub(crate) write_channel_capacity: usize,
    writer_handle: StdMutex<Option<JoinHandle<()>>>,
    client_read_runner: StdMutex<Option<crate::tasks::ClientReadRunner>>,
}

impl ConnectionHandle {
    /// Construct a handle from the driver's low-level transport components.
    ///
    /// This is exposed for the driver's integration tests and is not part of
    /// the supported high-level API. Use [`connect_client`](crate::connect_client)
    /// for normal client connections.
    #[doc(hidden)]
    pub fn new(
        write_half: WriteHalf,
        config: &crate::ConnectionConfig,
        span: tracing::Span,
    ) -> Self {
        let pool = config
            .buf_pool
            .clone()
            .unwrap_or_else(|| Arc::new(BufPool::new()));
        let (tx, rx) = mpsc::channel(config.write_channel_capacity);
        let handle =
            tokio::spawn(writer_task(rx, write_half, pool.clone()).instrument(span.clone()));
        Self {
            connection_state: StdMutex::new(ConnectionState::new()),
            write_tx: tx,
            buf_pool: pool,
            pending_count: AtomicUsize::new(0),
            connection_span: span,
            writer_handle: StdMutex::new(Some(handle)),
            client_read_runner: StdMutex::new(None),
            max_concurrent_dispatches: config.max_concurrent_dispatches,
            read_channel_capacity: config.read_channel_capacity,
            inline_ready_dispatch: config.inline_ready_dispatch,
            write_channel_capacity: config.write_channel_capacity,
        }
    }

    pub(crate) fn attach_client_read_runner(&self, runner: crate::tasks::ClientReadRunner) {
        *self.client_read_runner.lock().unwrap() = Some(runner);
    }

    pub(crate) fn begin_poison(&self) -> ConnectionShutdown {
        // Fail all pending requests. Wakes callers parked in poll_fn,
        // which will see the entry removed and return Err(SystemError).
        let faf_count = self.connection_state.lock().unwrap().poison();
        if faf_count > 0 {
            let _ = self
                .pending_count
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |cur| {
                    Some(cur.saturating_sub(faf_count))
                });
        }

        let mut shutdown = ConnectionShutdown {
            writer: self.writer_handle.lock().unwrap().take(),
            client_read_runner: self.client_read_runner.lock().unwrap().take(),
        };
        shutdown.start();
        shutdown
    }

    /// Poison the connection, failing all pending requests and closing the write half.
    pub async fn poison(&self) {
        self.begin_poison().finish().await;
    }

    /// Send a pre-serialized message through the write channel.
    #[inline(always)]
    pub(crate) async fn send_msg(&self, msg: Vec<u8>) -> Result<(), io::Error> {
        self.write_tx
            .send(msg)
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "writer closed"))
    }

    /// Attempt to enqueue a pre-serialized message without waiting for channel
    /// capacity.
    #[cfg_attr(not(feature = "concurrent-dispatch"), allow(dead_code))]
    #[inline(always)]
    pub(crate) fn try_send_msg(
        &self,
        msg: Vec<u8>,
    ) -> Result<(), mpsc::error::TrySendError<Vec<u8>>> {
        self.write_tx.try_send(msg)
    }

    #[cfg(all(test, feature = "concurrent-dispatch"))]
    pub(crate) fn new_with_write_sender_for_test(
        write_tx: mpsc::Sender<Vec<u8>>,
        config: &crate::ConnectionConfig,
    ) -> Self {
        Self {
            connection_state: StdMutex::new(ConnectionState::new()),
            write_tx,
            buf_pool: Arc::new(BufPool::new()),
            pending_count: AtomicUsize::new(0),
            connection_span: tracing::Span::none(),
            writer_handle: StdMutex::new(None),
            client_read_runner: StdMutex::new(None),
            max_concurrent_dispatches: config.max_concurrent_dispatches,
            read_channel_capacity: config.read_channel_capacity,
            inline_ready_dispatch: config.inline_ready_dispatch,
            write_channel_capacity: config.write_channel_capacity,
        }
    }

    /// A buffer to encode the next call into, recycled from a record this
    /// connection has already written when one is available.
    pub fn call_buf(&self) -> CallBuf {
        self.buf_pool.take_call_buf()
    }

    /// Send an RPC call and wait for the response.
    ///
    /// The call is framed in place: the driver fills the prefix `call`
    /// reserves and hands the buffer to the socket without copying the
    /// payload. Returns the response data on success, or an error if the
    /// call fails.
    pub async fn send_call(
        &self,
        progvers: ProgVers,
        proc: u32,
        call: CallBuf,
    ) -> Result<RpcResponse, RpcError> {
        let xid = self.connection_state.lock().unwrap().register_call()?;
        self.pending_count.fetch_add(1, Ordering::Relaxed);

        let span = tracing::debug_span!(
            parent: &self.connection_span,
            "rpc_call",
            xid,
            prog = progvers.prog(),
            vers = progvers.vers(),
            proc = proc,
        );

        // Guard removes the pending entry on drop (cancellation safety).
        let guard = CallGuard {
            state: &self.connection_state,
            pending_count: &self.pending_count,
            xid,
            done: false,
        };

        // On error the guard's drop cleans up the pending entry.
        let msg = Self::frame_call_or_warn(call, xid, progvers, proc)?;

        let result = async {
            if self.write_tx.send(msg).await.is_err() {
                return Err(RpcError::SystemError);
            }

            self.await_reply(xid).await
        }
        .instrument(span)
        .await;

        guard.complete();
        result
    }

    /// Send an RPC call without waiting for the response.
    ///
    /// Completes after the message is queued for the connection's writer; it
    /// may wait for channel capacity under backpressure. Success does not
    /// confirm that the socket write or peer-side dispatch completed. The
    /// reader task silently discards the eventual reply.
    pub async fn send_fire_and_forget(
        &self,
        progvers: ProgVers,
        proc: u32,
        call: CallBuf,
    ) -> Result<(), RpcError> {
        let xid = self
            .connection_state
            .lock()
            .unwrap()
            .register_fire_and_forget()?;
        self.pending_count.fetch_add(1, Ordering::Relaxed);

        let span = tracing::debug_span!(
            parent: &self.connection_span,
            "rpc_call",
            xid,
            prog = progvers.prog(),
            vers = progvers.vers(),
            proc = proc,
            fire_and_forget = true,
        );

        // Guard ensures cleanup if the future is cancelled (e.g. timeout)
        // while awaiting channel send.
        let mut guard = FireAndForgetGuard {
            state: &self.connection_state,
            pending_count: &self.pending_count,
            xid,
            sent: false,
        };

        let msg = Self::frame_call_or_warn(call, xid, progvers, proc)?;

        // Use .instrument(span) instead of span.enter() — enter() is
        // thread-local and incorrect across .await points in multi-thread
        // runtimes (the future may resume on a different thread).
        async {
            if self.write_tx.send(msg).await.is_err() {
                return Err(RpcError::SystemError); // guard drops and cleans up
            }
            guard.sent = true;
            Ok(())
        }
        .instrument(span)
        .await
    }

    fn frame_call_or_warn(
        call: CallBuf,
        xid: u32,
        progvers: ProgVers,
        proc: u32,
    ) -> Result<Vec<u8>, RpcError> {
        let payload_len = call.payload().len();
        frame_call(call, xid, progvers, proc).map_err(|()| {
            tracing::warn!(payload_len, "message too large: exceeds max (0x7FFF_FFFF)");
            RpcError::SystemError
        })
    }

    /// Wait for the reader task to deliver a result for the given XID.
    async fn await_reply(&self, xid: u32) -> Result<RpcResponse, RpcError> {
        std::future::poll_fn(|cx| {
            let mut state = self.connection_state.lock().unwrap();
            match state.pending_requests.get_mut(&xid) {
                Some(CallbackSlot::Ready(_)) => match state.pending_requests.remove(&xid) {
                    Some(CallbackSlot::Ready(reply)) => std::task::Poll::Ready(match reply {
                        ReplyResult::Ok(buf, offset) => Ok(RpcResponse::from_record(buf, offset)),
                        ReplyResult::Err(e) => Err(e),
                    }),
                    _ => unreachable!(),
                },
                Some(slot) => {
                    // Avoid cloning the waker if unchanged since last poll.
                    match slot {
                        CallbackSlot::Waiting(w) if w.will_wake(cx.waker()) => {}
                        _ => *slot = CallbackSlot::Waiting(cx.waker().clone()),
                    }
                    std::task::Poll::Pending
                }
                // Entry gone — connection was poisoned.
                None => std::task::Poll::Ready(Err(RpcError::SystemError)),
            }
        })
        .await
    }

    /// Returns true if the connection has been poisoned (e.g. the remote
    /// peer closed the connection and the read task detected the EOF).
    pub fn is_poisoned(&self) -> bool {
        self.connection_state.lock().unwrap().poisoned
    }

    /// Returns the number of pending (in-flight) requests.
    ///
    /// Lock-free — reads an atomic counter instead of acquiring the
    /// connection state mutex. Used by DynamicPool for load balancing.
    pub fn num_pending_requests(&self) -> usize {
        self.pending_count.load(Ordering::Relaxed)
    }

    /// Decrement the pending-request counter by one (fire-and-forget cleanup).
    #[inline(always)]
    pub(crate) fn decrement_pending(&self) {
        self.pending_count.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Drop guard for cancellation safety in `send_call`.
///
/// If the future is dropped before the response arrives, this removes
/// the pending entry from the HashMap so it doesn't leak.
struct CallGuard<'a> {
    state: &'a StdMutex<ConnectionState>,
    pending_count: &'a AtomicUsize,
    xid: u32,
    done: bool,
}

impl CallGuard<'_> {
    fn cleanup(&mut self) {
        if !self.done {
            self.done = true;
            self.pending_count.fetch_sub(1, Ordering::Relaxed);
            self.state
                .lock()
                .unwrap()
                .pending_requests
                .remove(&self.xid);
        }
    }

    fn complete(mut self) {
        self.cleanup();
    }
}

impl Drop for CallGuard<'_> {
    fn drop(&mut self) {
        self.cleanup();
    }
}

/// Drop guard for cancellation safety in `send_fire_and_forget`.
///
/// If the future is dropped before the message is sent (e.g. due to a
/// timeout), this removes the pending entry and decrements the counter.
/// Unlike `CallGuard`, this only guards the send phase — once sent, the
/// reader task handles cleanup via `route_reply`.
struct FireAndForgetGuard<'a> {
    state: &'a StdMutex<ConnectionState>,
    pending_count: &'a AtomicUsize,
    xid: u32,
    sent: bool,
}

impl Drop for FireAndForgetGuard<'_> {
    fn drop(&mut self) {
        if !self.sent {
            // Only decrement pending_count if we actually removed the entry.
            // poison() may have already drained it — without this check, both
            // paths would decrement, wrapping pending_count to usize::MAX.
            let removed = self
                .state
                .lock()
                .unwrap()
                .pending_requests
                .remove(&self.xid);
            if removed.is_some() {
                self.pending_count.fetch_sub(1, Ordering::Relaxed);
            }
        }
    }
}

/// RAII wrapper for a client connection created by [`connect_client`](crate::connect_client).
///
/// Automatically shuts down the connection (poisoning pending requests
/// and aborting background tasks) when dropped. For pool implementations
/// that manage their own lifecycle, call [`into_handle`](Self::into_handle)
/// to take the raw handle and disable automatic shutdown.
pub struct ClientConnection {
    handle: Option<Arc<ConnectionHandle>>,
}

impl ClientConnection {
    pub(crate) fn new(handle: Arc<ConnectionHandle>) -> Self {
        Self {
            handle: Some(handle),
        }
    }

    /// Access the underlying connection handle.
    ///
    /// Use this to clone the `Arc` for passing to generated RPC client
    /// wrappers or spawned tasks:
    ///
    /// ```ignore
    /// let conn = connect_client(&addr).await?;
    /// let rpc_client = MyService::new(conn.handle().clone());
    /// ```
    ///
    /// Note: `conn` must remain alive for as long as the extracted handle
    /// is used. To fully transfer ownership (disconnecting RAII shutdown),
    /// use [`into_handle`](Self::into_handle) instead.
    pub fn handle(&self) -> &Arc<ConnectionHandle> {
        self.handle.as_ref().unwrap()
    }

    /// Consume this wrapper, returning the raw handle without triggering
    /// automatic shutdown.
    ///
    /// The caller becomes responsible for calling [`ConnectionHandle::poison`]
    /// when done. Used by pool implementations that manage connection
    /// lifecycle explicitly.
    pub fn into_handle(mut self) -> Arc<ConnectionHandle> {
        self.handle.take().unwrap()
    }

    /// Explicitly shut down the connection, awaiting task completion.
    ///
    /// Unlike `Drop`, which initiates shutdown without waiting, this method
    /// waits for the connection's background tasks to stop.
    pub async fn shutdown(mut self) {
        if let Some(handle) = self.handle.take() {
            handle.poison().await;
        }
    }
}

impl Drop for ClientConnection {
    fn drop(&mut self) {
        if let Some(handle) = &self.handle {
            let _ = handle.begin_poison();
        }
    }
}

impl std::ops::Deref for ClientConnection {
    type Target = ConnectionHandle;
    fn deref(&self) -> &ConnectionHandle {
        // Safe: Option is None only after into_handle()/shutdown() consume self
        self.handle.as_ref().unwrap()
    }
}

impl std::fmt::Debug for ClientConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientConnection")
            .field("handle", &self.handle)
            .finish()
    }
}

#[async_trait]
impl RpcSender for ClientConnection {
    fn call_buf(&self) -> CallBuf {
        (**self).call_buf()
    }

    async fn send_call(
        &self,
        progvers: ProgVers,
        proc: u32,
        call: CallBuf,
    ) -> Result<RpcResponse, RpcError> {
        (**self).send_call(progvers, proc, call).await
    }

    async fn send_fire_and_forget(
        &self,
        progvers: ProgVers,
        proc: u32,
        call: CallBuf,
    ) -> Result<(), RpcError> {
        (**self).send_fire_and_forget(progvers, proc, call).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpc_error::DEFAULT_OPAQUE_AUTH;

    fn call_with(payload: &[u8]) -> CallBuf {
        let mut call = CallBuf::new();
        call.extend_from_slice(payload);
        call
    }

    #[test]
    fn frame_call_matches_xdr_encoded_header() {
        let progvers = ProgVers::new(0x0102_0304, 7);
        let payload = b"payload bytes";
        let header = rpc_msg::rpc_msg {
            xid: 0xDEAD_BEEF,
            body: rpc_msg::rpc_msg_body::CALL {
                cbody: rpc_msg::call_body {
                    rpcvers: 2,
                    prog: progvers.prog(),
                    vers: progvers.vers(),
                    proc: 9,
                    cred: DEFAULT_OPAQUE_AUTH,
                    verf: DEFAULT_OPAQUE_AUTH,
                },
            },
        };
        let expected = frame_rpc_record(Vec::new(), &header, payload).unwrap();

        let framed = frame_call(call_with(payload), 0xDEAD_BEEF, progvers, 9).unwrap();

        assert_eq!(framed, expected);
        assert_eq!(framed.len(), CallBuf::PREFIX_LEN + payload.len());
    }

    #[test]
    fn frame_call_empty_payload() {
        let framed = frame_call(CallBuf::new(), 1, ProgVers::new(1, 1), 0).unwrap();
        assert_eq!(framed.len(), CallBuf::PREFIX_LEN);
        assert_eq!(&framed[..4], &(0x8000_0000u32 | 40).to_be_bytes());
    }

    #[test]
    fn buf_pool_is_bounded() {
        let pool = BufPool::new();
        let mut batch: Vec<Vec<u8>> = (0..MAX_POOLED_BUFFERS + 10).map(|_| vec![1]).collect();
        batch.push(Vec::with_capacity(MAX_POOLED_CAPACITY + 1));
        pool.put_batch(&mut batch);
        assert!(batch.is_empty());
        let pooled = pool.0.lock().unwrap();
        assert_eq!(pooled.len(), MAX_POOLED_BUFFERS);
        assert!(pooled.iter().all(|b| b.capacity() <= MAX_POOLED_CAPACITY));
        drop(pooled);
        assert!(pool.take().is_empty());
    }

    #[test]
    fn call_buf_reuses_written_records() {
        let pool = BufPool::new();
        let record = frame_call(call_with(&[9; 300]), 1, ProgVers::new(1, 1), 0).unwrap();
        let ptr = record.as_ptr();
        pool.put_batch(&mut vec![record]);

        let call = pool.take_call_buf();
        assert!(call.payload().is_empty());
        let reused = call.into_vec();
        assert_eq!(reused.as_ptr(), ptr);
        // The pool is empty again; the next buffer is fresh.
        assert!(pool.take_call_buf().into_vec().capacity() < 300);
    }

    #[test]
    fn test_callback_roundtrip() {
        let mut state = ConnectionState::new();

        let xid = state.register_call().unwrap();
        assert_eq!(xid, 1);

        // Reply for unknown xid — should be ignored.
        state.handle_reply(0, ReplyResult::Ok(b"world".to_vec(), 0));

        // Reply for our xid.
        state.handle_reply(xid, ReplyResult::Ok(b"hello".to_vec(), 0));

        // Slot should now be Ready.
        match state.pending_requests.remove(&xid) {
            Some(CallbackSlot::Ready(ReplyResult::Ok(data, offset))) => {
                assert_eq!(&data[offset..], b"hello");
            }
            other => panic!("expected Ready(Ok), got {other:?}"),
        }

        // Duplicate reply — entry already removed, should be ignored.
        state.handle_reply(xid, ReplyResult::Ok(b"foo".to_vec(), 0));
    }

    #[test]
    fn test_call_guard_completion_cleans_registered_entry() {
        let state = StdMutex::new(ConnectionState::new());
        let xid = state.lock().unwrap().register_call().unwrap();
        let pending_count = AtomicUsize::new(1);

        CallGuard {
            state: &state,
            pending_count: &pending_count,
            xid,
            done: false,
        }
        .complete();

        assert_eq!(pending_count.load(Ordering::Relaxed), 0);
        assert!(state.lock().unwrap().pending_requests.is_empty());
    }

    #[test]
    fn test_poison_wakes_waiters() {
        let mut state = ConnectionState::new();
        let xid = state.register_call().unwrap();

        // Simulate a waiting caller by inserting a waker.
        let waker = std::task::Waker::noop().clone();
        *state.pending_requests.get_mut(&xid).unwrap() = CallbackSlot::Waiting(waker);

        state.poison();
        assert!(state.pending_requests.is_empty());
        assert!(state.poisoned);

        // New registrations should fail.
        assert!(state.register_call().is_err());
    }

    #[test]
    fn test_register_fire_and_forget() {
        let mut state = ConnectionState::new();

        let xid = state.register_fire_and_forget().unwrap();
        assert_eq!(xid, 1);

        // Slot should be FireAndForget.
        match state.pending_requests.get(&xid) {
            Some(CallbackSlot::FireAndForget) => {}
            other => panic!("expected FireAndForget, got {other:?}"),
        }

        // Poisoned state should reject registration.
        state.poisoned = true;
        assert!(state.register_fire_and_forget().is_err());
    }

    #[test]
    fn test_handle_reply_fire_and_forget() {
        let mut state = ConnectionState::new();

        // Fire-and-forget call: handle_reply should remove the entry and return true.
        let faf_xid = state.register_fire_and_forget().unwrap();
        let was_faf = state.handle_reply(faf_xid, ReplyResult::Ok(b"ignored".to_vec(), 0));
        assert!(was_faf, "expected true for fire-and-forget reply");
        assert!(
            !state.pending_requests.contains_key(&faf_xid),
            "fire-and-forget entry should be removed"
        );

        // Normal call: handle_reply should keep the entry as Ready and return false.
        let normal_xid = state.register_call().unwrap();
        let was_faf = state.handle_reply(normal_xid, ReplyResult::Ok(b"hello".to_vec(), 0));
        assert!(!was_faf, "expected false for normal reply");
        match state.pending_requests.get(&normal_xid) {
            Some(CallbackSlot::Ready(_)) => {}
            other => panic!("expected Ready, got {other:?}"),
        }

        // Unknown xid: should return false.
        let was_faf = state.handle_reply(9999, ReplyResult::Ok(b"unknown".to_vec(), 0));
        assert!(!was_faf, "expected false for unknown xid");
    }

    #[test]
    fn test_poison_counts_fire_and_forget() {
        let mut state = ConnectionState::new();

        // Register a mix of normal and fire-and-forget calls.
        let _normal_xid = state.register_call().unwrap();
        let _faf_xid1 = state.register_fire_and_forget().unwrap();
        let _faf_xid2 = state.register_fire_and_forget().unwrap();

        // Add a Waiting entry (simulating a polled normal call).
        let waker = std::task::Waker::noop().clone();
        *state.pending_requests.get_mut(&_normal_xid).unwrap() = CallbackSlot::Waiting(waker);

        let faf_count = state.poison();
        assert_eq!(faf_count, 2, "expected 2 fire-and-forget entries");
        assert!(state.pending_requests.is_empty());
    }
}
