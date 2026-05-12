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

use crate::{
    ProgVers,
    rpc_error::{DEFAULT_OPAQUE_AUTH, RpcError},
    rpc_msg,
    tcp_connection::WriteHalf,
};

const MAX_BATCH: usize = 64;

/// XDR header is ~40 bytes (xid + msg_type + rpcvers + prog + vers + proc + 2x opaque_auth).
const XDR_HEADER_ESTIMATE: usize = 48;

// ---------------------------------------------------------------------------
// Buffer pool — recycles Vec<u8> allocations across RPC messages
// ---------------------------------------------------------------------------

/// Pool of reusable `Vec<u8>` buffers shared between callers and the writer
/// task. After warmup, steady-state operation allocates nothing.
pub(crate) struct BufPool(StdMutex<Vec<Vec<u8>>>);

impl std::fmt::Debug for BufPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let len = self.0.lock().map_or(0, |v| v.len());
        f.debug_tuple("BufPool").field(&len).finish()
    }
}

impl BufPool {
    fn new() -> Self {
        Self(StdMutex::new(Vec::new()))
    }

    /// Take a cleared buffer from the pool, or allocate a new one.
    pub(crate) fn take(&self) -> Vec<u8> {
        self.0.lock().unwrap().pop().unwrap_or_default()
    }

    /// Return a spent buffer to the pool for reuse.
    pub(crate) fn put(&self, mut buf: Vec<u8>) {
        buf.clear();
        self.0.lock().unwrap().push(buf);
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
        for buf in batch.drain(..) {
            pool.put(buf);
        }

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
/// allocation after warmup when fed from a [`BufPool`]).
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

/// Like [`frame_rpc_record`], but takes ownership of the payload Vec and
/// reuses its allocation for the output. Shifts the payload right to make
/// room for the record-length + XDR header prefix.
///
/// Saves one allocation vs. the borrowed path when the caller already owns
/// the data (e.g., pool workers that received it over a channel).
#[inline]
pub(crate) fn frame_rpc_record_owned(
    header: &rpc_msg::rpc_msg,
    mut data: Vec<u8>,
) -> Result<Vec<u8>, ()> {
    // Serialize the XDR header into a small temp buffer.
    let mut hdr_writer = xdrs::txs::buf_writer::BufWriter::with_capacity(XDR_HEADER_ESTIMATE);
    xdrs::write_xdr(&0u32, &mut hdr_writer).map_err(drop)?;
    xdrs::write_xdr(header, &mut hdr_writer).map_err(drop)?;

    let hdr_bytes: &[u8] = hdr_writer.as_ref();
    let hdr_xdr_len = hdr_bytes.len() - 4; // minus the placeholder
    let total_len = hdr_xdr_len
        .checked_add(data.len())
        .and_then(|sum| u32::try_from(sum).ok())
        .filter(|&len| len <= 0x7FFF_FFFF)
        .ok_or(())?;

    let msg_len: u32 = total_len | 0x8000_0000;
    let hdr_size = hdr_bytes.len();
    let data_len = data.len();

    // Shift payload right to make room for header.
    data.reserve(hdr_size);
    data.resize(data_len + hdr_size, 0);
    data.copy_within(..data_len, hdr_size);

    // Write record-length + XDR header at the front.
    data[..4].copy_from_slice(&msg_len.to_be_bytes());
    data[4..hdr_size].copy_from_slice(&hdr_bytes[4..]);

    Ok(data)
}

/// Response from an RPC call. Wraps the raw record buffer and payload
/// offset, providing zero-copy access to the payload via `Deref<Target = [u8]>`.
///
/// The caller deserializes directly from `&*response` without any copy.
#[derive(Clone)]
pub struct RpcResponse {
    buf: Vec<u8>,
    offset: usize,
}

impl RpcResponse {
    /// Create a response from a payload Vec with no header (offset 0).
    pub fn from_payload(payload: Vec<u8>) -> Self {
        Self {
            buf: payload,
            offset: 0,
        }
    }

    pub(crate) fn from_record(buf: Vec<u8>, offset: usize) -> Self {
        Self { buf, offset }
    }
}

impl std::ops::Deref for RpcResponse {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        &self.buf[self.offset..]
    }
}

impl AsRef<[u8]> for RpcResponse {
    fn as_ref(&self) -> &[u8] {
        self
    }
}

impl PartialEq<[u8]> for RpcResponse {
    fn eq(&self, other: &[u8]) -> bool {
        &**self == other
    }
}

impl<const N: usize> PartialEq<[u8; N]> for RpcResponse {
    fn eq(&self, other: &[u8; N]) -> bool {
        &**self == other
    }
}

impl<const N: usize> PartialEq<&[u8; N]> for RpcResponse {
    fn eq(&self, other: &&[u8; N]) -> bool {
        &**self == *other
    }
}

impl PartialEq<&[u8]> for RpcResponse {
    fn eq(&self, other: &&[u8]) -> bool {
        &**self == *other
    }
}

impl PartialEq<Vec<u8>> for RpcResponse {
    fn eq(&self, other: &Vec<u8>) -> bool {
        &**self == other.as_slice()
    }
}

impl PartialEq for RpcResponse {
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}

impl Eq for RpcResponse {}

impl std::fmt::Debug for RpcResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RpcResponse")
            .field("len", &self.len())
            .finish()
    }
}

/// Trait for making outgoing RPC calls.
///
/// Implemented by [`ConnectionHandle`] for real TCP connections,
/// and by [`FixedSerialPool`](crate::FixedSerialPool), [`DynamicPool`](crate::DynamicPool),
/// and [`CpuSetPool`](crate::CpuSetPool) for pooled connections.
///
/// Inside an [`RpcHandler`](crate::handler::RpcHandler) dispatch, the
/// per-connection sender is available via [`peer()`](crate::peer).
///
/// Implement on a mock struct for unit testing without TCP.
#[async_trait]
pub trait RpcSender: Send + Sync {
    /// Send an RPC call and return the response.
    ///
    /// The returned [`RpcResponse`] derefs to `&[u8]` for zero-copy
    /// deserialization, or can be copied to an owned `Vec<u8>` via
    /// `.to_vec()`.
    async fn send_call(
        &self,
        progvers: ProgVers,
        proc: u32,
        data: &[u8],
    ) -> Result<RpcResponse, RpcError>;

    /// Send an RPC call without waiting for the response.
    ///
    /// Default implementation calls [`send_call`](Self::send_call) and
    /// discards the response. Override for optimal performance.
    async fn send_fire_and_forget(
        &self,
        progvers: ProgVers,
        proc: u32,
        data: &[u8],
    ) -> Result<(), RpcError> {
        self.send_call(progvers, proc, data).await.map(|_| ())
    }
}

#[async_trait]
impl RpcSender for ConnectionHandle {
    async fn send_call(
        &self,
        progvers: ProgVers,
        proc: u32,
        data: &[u8],
    ) -> Result<RpcResponse, RpcError> {
        ConnectionHandle::send_call(self, progvers, proc, data).await
    }

    async fn send_fire_and_forget(
        &self,
        progvers: ProgVers,
        proc: u32,
        data: &[u8],
    ) -> Result<(), RpcError> {
        ConnectionHandle::send_fire_and_forget(self, progvers, proc, data).await
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
/// and graceful shutdown via poisoning.
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
    writer_handle: StdMutex<Option<JoinHandle<()>>>,
    client_read_runner: StdMutex<Option<crate::tasks::ClientReadRunner>>,
}

impl ConnectionHandle {
    /// Create a new connection handle wrapping the given TCP write half.
    pub fn new(
        write_half: WriteHalf,
        config: &crate::ConnectionConfig,
        span: tracing::Span,
    ) -> Self {
        let pool = Arc::new(BufPool::new());
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

    fn prepare_call_msg(
        buf: Vec<u8>,
        xid: u32,
        progvers: ProgVers,
        proc: u32,
        data: &[u8],
    ) -> Result<Vec<u8>, RpcError> {
        let header = rpc_msg::rpc_msg {
            xid,
            body: rpc_msg::rpc_msg_body::CALL {
                cbody: rpc_msg::call_body {
                    rpcvers: 2,
                    prog: progvers.prog,
                    vers: progvers.vers,
                    proc,
                    cred: DEFAULT_OPAQUE_AUTH,
                    verf: DEFAULT_OPAQUE_AUTH,
                },
            },
        };
        frame_rpc_record(buf, &header, data).map_err(|()| {
            tracing::warn!(data_len = data.len(), "message too large");
            RpcError::SystemError
        })
    }

    /// Send an RPC call and wait for the response.
    ///
    /// Returns the response data on success, or an error if the call fails.
    pub async fn send_call(
        &self,
        progvers: ProgVers,
        proc: u32,
        data: &[u8],
    ) -> Result<RpcResponse, RpcError> {
        let xid = self.connection_state.lock().unwrap().register_call()?;
        self.pending_count.fetch_add(1, Ordering::Relaxed);

        let span = tracing::debug_span!(
            parent: &self.connection_span,
            "rpc_call",
            xid,
            prog = progvers.prog,
            vers = progvers.vers,
            proc = proc,
        );

        // Guard removes the pending entry on drop (cancellation safety).
        let guard = CallGuard {
            state: &self.connection_state,
            pending_count: &self.pending_count,
            xid,
            done: false,
        };

        let msg = match Self::prepare_call_msg(self.buf_pool.take(), xid, progvers, proc, data) {
            Ok(msg) => msg,
            Err(e) => {
                // guard.drop() will clean up pending_requests
                return Err(e);
            }
        };

        let result = async {
            if self.write_tx.send(msg).await.is_err() {
                return Err(RpcError::SystemError);
            }

            self.await_reply(xid).await
        }
        .instrument(span)
        .await;

        guard.disarm();
        result
    }

    /// Send an RPC call without waiting for the response.
    ///
    /// Registers an XID for tracing, sends the message, and returns
    /// immediately. The reader task will silently discard the reply when
    /// it arrives.
    pub async fn send_fire_and_forget(
        &self,
        progvers: ProgVers,
        proc: u32,
        data: &[u8],
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
            prog = progvers.prog,
            vers = progvers.vers,
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

        let msg = Self::prepare_call_msg(self.buf_pool.take(), xid, progvers, proc, data)?;

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

    /// Like [`send_fire_and_forget`](Self::send_fire_and_forget), but takes
    /// ownership of the data Vec. Reuses its allocation for the framed
    /// message, saving one copy when the caller already owns the data
    /// (e.g., pool workers).
    pub async fn send_fire_and_forget_owned(
        &self,
        progvers: ProgVers,
        proc: u32,
        data: Vec<u8>,
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
            prog = progvers.prog,
            vers = progvers.vers,
            proc = proc,
            fire_and_forget = true,
        );

        let mut guard = FireAndForgetGuard {
            state: &self.connection_state,
            pending_count: &self.pending_count,
            xid,
            sent: false,
        };

        let header = rpc_msg::rpc_msg {
            xid,
            body: rpc_msg::rpc_msg_body::CALL {
                cbody: rpc_msg::call_body {
                    rpcvers: 2,
                    prog: progvers.prog,
                    vers: progvers.vers,
                    proc,
                    cred: DEFAULT_OPAQUE_AUTH,
                    verf: DEFAULT_OPAQUE_AUTH,
                },
            },
        };

        let msg = frame_rpc_record_owned(&header, data).map_err(|()| {
            tracing::warn!("message too large: exceeds max (0x7FFF_FFFF)");
            RpcError::SystemError
        })?;

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

    /// Like [`send_call`](Self::send_call), but takes ownership of the data
    /// Vec. Reuses its allocation for the framed message by shifting the
    /// payload and prepending the header. Saves one copy when the caller
    /// already owns the data (e.g., pool workers).
    pub async fn send_call_owned(
        &self,
        progvers: ProgVers,
        proc: u32,
        data: Vec<u8>,
    ) -> Result<RpcResponse, RpcError> {
        let xid = self.connection_state.lock().unwrap().register_call()?;
        self.pending_count.fetch_add(1, Ordering::Relaxed);

        let span = tracing::debug_span!(
            parent: &self.connection_span,
            "rpc_call",
            xid,
            prog = progvers.prog,
            vers = progvers.vers,
            proc = proc,
        );

        let guard = CallGuard {
            state: &self.connection_state,
            pending_count: &self.pending_count,
            xid,
            done: false,
        };

        let header = rpc_msg::rpc_msg {
            xid,
            body: rpc_msg::rpc_msg_body::CALL {
                cbody: rpc_msg::call_body {
                    rpcvers: 2,
                    prog: progvers.prog,
                    vers: progvers.vers,
                    proc,
                    cred: DEFAULT_OPAQUE_AUTH,
                    verf: DEFAULT_OPAQUE_AUTH,
                },
            },
        };

        let msg = match frame_rpc_record_owned(&header, data) {
            Ok(msg) => msg,
            Err(()) => {
                tracing::warn!("message too large: exceeds max (0x7FFF_FFFF)");
                return Err(RpcError::SystemError);
            }
        };

        let result = async {
            if self.write_tx.send(msg).await.is_err() {
                return Err(RpcError::SystemError);
            }

            self.await_reply(xid).await
        }
        .instrument(span)
        .await;

        guard.disarm();
        result
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
    fn disarm(mut self) {
        self.done = true;
        self.pending_count.fetch_sub(1, Ordering::Relaxed);
    }
}

impl Drop for CallGuard<'_> {
    fn drop(&mut self) {
        if !self.done {
            self.pending_count.fetch_sub(1, Ordering::Relaxed);
            self.state
                .lock()
                .unwrap()
                .pending_requests
                .remove(&self.xid);
        }
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
    /// This is the async counterpart to the synchronous shutdown that
    /// `Drop` performs. Prefer this when you can `.await`.
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
    async fn send_call(
        &self,
        progvers: ProgVers,
        proc: u32,
        data: &[u8],
    ) -> Result<RpcResponse, RpcError> {
        (**self).send_call(progvers, proc, data).await
    }

    async fn send_fire_and_forget(
        &self,
        progvers: ProgVers,
        proc: u32,
        data: &[u8],
    ) -> Result<(), RpcError> {
        (**self).send_fire_and_forget(progvers, proc, data).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            state.pending_requests.get(&faf_xid).is_none(),
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
