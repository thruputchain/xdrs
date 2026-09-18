//! Connection state and RPC call handling.

use std::collections::{HashMap, VecDeque};
use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::task::Waker;
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

// ---------------------------------------------------------------------------
// Writer — records go to the socket from the sending task itself
// ---------------------------------------------------------------------------

/// Result of a [`Writer`] drain pass.
enum Drain {
    /// Everything queued so far is on the wire; the pass released the socket.
    Done,
    /// The socket is full; the pass still owns the socket and parked what is
    /// left in `WriteState::batch` for a flusher task to finish.
    WouldBlock,
    /// The socket failed or the connection was poisoned.
    Closed,
}

/// Buffered bytes at which the writer flushes regardless of reader phase.
/// Matches x23's `WriteBuffer::kFlushThreshold`.
const FLUSH_BYTES: usize = 1 << 16;

#[derive(Default)]
struct WriteState {
    /// Records accepted since the active drain pass last looked.
    queue: VecDeque<Vec<u8>>,
    /// Records the active drain pass is writing; `batch_off` bytes of the
    /// front record are already on the wire.
    batch: VecDeque<Vec<u8>>,
    batch_off: usize,
    /// Exactly one drain pass owns the socket at a time — either the task
    /// that enqueued a record, running inline, or a flusher task parked on
    /// writable().
    draining: bool,
    /// The connection's reader is working through records it already has
    /// buffered and will flush when it runs out. Calls enqueued now ride
    /// along in that flush instead of each taking a `writev`.
    reader_busy: bool,
    /// The connection's reader is parked waiting for the socket. Nothing
    /// else will flush, so replies go out as soon as they are enqueued.
    reader_waiting: bool,
    closed: bool,
}

/// Per-connection record writer, in the shape of x23's `awrite`/`dwrite`.
///
/// Records are written from the task that produces them, with
/// `try_write_vectored` on that task's own thread: no writer task is woken
/// and no worker is unparked. Only when the socket returns `WouldBlock` does
/// a flusher task exist, and it exits once the backlog is on the wire. While
/// one task drains, others just enqueue; the drainer picks their records up
/// before it lets go of the socket.
///
/// The connection's reader reports its phase so writes coalesce the way x23's
/// do. While it is busy with records it has already buffered, calls are
/// queued rather than written, because the reader flushes everything queued
/// before it waits for the socket again (a burst of replies wakes a burst of
/// callers whose next calls then leave in one `writev`). Replies are queued
/// unless the reader is already waiting: a reply is produced by the reader or
/// by a handler task it spawned, so it is either flushed at the end of the
/// reader's pass or written immediately because nothing else would. A call
/// is never delayed while the reader is neither busy nor waiting (a serial
/// handler is running), so a handler's calls to its peer cannot deadlock,
/// and a serial reader flushes before it blocks on a handler.
///
/// Nothing ever waits on the queue. The backlog only grows while the peer is
/// not reading, and that is answered by failing work rather than parking
/// it: past `soft_limit` unsent bytes a new call is rejected
/// ([`SendError::Overloaded`]) and the read loop sheds new inbound calls
/// with SYSTEM_ERR instead of dispatching them, while replies to calls that
/// were already admitted are still accepted. Past `hard_limit` the
/// connection fails closed.
pub(crate) struct Writer {
    sock: tokio::net::tcp::OwnedWriteHalf,
    state: StdMutex<WriteState>,
    /// Bytes of records accepted and not yet completely written, across
    /// `queue` and `batch`. Only ever changed while `state` is locked, so
    /// reads under the lock are exact; the read loop's admission check reads
    /// it without the lock (the limit is advisory, a stale read is fine)
    /// rather than contend with every producer on the connection.
    pending_bytes: AtomicUsize,
    /// Unsent bytes past which new calls are rejected and inbound calls shed.
    soft_limit: usize,
    /// Unsent bytes past which the connection fails closed.
    hard_limit: usize,
    pool: Arc<BufPool>,
}

impl std::fmt::Debug for Writer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.state.lock().unwrap();
        f.debug_struct("Writer")
            .field("queued", &(state.queue.len() + state.batch.len()))
            .field("pending_bytes", &self.pending_bytes.load(Ordering::Relaxed))
            .field("draining", &state.draining)
            .field("closed", &state.closed)
            .finish()
    }
}

/// Why the writer refused a call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SendError {
    /// The writer is closed.
    Closed,
    /// More than `soft_limit` bytes are waiting for the peer to read.
    Overloaded,
}

/// Which kind of record is being enqueued; decides when it is flushed.
#[derive(Clone, Copy)]
enum Kind {
    Call,
    Reply,
}

impl Writer {
    /// `hard_limit` is raised to `soft_limit` and to one maximum-size record
    /// (`max_message_size` plus its record mark), so the memory bound can
    /// never refuse a call the protocol allows when nothing else is queued.
    fn new(
        sock: tokio::net::tcp::OwnedWriteHalf,
        soft_limit: usize,
        hard_limit: usize,
        max_message_size: u32,
        pool: Arc<BufPool>,
    ) -> Self {
        Self {
            sock,
            state: StdMutex::new(WriteState::default()),
            pending_bytes: AtomicUsize::new(0),
            soft_limit,
            hard_limit: hard_limit
                .max(soft_limit)
                .max(max_message_size as usize + 4),
            pool,
        }
    }

    /// Queue an outgoing CALL, or refuse it if the peer has fallen more than
    /// `soft_limit` bytes behind or the call would push the backlog past
    /// `hard_limit`. Never waits, and never fails the connection: refusing
    /// the call is enough to hold the bound.
    pub(crate) fn send_call(self: &Arc<Self>, msg: Vec<u8>) -> Result<(), SendError> {
        let flush = {
            let mut state = self.state.lock().unwrap();
            if state.closed {
                return Err(SendError::Closed);
            }
            let pending = self.pending_bytes.load(Ordering::Relaxed);
            if pending >= self.soft_limit || pending.saturating_add(msg.len()) > self.hard_limit {
                drop(state);
                // Whatever is queued should be on its way regardless.
                self.kick();
                return Err(SendError::Overloaded);
            }
            self.push(&mut state, msg, Kind::Call)
        };
        if flush {
            self.kick();
        }
        Ok(())
    }

    /// Queue an outgoing REPLY. Replies answer calls already admitted, so
    /// they are accepted past `soft_limit`; past `hard_limit` the connection
    /// fails closed instead. Never waits.
    pub(crate) fn send_reply(self: &Arc<Self>, msg: Vec<u8>) -> io::Result<()> {
        let flush = {
            let mut state = self.state.lock().unwrap();
            if state.closed {
                return Err(closed_error());
            }
            let pending = self.pending_bytes.load(Ordering::Relaxed);
            if pending.saturating_add(msg.len()) > self.hard_limit {
                drop(state);
                tracing::error!(
                    pending_bytes = pending,
                    hard_limit = self.hard_limit,
                    "peer is not reading; closing connection"
                );
                self.close();
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "outbound buffer over hard limit",
                ));
            }
            self.push(&mut state, msg, Kind::Reply)
        };
        if flush {
            self.kick();
        }
        Ok(())
    }

    /// More than `soft_limit` bytes are waiting for the peer to read. The
    /// read loop sheds new inbound calls while this holds. Lock-free.
    pub(crate) fn overloaded(&self) -> bool {
        self.pending_bytes.load(Ordering::Relaxed) >= self.soft_limit
    }

    /// Append `msg` to the queue; returns whether the caller should flush now.
    fn push(&self, state: &mut WriteState, msg: Vec<u8>, kind: Kind) -> bool {
        let pending = self.pending_bytes.fetch_add(msg.len(), Ordering::Relaxed) + msg.len();
        state.queue.push_back(msg);
        state.queue.len() >= MAX_BATCH
            || pending >= FLUSH_BYTES
            || match kind {
                Kind::Call => !state.reader_busy,
                Kind::Reply => state.reader_waiting,
            }
    }

    /// The reader has records buffered and is about to work through them
    /// without waiting on the socket.
    pub(crate) fn reader_busy(&self) {
        let mut state = self.state.lock().unwrap();
        state.reader_busy = true;
        state.reader_waiting = false;
    }

    /// The reader is about to await something other than the socket — a
    /// serial handler — for an unbounded time. Flushes everything queued so
    /// far: replies already produced must not wait behind that handler.
    pub(crate) fn reader_yield(self: &Arc<Self>) {
        {
            let mut state = self.state.lock().unwrap();
            state.reader_busy = false;
            state.reader_waiting = false;
        }
        self.kick();
    }

    /// The reader has nothing buffered and is about to wait for the socket.
    /// Flushes everything queued so far.
    pub(crate) fn reader_waiting(self: &Arc<Self>) {
        {
            let mut state = self.state.lock().unwrap();
            state.reader_busy = false;
            state.reader_waiting = true;
        }
        self.kick();
    }

    /// Start a drain pass on the calling thread unless one already owns the
    /// socket; that pass will find the new records itself.
    fn kick(self: &Arc<Self>) {
        {
            let mut state = self.state.lock().unwrap();
            if state.closed || state.draining || state.queue.is_empty() {
                return;
            }
            state.draining = true;
        }
        if let Drain::WouldBlock = self.drain_pass() {
            self.spawn_flusher();
        }
    }

    fn spawn_flusher(self: &Arc<Self>) {
        let writer = self.clone();
        tokio::spawn(async move {
            loop {
                if let Err(e) = writer.sock.writable().await {
                    writer.fail(&e);
                    return;
                }
                match writer.drain_pass() {
                    Drain::WouldBlock => {}
                    Drain::Done | Drain::Closed => return,
                }
            }
        });
    }

    /// Write until the queue is empty or the socket is full. The caller must
    /// own the socket (`draining` is set); on `Done` this releases it.
    fn drain_pass(&self) -> Drain {
        let mut spent: Vec<Vec<u8>> = Vec::new();
        let outcome = loop {
            let (mut batch, mut off) = {
                let mut state = self.state.lock().unwrap();
                if state.closed {
                    break Drain::Closed;
                }
                if state.batch.is_empty() {
                    if state.queue.is_empty() {
                        state.draining = false;
                        break Drain::Done;
                    }
                    let WriteState { queue, batch, .. } = &mut *state;
                    std::mem::swap(batch, queue);
                    state.batch_off = 0;
                }
                (std::mem::take(&mut state.batch), state.batch_off)
            };

            let result = loop {
                let mut slices = [io::IoSlice::new(&[]); MAX_BATCH];
                let mut n = 0;
                for (slot, record) in slices.iter_mut().zip(&batch) {
                    let skip = if n == 0 { off } else { 0 };
                    *slot = io::IoSlice::new(&record[skip..]);
                    n += 1;
                }
                match self.sock.try_write_vectored(&slices[..n]) {
                    Ok(0) => {
                        break Err(io::Error::new(io::ErrorKind::WriteZero, "write returned 0"));
                    }
                    Ok(mut written) => {
                        while written > 0 {
                            let remaining = batch[0].len() - off;
                            if written < remaining {
                                off += written;
                                break;
                            }
                            written -= remaining;
                            off = 0;
                            spent.push(batch.pop_front().unwrap());
                        }
                        if batch.is_empty() {
                            break Ok(());
                        }
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        let mut state = self.state.lock().unwrap();
                        if state.closed {
                            break Ok(());
                        }
                        state.batch = batch;
                        state.batch_off = off;
                        drop(state);
                        self.recycle(&mut spent);
                        return Drain::WouldBlock;
                    }
                    Err(e) => break Err(e),
                }
            };
            if let Err(e) = result {
                self.fail(&e);
                break Drain::Closed;
            }
            self.recycle(&mut spent);
        };
        self.recycle(&mut spent);
        outcome
    }

    /// Account for records now fully on the wire and return their buffers.
    fn recycle(&self, spent: &mut Vec<Vec<u8>>) {
        if spent.is_empty() {
            return;
        }
        let bytes: usize = spent.iter().map(Vec::len).sum();
        {
            // close() zeroes the count under this lock; records written
            // before a failing pass ended must not be subtracted after that.
            let state = self.state.lock().unwrap();
            if !state.closed {
                self.pending_bytes.fetch_sub(bytes, Ordering::Relaxed);
            }
        }
        self.pool.put_batch(spent);
    }

    fn fail(&self, error: &io::Error) {
        tracing::error!(%error, "connection write failed");
        self.close();
    }

    /// Stop accepting records, drop the backlog and shut the socket's write
    /// side so the peer sees EOF. Idempotent.
    pub(crate) fn close(&self) {
        {
            let mut state = self.state.lock().unwrap();
            if state.closed {
                return;
            }
            state.closed = true;
            state.queue.clear();
            state.batch.clear();
            self.pending_bytes.store(0, Ordering::Relaxed);
        }
        let _ = socket2::SockRef::from(self.sock.as_ref()).shutdown(std::net::Shutdown::Write);
    }

    /// Pretend a flusher owns the socket, so nothing is written until
    /// [`unstall_for_test`](Self::unstall_for_test). Lets tests fill the
    /// queue deterministically.
    #[cfg(test)]
    pub(crate) fn stall_for_test(&self) {
        self.state.lock().unwrap().draining = true;
    }

    #[cfg(test)]
    pub(crate) fn unstall_for_test(self: &Arc<Self>) {
        if let Drain::WouldBlock = self.drain_pass() {
            self.spawn_flusher();
        }
    }

    /// Records queued and not yet handed to the socket.
    #[cfg(test)]
    pub(crate) fn queued_for_test(&self) -> usize {
        let state = self.state.lock().unwrap();
        state.queue.len() + state.batch.len()
    }
}

fn closed_error() -> io::Error {
    io::Error::new(io::ErrorKind::BrokenPipe, "writer closed")
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
    client_read_runner: Option<crate::tasks::ClientReadRunner>,
}

impl ConnectionShutdown {
    pub(crate) fn start(&mut self) {
        if let Some(runner) = self.client_read_runner.as_mut() {
            runner.begin_shutdown();
        }
    }

    pub(crate) async fn finish(mut self) {
        self.start();
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
    pub(crate) writer: Arc<Writer>,
    pub(crate) buf_pool: Arc<BufPool>,
    /// Atomic pending-request counter — avoids locking `connection_state`
    /// for load-balancing queries in DynamicPool.
    pending_count: AtomicUsize,
    // Cold — accessed rarely:
    pub(crate) connection_span: tracing::Span,
    #[cfg_attr(not(feature = "concurrent-dispatch"), allow(dead_code))]
    pub(crate) max_concurrent_dispatches: usize,
    /// Accepted for configuration compatibility; the read loop dispatches
    /// straight out of the read buffer and no longer queues records.
    #[allow(dead_code)]
    pub(crate) read_channel_capacity: usize,
    #[cfg_attr(not(feature = "concurrent-dispatch"), allow(dead_code))]
    pub(crate) inline_ready_dispatch: bool,
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
        let writer = Arc::new(Writer::new(
            write_half.into_inner(),
            config.write_buffer_bytes,
            config.write_buffer_limit_bytes,
            config.max_message_size,
            pool.clone(),
        ));
        Self {
            connection_state: StdMutex::new(ConnectionState::new()),
            writer,
            buf_pool: pool,
            pending_count: AtomicUsize::new(0),
            connection_span: span,
            client_read_runner: StdMutex::new(None),
            max_concurrent_dispatches: config.max_concurrent_dispatches,
            read_channel_capacity: config.read_channel_capacity,
            inline_ready_dispatch: config.inline_ready_dispatch,
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

        self.writer.close();
        let mut shutdown = ConnectionShutdown {
            client_read_runner: self.client_read_runner.lock().unwrap().take(),
        };
        shutdown.start();
        shutdown
    }

    /// Poison the connection, failing all pending requests and closing the write half.
    pub async fn poison(&self) {
        self.begin_poison().finish().await;
    }

    /// Send a pre-serialized REPLY record. Never waits; fails the connection
    /// closed if the peer has stopped reading and the backlog is past its
    /// hard limit.
    #[inline(always)]
    pub(crate) fn send_reply(&self, msg: Vec<u8>) -> Result<(), io::Error> {
        self.writer.send_reply(msg)
    }

    /// A handle whose writer is stalled, its own read half, and the peer
    /// socket its records arrive on once the test unstalls the writer.
    #[cfg(test)]
    pub(crate) async fn new_stalled_for_test(
        config: &crate::ConnectionConfig,
    ) -> (Self, crate::tcp_connection::ReadHalf, tokio::net::TcpStream) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (local, peer) = tokio::join!(
            tokio::net::TcpStream::connect(listener.local_addr().unwrap()),
            listener.accept(),
        );
        let (read, write) = local.unwrap().into_split();
        let handle = Self::new(
            crate::tcp_connection::WriteHalf::from_inner(write),
            config,
            tracing::Span::none(),
        );
        handle.writer.stall_for_test();
        (
            handle,
            crate::tcp_connection::ReadHalf::new_for_test(read),
            peer.unwrap().0,
        )
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
    /// call fails. Sending never waits for the peer: if more than the
    /// configured outbound buffer is already waiting for it to read, the
    /// call fails immediately with [`RpcError::SystemError`].
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

        let mut replied = false;
        let result = async {
            if let Err(e) = self.writer.send_call(msg) {
                tracing::debug!(?e, "call refused by the connection writer");
                return Err(RpcError::SystemError);
            }

            let result = self.await_reply(xid).await;
            replied = true;
            result
        }
        .instrument(span)
        .await;

        if replied {
            // await_reply took the entry with it, so skip the lock.
            guard.complete_without_entry();
        } else {
            guard.complete();
        }
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
            if let Err(e) = self.writer.send_call(msg) {
                tracing::debug!(?e, "call refused by the connection writer");
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

    /// Wait for the read loop to deliver a result for the given XID. Either
    /// way — reply or poisoned connection — the pending entry is gone when
    /// this returns.
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

    /// Finish a call whose entry `await_reply` already removed.
    fn complete_without_entry(mut self) {
        debug_assert!(!self.done);
        self.done = true;
        self.pending_count.fetch_sub(1, Ordering::Relaxed);
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

    /// A connection handle over a live loopback socket, its peer's read
    /// half, and the peer's write half (kept alive so the socket stays open).
    async fn live_pair(
        soft_limit: usize,
        hard_limit: usize,
    ) -> (
        Arc<ConnectionHandle>,
        crate::tcp_connection::ReadHalf,
        tokio::net::tcp::OwnedWriteHalf,
    ) {
        // A small message limit so tiny byte limits are not raised past it.
        let config = crate::ConnectionConfig::new(Some(64), crate::ServiceMap::new())
            .with_write_buffer_bytes(soft_limit)
            .with_write_buffer_limit_bytes(hard_limit);
        let (handle, _read, peer) = ConnectionHandle::new_stalled_for_test(&config).await;
        let (peer_read, peer_write) = peer.into_split();
        (
            Arc::new(handle),
            crate::tcp_connection::ReadHalf::new_for_test(peer_read),
            peer_write,
        )
    }

    fn record_with_xid(xid: u32, payload: &[u8]) -> Vec<u8> {
        frame_call(call_with(payload), xid, ProgVers::new(1, 1), 0).unwrap()
    }

    async fn peer_xid(peer: &mut crate::tcp_connection::ReadHalf) -> Option<u32> {
        match tokio::time::timeout(std::time::Duration::from_millis(200), peer.read_record()).await
        {
            Ok(Ok(Some(record))) => Some(u32::from_be_bytes(record[..4].try_into().unwrap())),
            Ok(other) => panic!("unexpected read result: {other:?}"),
            Err(_) => None,
        }
    }

    #[tokio::test]
    async fn replies_wait_for_the_reader_to_go_idle() {
        let (handle, mut peer, _peer_write) = live_pair(1 << 20, 1 << 24).await;
        handle.writer.unstall_for_test();

        // While the reader works through buffered input, replies queue up.
        handle.writer.reader_busy();
        handle.writer.send_reply(record_with_xid(1, b"a")).unwrap();
        handle.writer.send_reply(record_with_xid(2, b"b")).unwrap();
        assert_eq!(handle.writer.queued_for_test(), 2);
        assert_eq!(peer_xid(&mut peer).await, None);

        // Going idle flushes them together, and a reply produced while the
        // reader is idle goes out on its own.
        handle.writer.reader_waiting();
        assert_eq!(peer_xid(&mut peer).await, Some(1));
        assert_eq!(peer_xid(&mut peer).await, Some(2));
        handle.writer.send_reply(record_with_xid(3, b"c")).unwrap();
        assert_eq!(peer_xid(&mut peer).await, Some(3));
    }

    #[tokio::test]
    async fn calls_are_held_only_while_the_reader_is_busy() {
        let (handle, mut peer, _peer_write) = live_pair(1 << 20, 1 << 24).await;
        handle.writer.unstall_for_test();

        handle.writer.reader_busy();
        handle.writer.send_call(record_with_xid(1, b"a")).unwrap();
        assert_eq!(handle.writer.queued_for_test(), 1);
        assert_eq!(peer_xid(&mut peer).await, None);

        // A serial handler is running: calls must not wait for the reader.
        handle.writer.reader_yield();
        handle.writer.send_call(record_with_xid(2, b"b")).unwrap();
        assert_eq!(peer_xid(&mut peer).await, Some(1));
        assert_eq!(peer_xid(&mut peer).await, Some(2));

        // Idle reader: same.
        handle.writer.reader_waiting();
        handle.writer.send_call(record_with_xid(3, b"c")).unwrap();
        assert_eq!(peer_xid(&mut peer).await, Some(3));
    }

    #[tokio::test]
    async fn yielding_to_a_handler_flushes_pending_replies() {
        let (handle, mut peer, _peer_write) = live_pair(1 << 20, 1 << 24).await;
        handle.writer.unstall_for_test();
        handle.writer.reader_busy();
        handle.writer.send_reply(record_with_xid(1, b"a")).unwrap();
        assert_eq!(peer_xid(&mut peer).await, None);
        // About to block on a slow serial handler: the reply already
        // produced must not wait for it.
        handle.writer.reader_yield();
        assert_eq!(peer_xid(&mut peer).await, Some(1));
    }

    #[tokio::test]
    async fn a_large_backlog_flushes_regardless_of_reader_phase() {
        let (handle, mut peer, _peer_write) = live_pair(1 << 20, 1 << 24).await;
        handle.writer.unstall_for_test();
        handle.writer.reader_busy();
        for xid in 0..MAX_BATCH as u32 - 1 {
            handle
                .writer
                .send_reply(record_with_xid(xid, b"x"))
                .unwrap();
        }
        assert_eq!(peer_xid(&mut peer).await, None);
        handle
            .writer
            .send_reply(record_with_xid(MAX_BATCH as u32 - 1, b"x"))
            .unwrap();
        for xid in 0..MAX_BATCH as u32 {
            assert_eq!(peer_xid(&mut peer).await, Some(xid));
        }
    }

    #[tokio::test]
    async fn overloaded_writer_refuses_calls_but_still_takes_replies() {
        // Soft limit of one byte with a stalled socket: the first record
        // puts the writer over the limit and nothing drains.
        let (handle, mut peer, _peer_write) = live_pair(1, 1 << 20).await;
        handle.writer.send_call(record_with_xid(1, b"a")).unwrap();
        assert!(handle.writer.overloaded());
        assert_eq!(
            handle.writer.send_call(record_with_xid(2, b"b")),
            Err(SendError::Overloaded)
        );
        // A reply answers a call the peer already made; it is not shed.
        handle.writer.send_reply(record_with_xid(3, b"c")).unwrap();
        assert_eq!(handle.writer.queued_for_test(), 2);

        handle.writer.unstall_for_test();
        assert_eq!(peer_xid(&mut peer).await, Some(1));
        assert_eq!(peer_xid(&mut peer).await, Some(3));
        assert!(!handle.writer.overloaded());
        handle.writer.send_call(record_with_xid(4, b"d")).unwrap();
        assert_eq!(peer_xid(&mut peer).await, Some(4));
    }

    #[tokio::test]
    async fn a_call_that_would_cross_the_hard_limit_is_refused_not_fatal() {
        // Nothing pending, so under the soft limit, but the record itself is
        // larger than the ceiling: refuse the call, keep the connection.
        let (handle, mut peer, _peer_write) = live_pair(100, 200).await;
        handle.writer.unstall_for_test();
        assert_eq!(
            handle.writer.send_call(record_with_xid(1, &[0; 300])),
            Err(SendError::Overloaded)
        );
        handle.writer.send_call(record_with_xid(2, b"b")).unwrap();
        assert_eq!(peer_xid(&mut peer).await, Some(2));
    }

    #[tokio::test]
    async fn replies_past_the_hard_limit_fail_the_connection_closed() {
        let (handle, mut peer, _peer_write) = live_pair(64, 200).await;
        let big = record_with_xid(1, &[0; 100]);
        handle.writer.send_reply(big.clone()).unwrap();
        assert_eq!(
            handle
                .writer
                .send_reply(record_with_xid(2, &[0; 100]))
                .unwrap_err()
                .kind(),
            io::ErrorKind::WouldBlock
        );
        assert_eq!(
            handle.writer.send_call(record_with_xid(3, b"c")),
            Err(SendError::Closed)
        );
        assert_eq!(
            handle
                .writer
                .send_reply(record_with_xid(4, b"d"))
                .unwrap_err()
                .kind(),
            io::ErrorKind::BrokenPipe
        );
        // The peer sees EOF, not the dropped backlog.
        assert_eq!(
            peer.read_record().await.unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
    }

    #[tokio::test]
    async fn backlog_beyond_the_socket_buffer_drains_through_the_flusher() {
        let (handle, mut peer, _peer_write) = live_pair(1 << 20, 1 << 30).await;
        handle.writer.unstall_for_test();
        handle.writer.reader_waiting();
        // Far more than a loopback socket buffers, so writes hit WouldBlock
        // and the flusher task carries the backlog while the peer drains.
        let count = 4000u32;
        let payload = vec![7u8; 4096];
        let reader = tokio::spawn(async move {
            for expected in 0..count {
                let record = peer.read_record().await.unwrap().unwrap();
                assert_eq!(
                    u32::from_be_bytes(record[..4].try_into().unwrap()),
                    expected
                );
                assert_eq!(record.len(), CallBuf::PREFIX_LEN - 4 + 4096);
            }
        });
        for xid in 0..count {
            handle
                .writer
                .send_reply(record_with_xid(xid, &payload))
                .unwrap();
        }
        tokio::time::timeout(std::time::Duration::from_secs(10), reader)
            .await
            .unwrap()
            .unwrap();
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
