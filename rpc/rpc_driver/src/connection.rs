//! Connection state and RPC call handling.

use std::cell::UnsafeCell;
use std::collections::{HashMap, VecDeque};
use std::io;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::task::{Poll, Waker};
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
        unsafe { self.call(progvers, proc, call, decode).await }
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

/// One call's rendezvous with the reader. It lives inside the call's own
/// future, which is pinned while the call is registered, and the pending map
/// holds only a pointer to it.
///
/// Whoever removes the call's map entry (the reader with its reply, or
/// poisoning) owns finishing it: it runs the decoder outside the lock,
/// stores the outcome and sets `done`, after which it never touches the slot
/// again. The future reads its outcome from here without taking the lock.
/// If the future is dropped while someone else holds its entry, the drop
/// waits for `done` so the decoder never outlives what it borrows.
pub(crate) struct CallSlot {
    decode: NonNull<ReplyDecoder<'static>>,
    /// Accessed only under `connection_state`, by the caller while its entry
    /// is in the map and by whoever removes the entry.
    waker: UnsafeCell<Option<Waker>>,
    /// Written once, before `done`, by whoever finished the call.
    outcome: UnsafeCell<Option<std::thread::Result<Result<(), RpcError>>>>,
    done: AtomicBool,
}

// SAFETY: the decoder is `Send`; `waker` is guarded by `connection_state`, and
// `outcome` is published by the release store to `done`.
unsafe impl Send for CallSlot {}
unsafe impl Sync for CallSlot {}

impl CallSlot {
    /// # Safety
    /// `decode` must stay valid until the call holding this slot finishes or
    /// its future is dropped.
    unsafe fn new(decode: &mut ReplyDecoder<'_>) -> Self {
        // SAFETY: lifetime erased under the caller's contract.
        let decode = unsafe {
            std::mem::transmute::<NonNull<ReplyDecoder<'_>>, NonNull<ReplyDecoder<'static>>>(
                NonNull::from(decode),
            )
        };
        Self {
            decode,
            waker: UnsafeCell::new(None),
            outcome: UnsafeCell::new(None),
            done: AtomicBool::new(false),
        }
    }

    /// # Safety
    /// Requires the `connection_state` lock (see `waker`).
    unsafe fn take_waker(&self) -> Option<Waker> {
        unsafe { (*self.waker.get()).take() }
    }

    fn is_done(&self) -> bool {
        self.done.load(Ordering::Acquire)
    }

    /// Wait for whoever removed this call's entry to finish it. That is the
    /// length of one decode, so spin rather than park.
    fn wait_done(&self) {
        let mut spins = 0u32;
        while !self.is_done() {
            if spins < 64 {
                std::hint::spin_loop();
                spins += 1;
            } else {
                std::thread::yield_now();
            }
        }
    }
}

/// Finish a call whose entry the caller just removed from the map, taking
/// `waker` out while it still held the lock.
///
/// # Safety
/// `slot` must be the removed entry's slot; removal makes the caller its
/// only finisher, and the owning future waits for `done` before freeing it.
unsafe fn finish_call(
    slot: NonNull<CallSlot>,
    waker: Option<Waker>,
    reply: Result<&[u8], RpcError>,
) {
    // SAFETY: see above; the slot outlives this call until `done` is set.
    let slot = unsafe { slot.as_ref() };
    let outcome = match reply {
        Ok(bytes) => std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            // SAFETY: the decoder lives as long as its call (CallSlot::new).
            unsafe { (*slot.decode.as_ptr())(bytes) }
        })),
        Err(error) => Ok(Err(error)),
    };
    // SAFETY: only the finisher writes `outcome`, before publishing `done`.
    unsafe { *slot.outcome.get() = Some(outcome) };
    slot.done.store(true, Ordering::Release);
    // The future may free the slot from here on.
    if let Some(waker) = waker {
        waker.wake();
    }
}

/// A pending map entry: a waiting call, or a fire-and-forget call whose
/// reply is discarded.
#[derive(Debug)]
pub(crate) enum Pending {
    Call(NonNull<CallSlot>),
    FireAndForget,
}

// SAFETY: `Call` points at a `Send + Sync` slot that outlives its entry.
unsafe impl Send for Pending {}

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
    pub(crate) pending_requests: XidMap<Pending>,
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
    pub(crate) fn register_call(&mut self, slot: NonNull<CallSlot>) -> Result<u32, RpcError> {
        if self.poisoned {
            return Err(RpcError::SystemError);
        }

        loop {
            self.last_xid = self.last_xid.wrapping_add(1);
            if let std::collections::hash_map::Entry::Vacant(e) =
                self.pending_requests.entry(self.last_xid)
            {
                e.insert(Pending::Call(slot));
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
                e.insert(Pending::FireAndForget);
                return Ok(self.last_xid);
            }
        }
    }

    pub(crate) fn poison(&mut self) -> usize {
        self.poisoned = true;
        let mut faf_count = 0usize;
        for (_xid, pending) in self.pending_requests.drain() {
            match pending {
                Pending::FireAndForget => faf_count += 1,
                // SAFETY: draining the entry under the lock makes this its finisher.
                Pending::Call(slot) => unsafe {
                    let waker = slot.as_ref().take_waker();
                    finish_call(slot, waker, Err(RpcError::SystemError));
                },
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
        let mut reply = None;
        // Runs on the reader, outside the connection lock: the same copy out
        // of the receive buffer the reader always made.
        let mut decode = |bytes: &[u8]| {
            let mut buffer = take_buffer();
            buffer.extend_from_slice(bytes);
            reply = Some(buffer);
            Ok(())
        };
        // SAFETY: `decode` lives in this future, so it outlives the call even
        // if the future is leaked rather than dropped.
        unsafe { self.call(progvers, proc, call, &mut decode).await }?;
        Ok(RpcResponse::from_record(
            reply.expect("a successful call ran its decoder"),
            0,
        ))
    }

    /// Send a call and run `decode` on the reader thread over the reply's
    /// payload, straight from the receive buffer.
    ///
    /// # Safety
    /// `decode` must stay valid until this future completes or is dropped;
    /// leaking the future instead leaves the reader free to call it later.
    async unsafe fn call(
        &self,
        progvers: ProgVers,
        proc: u32,
        call: CallBuf,
        decode: &mut ReplyDecoder<'_>,
    ) -> Result<(), RpcError> {
        // SAFETY: forwarded from this function's contract.
        let mut pending = PendingCall {
            handle: self,
            slot: unsafe { CallSlot::new(decode) },
            xid: None,
        };
        // Register with this poll's waker under one lock; the reply's
        // completion then needs no lock on this side.
        let xid = std::future::poll_fn(|cx| Poll::Ready(pending.register(cx.waker()))).await?;

        let span = tracing::debug_span!(
            parent: &self.connection_span,
            "rpc_call",
            xid,
            prog = progvers.prog(),
            vers = progvers.vers(),
            proc = proc,
        );

        // On error the pending call's drop cleans up its entry.
        let msg = self.frame_call_or_warn(call, xid, progvers, proc)?;

        async {
            if let Err(e) = self.writer.send_call(msg) {
                tracing::debug!(?e, "call refused by the connection writer");
                if e.kind() != io::ErrorKind::WouldBlock {
                    let _shutdown = self.begin_poison();
                }
                return Err(RpcError::SystemError);
            }
            pending.wait().await
        }
        .instrument(span)
        .await
    }

    /// Deliver a reply the reader parsed out of its receive buffer: the
    /// call's decoder runs here, outside the connection lock. Returns true
    /// for a fire-and-forget call's reply.
    pub(crate) fn deliver_reply(&self, xid: u32, reply: Result<&[u8], RpcError>) -> bool {
        let mut state = self.connection_state.lock().unwrap();
        match state.pending_requests.remove(&xid) {
            Some(Pending::Call(slot)) => {
                // SAFETY: removing the entry under the lock makes this its finisher.
                unsafe {
                    let waker = slot.as_ref().take_waker();
                    drop(state);
                    finish_call(slot, waker, reply);
                }
                false
            }
            Some(Pending::FireAndForget) => true,
            None => {
                drop(state);
                tracing::warn!(xid, "received reply for unknown xid");
                false
            }
        }
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

/// A call's slot and its registration; dropping it cancels the call.
struct PendingCall<'a> {
    handle: &'a ConnectionHandle,
    slot: CallSlot,
    /// Set while registered (and counted in `pending_count`).
    xid: Option<u32>,
}

impl PendingCall<'_> {
    fn register(&mut self, waker: &Waker) -> Result<u32, RpcError> {
        let slot = NonNull::from(&self.slot);
        let mut state = self.handle.connection_state.lock().unwrap();
        let xid = state.register_call(slot)?;
        // SAFETY: the entry was just inserted under this lock.
        unsafe { *self.slot.waker.get() = Some(waker.clone()) };
        drop(state);
        self.handle.pending_count.fetch_add(1, Ordering::Relaxed);
        self.xid = Some(xid);
        Ok(xid)
    }

    async fn wait(&mut self) -> Result<(), RpcError> {
        let xid = self.xid.expect("registered");
        // The first poll is the one that registered the waker.
        let mut fresh = true;
        std::future::poll_fn(|cx| {
            if self.slot.is_done() {
                return Poll::Ready(());
            }
            if std::mem::take(&mut fresh) {
                return Poll::Pending;
            }
            let state = self.handle.connection_state.lock().unwrap();
            if state.pending_requests.contains_key(&xid) {
                // SAFETY: our entry is in the map and the lock is held.
                let waker = unsafe { &mut *self.slot.waker.get() };
                if !waker.as_ref().is_some_and(|w| w.will_wake(cx.waker())) {
                    *waker = Some(cx.waker().clone());
                }
                return Poll::Pending;
            }
            drop(state);
            // Claimed: the reader is decoding into it right now.
            self.slot.wait_done();
            Poll::Ready(())
        })
        .await;
        self.xid = None;
        self.handle.pending_count.fetch_sub(1, Ordering::Relaxed);
        // SAFETY: `done` was observed, so the outcome is published and no
        // one else touches the slot.
        match unsafe { (*self.slot.outcome.get()).take() }.expect("finished call has an outcome") {
            Ok(result) => result,
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }
}

impl Drop for PendingCall<'_> {
    fn drop(&mut self) {
        let Some(xid) = self.xid else { return };
        self.handle.pending_count.fetch_sub(1, Ordering::Relaxed);
        if self.slot.is_done() {
            return;
        }
        let removed = self
            .handle
            .connection_state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pending_requests
            .remove(&xid);
        if removed.is_none() {
            // Someone claimed it and may be running the decoder, which
            // borrows memory this drop is about to free.
            self.slot.wait_done();
        }
    }
}

/// Drop guard for cancellation safety in `send_fire_and_forget`.
///
/// If the future is dropped before the message is sent (e.g. due to a
/// timeout), this removes the pending entry and decrements the counter.
/// Unlike `PendingCall`, this only guards the send phase — once sent, the
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
        unsafe { (**self).call(progvers, proc, call, decode).await }
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
        assert!(!handle.deliver_reply(xid, Ok(b"completed")));
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

    /// A slot whose decoder records the reply.
    fn recording_slot(seen: &mut Vec<u8>) -> impl FnMut(&[u8]) -> Result<(), RpcError> + Send + '_ {
        |bytes: &[u8]| {
            seen.extend_from_slice(bytes);
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_callback_roundtrip() {
        let (handle, _peer, _peer_write) = live_pair().await;
        let mut seen = Vec::new();
        let mut decode = recording_slot(&mut seen);
        // SAFETY: `decode` outlives `slot`.
        let slot = unsafe { CallSlot::new(&mut decode) };
        let xid = handle
            .connection_state
            .lock()
            .unwrap()
            .register_call(NonNull::from(&slot))
            .unwrap();
        assert_eq!(xid, 1);

        // A reply for an unknown xid is ignored.
        assert!(!handle.deliver_reply(0, Ok(b"world")));
        assert!(!slot.is_done());

        assert!(!handle.deliver_reply(xid, Ok(b"hello")));
        assert!(slot.is_done());
        assert!(matches!(
            unsafe { (*slot.outcome.get()).take() },
            Some(Ok(Ok(())))
        ));
        assert!(
            handle
                .connection_state
                .lock()
                .unwrap()
                .pending_requests
                .is_empty()
        );

        // A duplicate reply finds no entry.
        assert!(!handle.deliver_reply(xid, Ok(b"foo")));
        drop(slot);
        drop(decode);
        assert_eq!(seen, b"hello");
    }

    #[tokio::test]
    async fn dropping_a_registered_call_removes_its_entry() {
        let (handle, _peer, _peer_write) = live_pair().await;
        let mut call = Box::pin(handle.send_call(ProgVers::new(1, 1), 0, CallBuf::new()));
        std::future::poll_fn(|cx| {
            assert!(call.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        assert_eq!(handle.num_pending_requests(), 1);

        drop(call);
        assert_eq!(handle.num_pending_requests(), 0);
        assert!(
            handle
                .connection_state
                .lock()
                .unwrap()
                .pending_requests
                .is_empty()
        );
    }

    #[test]
    fn test_poison_wakes_waiters() {
        let mut state = ConnectionState::new();
        let mut decode = |_: &[u8]| Ok(());
        // SAFETY: `decode` outlives `slot`.
        let slot = unsafe { CallSlot::new(&mut decode) };
        state.register_call(NonNull::from(&slot)).unwrap();
        // SAFETY: no lock to contend with in this test.
        unsafe { *slot.waker.get() = Some(std::task::Waker::noop().clone()) };

        state.poison();
        assert!(state.pending_requests.is_empty());
        assert!(state.poisoned);
        assert!(slot.is_done());
        assert!(matches!(
            unsafe { (*slot.outcome.get()).take() },
            Some(Ok(Err(RpcError::SystemError)))
        ));

        // New registrations should fail.
        assert!(state.register_call(NonNull::from(&slot)).is_err());
    }

    #[test]
    fn test_register_fire_and_forget() {
        let mut state = ConnectionState::new();

        let xid = state.register_fire_and_forget().unwrap();
        assert_eq!(xid, 1);

        // Slot should be FireAndForget.
        match state.pending_requests.get(&xid) {
            Some(Pending::FireAndForget) => {}
            other => panic!("expected FireAndForget, got {other:?}"),
        }

        // Poisoned state should reject registration.
        state.poisoned = true;
        assert!(state.register_fire_and_forget().is_err());
    }

    #[tokio::test]
    async fn test_deliver_reply_fire_and_forget() {
        let (handle, _peer, _peer_write) = live_pair().await;

        // Fire-and-forget call: the entry goes and the reply reports it.
        let faf_xid = handle
            .connection_state
            .lock()
            .unwrap()
            .register_fire_and_forget()
            .unwrap();
        assert!(handle.deliver_reply(faf_xid, Ok(b"ignored")));
        assert!(
            !handle
                .connection_state
                .lock()
                .unwrap()
                .pending_requests
                .contains_key(&faf_xid)
        );

        // Normal call: finished in place, not reported as fire-and-forget.
        let mut decode = |_: &[u8]| Ok(());
        // SAFETY: `decode` outlives `slot`.
        let slot = unsafe { CallSlot::new(&mut decode) };
        let normal_xid = handle
            .connection_state
            .lock()
            .unwrap()
            .register_call(NonNull::from(&slot))
            .unwrap();
        assert!(!handle.deliver_reply(normal_xid, Ok(b"hello")));
        assert!(slot.is_done());

        // Unknown xid.
        assert!(!handle.deliver_reply(9999, Ok(b"unknown")));
    }

    #[test]
    fn test_poison_counts_fire_and_forget() {
        let mut state = ConnectionState::new();

        // Register a mix of normal and fire-and-forget calls.
        let mut decode = |_: &[u8]| Ok(());
        // SAFETY: `decode` outlives `slot`.
        let slot = unsafe { CallSlot::new(&mut decode) };
        state.register_call(NonNull::from(&slot)).unwrap();
        let _faf_xid1 = state.register_fire_and_forget().unwrap();
        let _faf_xid2 = state.register_fire_and_forget().unwrap();

        let faf_count = state.poison();
        assert_eq!(faf_count, 2, "expected 2 fire-and-forget entries");
        assert!(state.pending_requests.is_empty());
    }
}
