//! Connection state and RPC call handling.

use std::collections::{HashMap, VecDeque};
use std::io;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::task::Waker;
use tracing::Instrument;

use async_trait::async_trait;

use crate::{ProgVers, RpcError, rpc_msg, tcp_connection::WriteHalf};

const MAX_BATCH: usize = 64;

/// XDR header is ~40 bytes (xid + msg_type + rpcvers + prog + vers + proc + 2x opaque_auth).
const XDR_HEADER_ESTIMATE: usize = 48;

/// Maximum cached record buffers per thread.
const MAX_POOLED_BUFFERS: usize = 256;

/// Largest allocation the cache keeps. Rare large messages are freed.
const MAX_POOLED_CAPACITY: usize = 1 << 20;

// ---------------------------------------------------------------------------
// Buffer cache — recycles Vec<u8> allocations on the current thread
// ---------------------------------------------------------------------------

/// Record allocations stay with the worker that wrote them. Connections and
/// connection pools share this cache without sharing a lock. Bound both the
/// number of entries and their combined capacity so idle workers retain at
/// most 4 MiB each, regardless of the number of connections they have served.
const MAX_POOLED_BYTES: usize = 4 << 20;

#[derive(Default)]
struct BufferCache {
    buffers: Vec<Vec<u8>>,
    bytes: usize,
}

impl BufferCache {
    fn take(&mut self) -> Vec<u8> {
        let buffer = self.buffers.pop().unwrap_or_default();
        self.bytes -= buffer.capacity();
        buffer
    }

    fn put_batch(&mut self, batch: &mut Vec<Vec<u8>>) -> usize {
        let mut retired_bytes = 0;
        for mut buffer in batch.drain(..) {
            let capacity = buffer.capacity();
            retired_bytes += capacity;
            if self.buffers.len() >= MAX_POOLED_BUFFERS
                || capacity > MAX_POOLED_CAPACITY
                || capacity > MAX_POOLED_BYTES - self.bytes
            {
                continue;
            }
            buffer.clear();
            self.bytes += capacity;
            self.buffers.push(buffer);
        }
        retired_bytes
    }
}

thread_local! {
    static BUFFERS: std::cell::RefCell<BufferCache> = const {
        std::cell::RefCell::new(BufferCache { buffers: Vec::new(), bytes: 0 })
    };
}

pub(crate) fn take_buffer() -> Vec<u8> {
    // Calls made during thread-local destruction may outlive this cache.
    BUFFERS
        .try_with(|cache| cache.borrow_mut().take())
        .unwrap_or_default()
}

fn recycle_buffers(batch: &mut Vec<Vec<u8>>) -> usize {
    if batch.is_empty() {
        return 0;
    }
    let retired_bytes = BUFFERS
        .try_with(|cache| cache.borrow_mut().put_batch(batch))
        .unwrap_or_else(|_| batch.iter().map(Vec::capacity).sum());
    batch.clear();
    retired_bytes
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
    queued_bytes: usize,
    /// Reuse bookkeeping allocations across drain passes.
    spent: Vec<Vec<u8>>,
    /// Records the active drain pass is writing; `batch_off` bytes of the
    /// front record are already on the wire.
    batch: VecDeque<Vec<u8>>,
    batch_off: usize,
    /// Allocation capacity retained by queued and active records. Retired
    /// under the existing lock at batch boundaries, not after each syscall.
    buffered_bytes: usize,
    /// Exactly one drain pass owns the socket at a time — either the task
    /// that enqueued a record, running inline, or a flusher task parked on
    /// writable().
    draining: bool,
    /// Tokio schedules callers woken by the reader as separate tasks. Let
    /// their requests share the reader's next flush while it routes input.
    reader_active: bool,
    /// The connection's reader is parked waiting for the socket. Nothing
    /// else will flush, so replies go out as soon as they are enqueued.
    reader_waiting: bool,
    closed: bool,
}

impl WriteState {
    fn close(&mut self) {
        self.closed = true;
        self.queue.clear();
        self.batch.clear();
        self.spent.clear();
        self.queued_bytes = 0;
        self.buffered_bytes = 0;
    }
}

/// Automatic record writer with x23-style delayed replies. Calls flush
/// immediately unless the reader is actively routing input; then they share
/// its next park/yield flush. Read-buffer refills do not split the burst:
/// replies and calls stay in wire order until a flush boundary.
/// Unlike x23's direct caller resumption, Tokio
/// schedules the caller separately, and immediate writes on that worker
/// fragment request and reply batches. Both kinds flush at 64 records or 64 KiB and before
/// the reader waits or yields. No timer or scheduler-turn batching heuristic.
///
/// One producer drains inline; other producers append in order. Only a full
/// socket creates a writable-readiness task, which exits after draining.
/// An optional output threshold rejects new calls under backlog. Replies always
/// enter the queue, so admission never discards a response or closes the socket.
pub(crate) struct Writer {
    sock: tokio::net::tcp::OwnedWriteHalf,
    state: StdMutex<WriteState>,
    output_admission_threshold_bytes: usize,
    #[cfg(test)]
    write_calls: AtomicUsize,
    #[cfg(test)]
    largest_writev: AtomicUsize,
}

impl std::fmt::Debug for Writer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.state.lock().unwrap();
        f.debug_struct("Writer")
            .field("queued", &(state.queue.len() + state.batch.len()))
            .field("queued_bytes", &state.queued_bytes)
            .field("draining", &state.draining)
            .field("closed", &state.closed)
            .finish()
    }
}

impl Writer {
    fn new(
        sock: tokio::net::tcp::OwnedWriteHalf,
        output_admission_threshold_bytes: Option<usize>,
    ) -> Self {
        Self {
            sock,
            state: StdMutex::new(WriteState::default()),
            output_admission_threshold_bytes: output_admission_threshold_bytes
                .unwrap_or(usize::MAX),
            #[cfg(test)]
            write_calls: AtomicUsize::new(0),
            #[cfg(test)]
            largest_writev: AtomicUsize::new(0),
        }
    }

    /// Start a write unless the active reader will flush this burst.
    pub(crate) fn send_call(self: &Arc<Self>, msg: Vec<u8>) -> io::Result<()> {
        if self.enqueue(msg, true)? {
            self.drain();
        }
        Ok(())
    }

    /// `dwrite`: batch replies while the reader processes buffered input.
    pub(crate) fn send_reply(self: &Arc<Self>, msg: Vec<u8>) -> io::Result<()> {
        if self.enqueue(msg, false)? {
            self.drain();
        }
        Ok(())
    }

    /// Check before invoking an inbound handler. Replies bypass admission, so
    /// already accepted work and rejection responses can exceed the threshold.
    pub(crate) fn accepts_inbound_call(&self) -> bool {
        self.output_admission_threshold_bytes == usize::MAX
            || self.state.lock().unwrap().buffered_bytes < self.output_admission_threshold_bytes
    }

    /// Append in wire order and claim the drainer under the same lock.
    fn enqueue(&self, msg: Vec<u8>, call: bool) -> io::Result<bool> {
        let mut state = self.state.lock().unwrap();
        if state.closed {
            return Err(closed_error());
        }
        if call
            && msg.capacity()
                > self
                    .output_admission_threshold_bytes
                    .saturating_sub(state.buffered_bytes)
        {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "output backlog refuses new calls",
            ));
        }
        state.buffered_bytes += msg.capacity();
        state.queued_bytes += msg.len();
        state.queue.push_back(msg);
        if !state.draining
            && ((call && !state.reader_active)
                || state.queued_bytes >= FLUSH_BYTES
                || state.queue.len() >= MAX_BATCH
                || state.reader_waiting)
        {
            state.draining = true;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub(crate) fn reader_active(&self) {
        let mut state = self.state.lock().unwrap();
        state.reader_active = true;
        state.reader_waiting = false;
    }

    /// Flush before the reader waits (including a cooperative fairness yield).
    /// Changing the phase under the enqueue lock prevents a late handler reply
    /// from getting stranded after this flush.
    pub(crate) fn reader_waiting(self: &Arc<Self>) {
        {
            let mut state = self.state.lock().unwrap();
            state.reader_active = false;
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
        self.drain();
    }

    fn drain(self: &Arc<Self>) {
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
        // Take the active batch and recycling storage together. This task
        // owns them until it either parks or releases the drainer.
        let (mut batch, mut off, mut spent) = {
            let mut state = self.state.lock().unwrap();
            if state.closed {
                return Drain::Closed;
            }
            if state.batch.is_empty() {
                let WriteState { queue, batch, .. } = &mut *state;
                std::mem::swap(batch, queue);
                state.batch_off = 0;
                state.queued_bytes = 0;
            }
            (
                std::mem::take(&mut state.batch),
                state.batch_off,
                std::mem::take(&mut state.spent),
            )
        };

        loop {
            while !batch.is_empty() {
                let mut slices = [io::IoSlice::new(&[]); MAX_BATCH];
                let mut n = 0;
                for (slot, record) in slices.iter_mut().zip(&batch) {
                    let skip = if n == 0 { off } else { 0 };
                    *slot = io::IoSlice::new(&record[skip..]);
                    n += 1;
                }
                #[cfg(test)]
                {
                    self.write_calls.fetch_add(1, Ordering::Relaxed);
                    self.largest_writev.fetch_max(n, Ordering::Relaxed);
                }
                let result = match self.sock.try_write_vectored(&slices[..n]) {
                    Ok(0) => Err(io::Error::new(io::ErrorKind::WriteZero, "write returned 0")),
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
                        Ok(())
                    }
                    Err(e) => Err(e),
                };
                if let Err(e) = result {
                    let retired_bytes = recycle_buffers(&mut spent);
                    if e.kind() != io::ErrorKind::WouldBlock {
                        self.fail(&e);
                        return Drain::Closed;
                    }
                    let mut state = self.state.lock().unwrap();
                    if state.closed {
                        return Drain::Closed;
                    }
                    state.buffered_bytes -= retired_bytes;
                    state.batch = batch;
                    state.batch_off = off;
                    state.spent = spent;
                    return Drain::WouldBlock;
                }
            }
            let retired_bytes = recycle_buffers(&mut spent);
            // Hand the empty queue allocation back and claim new work in
            // one critical section. Producers cannot lose a flush between
            // our empty check and releasing drainer ownership.
            let mut state = self.state.lock().unwrap();
            if state.closed {
                return Drain::Closed;
            }
            state.buffered_bytes -= retired_bytes;
            if state.queue.is_empty() {
                state.batch = batch;
                state.spent = spent;
                state.draining = false;
                return Drain::Done;
            }
            std::mem::swap(&mut batch, &mut state.queue);
            state.queued_bytes = 0;
        }
    }

    #[cfg(test)]
    pub(crate) fn write_stats(&self) -> (usize, usize) {
        (
            self.write_calls.load(Ordering::Relaxed),
            self.largest_writev.load(Ordering::Relaxed),
        )
    }

    fn fail(&self, error: &io::Error) {
        tracing::error!(%error, "connection write failed");
        self.close();
    }

    /// Stop accepting records, drop the backlog and shut both socket directions.
    /// Waking the reader also cancels suspended handlers and pending calls.
    pub(crate) fn close(&self) {
        {
            let mut state = self.state.lock().unwrap();
            if state.closed {
                return;
            }
            state.close();
        }
        self.shutdown_socket();
    }

    fn shutdown_socket(&self) {
        let _ = socket2::SockRef::from(self.sock.as_ref()).shutdown(std::net::Shutdown::Both);
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
/// thread-local buffer cache).
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

type ReplyDecoder<'a> = dyn for<'reply> FnMut(&'reply [u8]) -> Result<(), RpcError> + Send + 'a;

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

    async unsafe fn send_call_decoded(
        &self,
        progvers: ProgVers,
        proc: u32,
        call: CallBuf,
        decode: &mut (dyn for<'reply> FnMut(&'reply [u8]) -> Result<(), RpcError> + Send),
    ) -> Result<(), RpcError> {
        // SAFETY: forward the caller's callback-lifetime contract unchanged.
        unsafe { self.call_with_decoder(progvers, proc, call, decode).await }
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
    /// The reader already decoded into the caller's result slot.
    Decoded,
    /// Resume decoder panics in the caller, outside the connection lock.
    DecodePanic(Box<dyn std::any::Any + Send>),
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
    /// Borrowed from a pinned call future. CallGuard removes it under the
    /// connection lock before cancellation can destroy callback state.
    Decoder {
        decode: &'static mut (dyn for<'reply> FnMut(&'reply [u8]) -> Result<(), RpcError> + Send),
        waker: Option<Waker>,
    },
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
            Self::Decoder { .. } => write!(f, "Decoder(..)"),
            Self::Ready(_) => write!(f, "Ready(..)"),
            Self::FireAndForget => write!(f, "FireAndForget"),
        }
    }
}

/// Hashes xids with one multiply. Every call looks its xid up several
/// times, and the default SipHash costs more than the rest of the lookup.
/// SipHash's collision resistance is not needed here: every key is
/// inserted from this connection's own xid counter, and xids a peer sends
/// are only looked up, so a peer cannot fill the map with colliding keys.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct XidHash(u64);

impl std::hash::Hasher for XidHash {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0 ^ u64::from(b)).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        }
    }
    fn write_u32(&mut self, xid: u32) {
        // Fibonacci hashing spreads sequential xids across the high bits
        // hashbrown uses for its control bytes.
        self.0 = u64::from(xid).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }
}

pub(crate) type XidMap<V> = HashMap<u32, V, std::hash::BuildHasherDefault<XidHash>>;

#[derive(Debug)]
pub(crate) struct ConnectionState {
    pub(crate) pending_requests: XidMap<CallbackSlot>,
    last_xid: u32,
    poisoned: bool,
}

impl ConnectionState {
    pub(crate) fn new() -> Self {
        Self {
            pending_requests: XidMap::default(),
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
            if matches!(slot, CallbackSlot::Decoder { .. }) {
                // The reader's decoder_calls check is only a hint, so an owned
                // reply can still reach a decoder: decode from the copy.
                return match reply {
                    ReplyResult::Ok(buf, offset) => {
                        self.handle_reply_bytes(xid, Ok(&buf[offset..]))
                    }
                    ReplyResult::Err(error) => self.handle_reply_bytes(xid, Err(error)),
                    ReplyResult::Decoded | ReplyResult::DecodePanic(_) => {
                        unreachable!("the reader only delivers raw replies")
                    }
                };
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

    /// The mutex remains held during decoding, synchronizing it with both
    /// polling and CallGuard's cancellation. No receive-buffer borrow escapes.
    pub(crate) fn handle_reply_bytes(&mut self, xid: u32, reply: Result<&[u8], RpcError>) -> bool {
        if let Some(slot @ CallbackSlot::Decoder { .. }) = self.pending_requests.get_mut(&xid) {
            let CallbackSlot::Decoder { decode, waker } =
                std::mem::replace(slot, CallbackSlot::Registered)
            else {
                unreachable!()
            };
            let result = match reply {
                Ok(bytes) => {
                    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| decode(bytes))) {
                        Ok(Ok(())) => ReplyResult::Decoded,
                        Ok(Err(error)) => ReplyResult::Err(error),
                        Err(panic) => ReplyResult::DecodePanic(panic),
                    }
                }
                Err(error) => ReplyResult::Err(error),
            };
            *slot = CallbackSlot::Ready(result);
            if let Some(waker) = waker {
                waker.wake();
            }
            return false;
        }
        // Preserve the raw-response API for existing clients and pool fallbacks.
        let reply = match reply {
            Ok(bytes) => {
                let mut buffer = take_buffer();
                buffer.extend_from_slice(bytes);
                ReplyResult::Ok(buffer, 0)
            }
            Err(error) => ReplyResult::Err(error),
        };
        self.handle_reply(xid, reply)
    }

    pub(crate) fn poison(&mut self) -> usize {
        self.poisoned = true;
        let mut faf_count = 0usize;
        for (_xid, slot) in self.pending_requests.drain() {
            match slot {
                CallbackSlot::FireAndForget => faf_count += 1,
                CallbackSlot::Waiting(waker) => waker.wake(),
                CallbackSlot::Decoder {
                    waker: Some(waker), ..
                } => waker.wake(),
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
    /// Atomic pending-request counter — avoids locking `connection_state`
    /// for load-balancing queries in DynamicPool.
    pending_count: AtomicUsize,
    /// Decoded calls in flight. While it is zero, which is always the case for
    /// callers that only use `send_call`, the reader copies a reply before
    /// taking `connection_state` instead of under it (see
    /// [`ConnectionHandle::deliver_reply`]).
    decoder_calls: AtomicUsize,
    /// Pool selection only needs the shutdown flag, not the callback map lock.
    poisoned: AtomicBool,
    // Cold — accessed rarely:
    pub(crate) connection_span: tracing::Span,
    max_message_size: u32,
    pub(crate) max_suspended_handlers: Option<usize>,
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
        let writer = Arc::new(Writer::new(
            write_half.into_inner(),
            config.output_admission_threshold_bytes,
        ));
        Self {
            connection_state: StdMutex::new(ConnectionState::new()),
            writer,
            pending_count: AtomicUsize::new(0),
            decoder_calls: AtomicUsize::new(0),
            poisoned: AtomicBool::new(false),
            connection_span: span,
            max_message_size: config.max_message_size,
            max_suspended_handlers: config.max_suspended_handlers,
            client_read_runner: StdMutex::new(None),
        }
    }

    pub(crate) fn attach_client_read_runner(&self, runner: crate::tasks::ClientReadRunner) {
        *self.client_read_runner.lock().unwrap() = Some(runner);
    }

    pub(crate) fn begin_poison(&self) -> ConnectionShutdown {
        self.poisoned.store(true, Ordering::Relaxed);
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

    /// Poison the connection, failing all pending requests and closing the socket.
    pub async fn poison(&self) {
        self.begin_poison().finish().await;
    }

    /// Queue a pre-serialized REPLY using delayed-write batching. Never waits.
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

    /// A buffer to encode the next call into, reusing a record allocation
    /// from the current thread when one is available.
    pub fn call_buf(&self) -> CallBuf {
        CallBuf::from_spare(take_buffer())
    }

    /// Send an RPC call and wait for the response.
    ///
    /// The call is framed in place: the driver fills the prefix `call`
    /// reserves and hands the buffer to the socket without copying the
    /// payload. The writer batches calls while the reader processes input,
    /// flushing before it waits/yields or reaches the output threshold.
    /// A reply flush also includes earlier calls. Returns the reply, or an
    /// error if the connection fails or output admission refuses this call.
    /// Admission failure leaves the connection and other pending calls intact.
    pub async fn send_call(
        &self,
        progvers: ProgVers,
        proc: u32,
        call: CallBuf,
    ) -> Result<RpcResponse, RpcError> {
        match self.send_call_inner(progvers, proc, call, None).await? {
            ReplyResult::Ok(buf, offset) => Ok(RpcResponse::from_record(buf, offset)),
            ReplyResult::Err(error) => Err(error),
            _ => unreachable!("raw call received a decoded reply"),
        }
    }

    /// Caller must uphold RpcSender::send_call_decoded's lifetime contract.
    async unsafe fn call_with_decoder(
        &self,
        progvers: ProgVers,
        proc: u32,
        call: CallBuf,
        decode: &mut (dyn for<'reply> FnMut(&'reply [u8]) -> Result<(), RpcError> + Send),
    ) -> Result<(), RpcError> {
        // SAFETY: the caller keeps the callback valid even if this future is
        // forgotten. The pending slot is only accessed under connection_state;
        // send_call_inner installs its cancellation guard before publishing it.
        // Completion removes the reference before waking/returning to the caller.
        let decode = unsafe {
            std::mem::transmute::<
                &mut (dyn for<'reply> FnMut(&'reply [u8]) -> Result<(), RpcError> + Send),
                &'static mut (dyn for<'reply> FnMut(&'reply [u8]) -> Result<(), RpcError> + Send),
            >(decode)
        };
        // Declared before the call so it outlives the pending slot, which the
        // call's own guard removes first.
        let _decoding = DecoderCall::new(&self.decoder_calls);
        match self
            .send_call_inner(progvers, proc, call, Some(decode))
            .await?
        {
            ReplyResult::Decoded => Ok(()),
            ReplyResult::Err(error) => Err(error),
            ReplyResult::DecodePanic(panic) => std::panic::resume_unwind(panic),
            _ => unreachable!("decoded call received a raw reply"),
        }
    }

    /// Deliver a reply the reader parsed out of its receive buffer. Callers
    /// waiting for an owned response get a copy made before taking the lock,
    /// as without decoders; only while decoded calls are in flight does the
    /// reader take the lock first, to decode in place or copy under it.
    pub(crate) fn deliver_reply(&self, xid: u32, reply: Result<&[u8], RpcError>) -> bool {
        if self.decoder_calls.load(Ordering::Acquire) != 0 {
            return self
                .connection_state
                .lock()
                .unwrap()
                .handle_reply_bytes(xid, reply);
        }
        let reply = match reply {
            Ok(bytes) => {
                let mut buffer = take_buffer();
                buffer.extend_from_slice(bytes);
                ReplyResult::Ok(buffer, 0)
            }
            Err(error) => ReplyResult::Err(error),
        };
        self.connection_state
            .lock()
            .unwrap()
            .handle_reply(xid, reply)
    }

    async fn send_call_inner(
        &self,
        progvers: ProgVers,
        proc: u32,
        call: CallBuf,
        decode: Option<&'static mut ReplyDecoder<'static>>,
    ) -> Result<ReplyResult, RpcError> {
        let (xid, guard) = {
            let mut state = self.connection_state.lock().unwrap();
            let xid = state.register_call()?;
            self.pending_count.fetch_add(1, Ordering::Relaxed);
            let guard = CallGuard {
                state: &self.connection_state,
                pending_count: &self.pending_count,
                xid,
                done: false,
            };
            if let Some(decode) = decode {
                *state.pending_requests.get_mut(&xid).unwrap() = CallbackSlot::Decoder {
                    decode,
                    waker: None,
                };
            }
            (xid, guard)
        };

        let span = tracing::debug_span!(
            parent: &self.connection_span,
            "rpc_call",
            xid,
            prog = progvers.prog(),
            vers = progvers.vers(),
            proc = proc,
        );

        // On error the guard's drop cleans up the pending entry.
        let msg = self.frame_call_or_warn(call, xid, progvers, proc)?;

        let mut replied = false;
        let result = async {
            if let Err(e) = self.writer.send_call(msg) {
                tracing::debug!(?e, "call refused by the connection writer");
                if e.kind() != io::ErrorKind::WouldBlock {
                    let _shutdown = self.begin_poison();
                }
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
    /// never waits for socket capacity. Success does not
    /// confirm that the socket write or peer-side dispatch completed. The
    /// reader task silently discards the eventual reply.
    /// Output admission failure returns an error without closing the connection.
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

        // Guard removes the entry if framing or enqueue fails.
        let mut guard = FireAndForgetGuard {
            state: &self.connection_state,
            pending_count: &self.pending_count,
            xid,
            sent: false,
        };

        let msg = self.frame_call_or_warn(call, xid, progvers, proc)?;

        // Use .instrument(span) instead of span.enter() — enter() is
        // thread-local and incorrect across .await points in multi-thread
        // runtimes (the future may resume on a different thread).
        async {
            if let Err(e) = self.writer.send_call(msg) {
                tracing::debug!(?e, "call refused by the connection writer");
                if e.kind() != io::ErrorKind::WouldBlock {
                    let _shutdown = self.begin_poison();
                }
                return Err(RpcError::SystemError); // guard drops and cleans up
            }
            guard.sent = true;
            Ok(())
        }
        .instrument(span)
        .await
    }

    fn frame_call_or_warn(
        &self,
        call: CallBuf,
        xid: u32,
        progvers: ProgVers,
        proc: u32,
    ) -> Result<Vec<u8>, RpcError> {
        let payload_len = call.payload().len();
        // x23 checks the outgoing record size before registering/sending it.
        // This is independent of the amount of output already queued.
        if payload_len.saturating_add(CallBuf::PREFIX_LEN - 4) > self.max_message_size as usize {
            tracing::warn!(payload_len, "outgoing CALL exceeds message-size limit");
            return Err(RpcError::SystemError);
        }
        frame_call(call, xid, progvers, proc).map_err(|()| {
            tracing::warn!(payload_len, "message too large: exceeds max (0x7FFF_FFFF)");
            RpcError::SystemError
        })
    }

    /// Wait for the read loop to deliver a result for the given XID. Either
    /// way — reply or poisoned connection — the pending entry is gone when
    /// this returns.
    async fn await_reply(&self, xid: u32) -> Result<ReplyResult, RpcError> {
        std::future::poll_fn(|cx| {
            let mut state = self.connection_state.lock().unwrap();
            match state.pending_requests.get_mut(&xid) {
                Some(CallbackSlot::Ready(_)) => match state.pending_requests.remove(&xid) {
                    Some(CallbackSlot::Ready(reply)) => std::task::Poll::Ready(Ok(reply)),
                    _ => unreachable!(),
                },
                Some(slot) => {
                    // Avoid cloning the waker if unchanged since last poll.
                    match slot {
                        CallbackSlot::Decoder { waker, .. } => {
                            if waker.as_ref().is_none_or(|w| !w.will_wake(cx.waker())) {
                                *waker = Some(cx.waker().clone());
                            }
                        }
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
        self.poisoned.load(Ordering::Relaxed)
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
/// Counts a decoded call in [`ConnectionHandle::decoder_calls`] while it lives.
struct DecoderCall<'a>(&'a AtomicUsize);

impl<'a> DecoderCall<'a> {
    fn new(count: &'a AtomicUsize) -> Self {
        count.fetch_add(1, Ordering::Release);
        Self(count)
    }
}

impl Drop for DecoderCall<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Release);
    }
}

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
            let removed = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .pending_requests
                .remove(&self.xid);
            drop(removed);
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

    async unsafe fn send_call_decoded(
        &self,
        progvers: ProgVers,
        proc: u32,
        call: CallBuf,
        decode: &mut (dyn for<'reply> FnMut(&'reply [u8]) -> Result<(), RpcError> + Send),
    ) -> Result<(), RpcError> {
        // SAFETY: forward the caller's callback-lifetime contract unchanged.
        unsafe {
            (**self)
                .call_with_decoder(progvers, proc, call, decode)
                .await
        }
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
    fn owned_reply_reaching_a_decoder_slot_is_decoded() {
        let mut seen = Vec::new();
        let mut decode = |bytes: &[u8]| {
            seen.extend_from_slice(bytes);
            Ok(())
        };
        let mut state = ConnectionState::new();
        let xid = state.register_call().unwrap();
        // SAFETY: the slot is consumed below, before `decode` goes out of scope.
        let decode: &'static mut ReplyDecoder<'static> = unsafe {
            std::mem::transmute::<&mut ReplyDecoder<'_>, &'static mut ReplyDecoder<'static>>(
                &mut decode,
            )
        };
        *state.pending_requests.get_mut(&xid).unwrap() = CallbackSlot::Decoder {
            decode,
            waker: None,
        };
        assert!(!state.handle_reply(xid, ReplyResult::Ok(b"hdrpayload".to_vec(), 3)));
        assert!(matches!(
            state.pending_requests.remove(&xid),
            Some(CallbackSlot::Ready(ReplyResult::Decoded))
        ));
        assert_eq!(seen, b"payload");
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
    fn buffer_cache_bounds_count_and_capacity() {
        let mut cache = BufferCache::default();
        let mut batch: Vec<Vec<u8>> = (0..MAX_POOLED_BUFFERS + 10).map(|_| vec![1]).collect();
        batch.push(Vec::with_capacity(MAX_POOLED_CAPACITY + 1));
        cache.put_batch(&mut batch);
        assert!(batch.is_empty());
        assert_eq!(cache.buffers.len(), MAX_POOLED_BUFFERS);
        assert!(
            cache
                .buffers
                .iter()
                .all(|b| b.capacity() <= MAX_POOLED_CAPACITY)
        );
        assert!(cache.take().is_empty());

        let mut cache = BufferCache::default();
        let mut batch: Vec<_> = (0..10)
            .map(|_| Vec::with_capacity(MAX_POOLED_CAPACITY))
            .collect();
        cache.put_batch(&mut batch);
        assert_eq!(cache.bytes, MAX_POOLED_BYTES);
        while !cache.buffers.is_empty() {
            cache.take();
        }
        assert_eq!(cache.bytes, 0);
    }

    #[test]
    fn call_buf_reuses_written_records_on_the_current_thread() {
        BUFFERS.with(|cache| *cache.borrow_mut() = BufferCache::default());
        let record = frame_call(call_with(&[9; 300]), 1, ProgVers::new(1, 1), 0).unwrap();
        let ptr = record.as_ptr();
        recycle_buffers(&mut vec![record]);

        let call = CallBuf::from_spare(take_buffer());
        assert!(call.payload().is_empty());
        let reused = call.into_vec();
        assert_eq!(reused.as_ptr(), ptr);
        assert_eq!(take_buffer().capacity(), 0);
    }

    #[test]
    fn buffer_caches_are_thread_local() {
        BUFFERS.with(|cache| *cache.borrow_mut() = BufferCache::default());
        recycle_buffers(&mut vec![Vec::with_capacity(1024)]);
        std::thread::spawn(|| assert_eq!(take_buffer().capacity(), 0))
            .join()
            .unwrap();
        assert!(take_buffer().capacity() >= 1024);
    }

    /// A connection handle over a live loopback socket, its peer's read
    /// half, and the peer's write half (kept alive so the socket stays open).
    async fn live_pair() -> (
        Arc<ConnectionHandle>,
        crate::tcp_connection::ReadHalf,
        tokio::net::tcp::OwnedWriteHalf,
    ) {
        live_pair_with_config(&crate::ConnectionConfig::default()).await
    }

    async fn live_pair_with_config(
        config: &crate::ConnectionConfig,
    ) -> (
        Arc<ConnectionHandle>,
        crate::tcp_connection::ReadHalf,
        tokio::net::tcp::OwnedWriteHalf,
    ) {
        let (handle, _read, peer) = ConnectionHandle::new_stalled_for_test(config).await;
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
        let (handle, mut peer, _peer_write) = live_pair().await;
        handle.writer.unstall_for_test();

        // While the reader works through buffered input, replies queue up.
        handle.writer.reader_active();
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
    async fn calls_flush_without_an_active_reader_including_delayed_replies() {
        let (handle, mut peer, _peer_write) = live_pair().await;
        handle.writer.unstall_for_test();
        handle
            .writer
            .send_reply(record_with_xid(1, b"reply"))
            .unwrap();
        handle
            .writer
            .send_call(record_with_xid(2, b"call"))
            .unwrap();
        assert_eq!(handle.writer.queued_for_test(), 0);
        assert_eq!(handle.writer.write_calls.load(Ordering::Relaxed), 1);
        assert_eq!(handle.writer.largest_writev.load(Ordering::Relaxed), 2);
        assert_eq!(peer_xid(&mut peer).await, Some(1));
        assert_eq!(peer_xid(&mut peer).await, Some(2));
    }

    #[tokio::test]
    async fn active_reader_batches_calls_and_flushes_before_waiting() {
        let (handle, mut peer, _peer_write) = live_pair().await;
        handle.writer.unstall_for_test();
        handle.writer.reader_active();
        handle.writer.send_call(record_with_xid(1, b"a")).unwrap();
        handle.writer.send_call(record_with_xid(2, b"b")).unwrap();
        assert_eq!(handle.writer.write_stats().0, 0);
        assert_eq!(peer_xid(&mut peer).await, None);
        handle.writer.reader_waiting();
        assert_eq!(handle.writer.write_stats(), (1, 2));
        assert_eq!(peer_xid(&mut peer).await, Some(1));
        assert_eq!(peer_xid(&mut peer).await, Some(2));
        // A caller arriving after the flush must make progress independently.
        handle
            .writer
            .send_call(record_with_xid(3, b"late"))
            .unwrap();
        assert_eq!(peer_xid(&mut peer).await, Some(3));
    }

    #[tokio::test]
    async fn yielding_to_a_handler_flushes_pending_replies() {
        let (handle, mut peer, _peer_write) = live_pair().await;
        handle.writer.unstall_for_test();
        handle.writer.reader_active();
        handle.writer.send_reply(record_with_xid(1, b"a")).unwrap();
        assert_eq!(peer_xid(&mut peer).await, None);
        // Fairness yield: flush before handing the worker to another task.
        handle.writer.reader_waiting();
        assert_eq!(peer_xid(&mut peer).await, Some(1));
    }

    #[tokio::test]
    async fn a_full_writev_of_small_records_flushes_without_waiting() {
        let (handle, mut peer, _peer_write) = live_pair().await;
        handle.writer.unstall_for_test();
        handle.writer.reader_active();
        for xid in 1..MAX_BATCH as u32 {
            handle
                .writer
                .send_reply(record_with_xid(xid, b"a"))
                .unwrap();
        }
        assert_eq!(handle.writer.write_stats().0, 0);
        handle
            .writer
            .send_reply(record_with_xid(MAX_BATCH as u32, b"a"))
            .unwrap();
        for xid in 1..=MAX_BATCH as u32 {
            assert_eq!(peer_xid(&mut peer).await, Some(xid));
        }
        // Writable-readiness retries or partial writes may take extra attempts.
        assert_eq!(handle.writer.write_stats().1, MAX_BATCH);
    }

    #[tokio::test]
    async fn replies_flush_at_the_byte_threshold() {
        let (handle, mut peer, _peer_write) = live_pair().await;
        handle.writer.unstall_for_test();
        handle.writer.reader_active();
        let record = record_with_xid(1, &[0; FLUSH_BYTES - CallBuf::PREFIX_LEN]);
        assert_eq!(record.len(), FLUSH_BYTES);
        handle.writer.send_reply(record).unwrap();
        assert_eq!(peer_xid(&mut peer).await, Some(1));
    }

    #[tokio::test]
    async fn read_refills_preserve_the_output_burst_until_waiting() {
        use tokio::io::AsyncWriteExt;
        let config = crate::ConnectionConfig::default();
        let (handle, mut read, mut peer) = ConnectionHandle::new_stalled_for_test(&config).await;
        handle.writer.unstall_for_test();
        handle.writer.reader_active();
        // The test reader has a 4096-byte buffer. The next record is only
        // partly buffered, while its remainder is already on the socket.
        let burst = [
            record_with_xid(1, &[1; 3000]),
            record_with_xid(2, &[2; 3000]),
        ]
        .concat();
        peer.write_all(&burst).await.unwrap();
        assert!(read.read_record().await.unwrap().is_some());
        handle
            .writer
            .send_reply(record_with_xid(9, b"reply"))
            .unwrap();
        handle
            .writer
            .send_call(record_with_xid(10, b"call"))
            .unwrap();
        assert_eq!(handle.writer.write_stats().0, 0);
        read.readable().await.unwrap();
        let next = read.try_read_record().unwrap();
        assert!(matches!(next, crate::tcp_connection::TryRecord::Ready(_)));
        assert_eq!(
            handle.writer.write_stats().0,
            0,
            "a readable socket refill must not split the output burst"
        );
        let (peer_read, _peer_write) = peer.into_split();
        let mut peer = crate::tcp_connection::ReadHalf::new_for_test(peer_read);
        assert_eq!(peer_xid(&mut peer).await, None);
        // Calls and replies across refills keep their order in one batch.
        handle
            .writer
            .send_call(record_with_xid(11, b"later"))
            .unwrap();
        assert_eq!(handle.writer.write_stats().0, 0);
        handle.writer.reader_waiting();
        assert_eq!(handle.writer.write_stats(), (1, 3));
        assert_eq!(peer_xid(&mut peer).await, Some(9));
        assert_eq!(peer_xid(&mut peer).await, Some(10));
        assert_eq!(peer_xid(&mut peer).await, Some(11));
    }

    #[tokio::test]
    async fn reply_during_reader_yield_is_not_stranded() {
        let (handle, mut peer, _peer_write) = live_pair().await;
        handle.writer.unstall_for_test();
        handle.writer.reader_waiting();
        // A suspended handler completes after the reader's flush, before
        // the reader resumes. Its reply must initiate its own flush.
        handle
            .writer
            .send_reply(record_with_xid(1, b"late"))
            .unwrap();
        assert_eq!(peer_xid(&mut peer).await, Some(1));
    }

    #[tokio::test]
    async fn closed_writer_rejects_calls_and_replies() {
        let (handle, mut peer, _peer_write) = live_pair().await;
        handle.writer.close();
        assert_eq!(
            handle
                .writer
                .send_call(record_with_xid(1, b"a"))
                .unwrap_err()
                .kind(),
            io::ErrorKind::BrokenPipe
        );
        assert_eq!(
            handle
                .writer
                .send_reply(record_with_xid(2, b"b"))
                .unwrap_err()
                .kind(),
            io::ErrorKind::BrokenPipe
        );
        assert_eq!(
            peer.read_record().await.unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
    }

    #[tokio::test]
    async fn output_admission_rejects_only_new_calls() {
        use std::future::Future;
        use std::task::Poll;

        let config = crate::ConnectionConfig::default()
            .with_output_admission_threshold_bytes(Some(CallBuf::new().into_vec().capacity()));
        let (handle, mut peer, _peer_write) = live_pair_with_config(&config).await;
        let mut pending = std::pin::pin!(handle.send_call(ProgVers::new(1, 1), 0, CallBuf::new()));
        std::future::poll_fn(|cx| {
            assert!(pending.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;

        assert!(matches!(
            handle
                .send_call(ProgVers::new(1, 1), 0, CallBuf::new())
                .await,
            Err(RpcError::SystemError)
        ));
        assert!(matches!(
            handle
                .send_fire_and_forget(ProgVers::new(1, 1), 0, CallBuf::new())
                .await,
            Err(RpcError::SystemError)
        ));
        assert!(!handle.is_poisoned());
        assert_eq!(handle.num_pending_requests(), 1);
        assert_eq!(handle.writer.queued_for_test(), 1);

        // Replies bypass admission even when the backlog is already full.
        handle
            .writer
            .send_reply(record_with_xid(999, b"reply"))
            .unwrap();
        assert_eq!(handle.writer.queued_for_test(), 2);
        handle.writer.unstall_for_test();
        let xid = peer_xid(&mut peer).await.unwrap();
        assert_eq!(peer_xid(&mut peer).await, Some(999));
        handle
            .connection_state
            .lock()
            .unwrap()
            .handle_reply(xid, ReplyResult::Ok(b"completed".to_vec(), 0));
        assert_eq!(pending.await.unwrap(), b"completed");
        assert_eq!(handle.num_pending_requests(), 0);

        // Once the backlog drains, the same connection accepts calls again.
        handle.writer.send_call(record_with_xid(1000, &[])).unwrap();
        assert_eq!(peer_xid(&mut peer).await, Some(1000));
    }

    #[tokio::test]
    async fn output_admission_preserves_partially_written_replies() {
        let payload = vec![7; 512 * 1024];
        let record = record_with_xid(1, &payload);
        let record_capacity = record.capacity();
        let config = crate::ConnectionConfig::default()
            .with_output_admission_threshold_bytes(Some(record_capacity));
        let (handle, mut peer, _peer_write) = live_pair_with_config(&config).await;
        socket2::SockRef::from(handle.writer.sock.as_ref())
            .set_send_buffer_size(4096)
            .unwrap();
        handle.writer.send_reply(record).unwrap();
        handle.writer.unstall_for_test();
        {
            let state = handle.writer.state.lock().unwrap();
            assert!(state.queue.is_empty());
            assert_eq!(state.batch.len(), 1);
            assert!(state.batch_off > 0, "must exercise a partial record write");
            assert_eq!(state.buffered_bytes, record_capacity);
        }
        assert_eq!(
            handle
                .writer
                .send_call(record_with_xid(3, &[]))
                .unwrap_err()
                .kind(),
            io::ErrorKind::WouldBlock
        );
        handle
            .writer
            .send_reply(record_with_xid(2, b"second"))
            .unwrap();
        assert!(!handle.is_poisoned());
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let first = peer.read_record().await.unwrap().unwrap();
            assert_eq!(&first[CallBuf::PREFIX_LEN - 4..], payload);
            let second = peer.read_record().await.unwrap().unwrap();
            assert_eq!(u32::from_be_bytes(second[..4].try_into().unwrap()), 2);
            assert_eq!(&second[CallBuf::PREFIX_LEN - 4..], b"second");
        })
        .await
        .expect("admitted replies must drain intact");
        assert!(handle.writer.accepts_inbound_call());
    }

    #[tokio::test]
    async fn output_admission_counts_spare_capacity_and_recovers_after_drain() {
        let config =
            crate::ConnectionConfig::default().with_output_admission_threshold_bytes(Some(1024));
        let (handle, mut peer, _peer_write) = live_pair_with_config(&config).await;
        // A short response can retain the much larger input allocation.
        let mut reply = Vec::with_capacity(1024);
        reply.extend_from_slice(&record_with_xid(1, &[]));
        handle.writer.send_reply(reply).unwrap();
        assert!(!handle.writer.accepts_inbound_call());
        assert_eq!(
            handle
                .writer
                .send_call(record_with_xid(2, &[]))
                .unwrap_err()
                .kind(),
            io::ErrorKind::WouldBlock
        );
        handle.writer.send_reply(record_with_xid(3, &[])).unwrap();
        handle.writer.unstall_for_test();
        assert_eq!(peer_xid(&mut peer).await, Some(1));
        assert_eq!(peer_xid(&mut peer).await, Some(3));
        assert!(handle.writer.accepts_inbound_call());
        handle.writer.send_call(record_with_xid(4, &[])).unwrap();
        assert_eq!(peer_xid(&mut peer).await, Some(4));
    }

    #[tokio::test]
    async fn backlog_beyond_the_socket_buffer_drains_through_the_flusher() {
        let config = crate::ConnectionConfig::default()
            .with_output_admission_threshold_bytes(Some(32 * 1024 * 1024));
        let (handle, mut peer, _peer_write) = live_pair_with_config(&config).await;
        handle.writer.unstall_for_test();
        handle.writer.reader_waiting();
        // Far more than a loopback socket buffers, so writes hit WouldBlock
        // and the flusher task carries the backlog while the peer drains.
        socket2::SockRef::from(handle.writer.sock.as_ref())
            .set_send_buffer_size(4096)
            .unwrap();
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
                assert!(record[CallBuf::PREFIX_LEN - 4..].iter().all(|&b| b == 7));
            }
        });
        for xid in 0..count {
            handle
                .writer
                .send_reply(record_with_xid(xid, &payload))
                .unwrap();
        }
        assert!(
            !handle.writer.state.lock().unwrap().batch.is_empty(),
            "test must exercise WouldBlock with a partially drained batch"
        );
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
