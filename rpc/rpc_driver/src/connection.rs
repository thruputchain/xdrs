//! Connection state and RPC call handling.

use std::collections::HashMap;
use std::io;
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
        let len = self.0.lock().map(|v| v.len()).unwrap_or(0);
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
    let mut slices: Vec<io::IoSlice<'_>> = msgs.iter().map(|m| io::IoSlice::new(m)).collect();
    let mut remaining: &mut [io::IoSlice<'_>] = &mut slices;
    let mut total: usize = msgs.iter().map(|m| m.len()).sum();

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
    /// Send an RPC call and return the response data.
    async fn send_call(
        &self,
        progvers: ProgVers,
        proc: u32,
        data: &[u8],
    ) -> Result<Vec<u8>, RpcError>;
}

#[async_trait]
impl RpcSender for ConnectionHandle {
    async fn send_call(
        &self,
        progvers: ProgVers,
        proc: u32,
        data: &[u8],
    ) -> Result<Vec<u8>, RpcError> {
        ConnectionHandle::send_call(self, progvers, proc, data).await
    }
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
    Ready(Result<Vec<u8>, RpcError>),
}

impl std::fmt::Debug for CallbackSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Registered => write!(f, "Registered"),
            Self::Waiting(_) => write!(f, "Waiting(..)"),
            Self::Ready(r) => write!(f, "Ready({r:?})"),
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

    pub(crate) fn handle_reply(&mut self, xid: u32, reply: Result<Vec<u8>, RpcError>) {
        if let Some(slot) = self.pending_requests.get_mut(&xid) {
            let prev = std::mem::replace(slot, CallbackSlot::Ready(reply));
            if let CallbackSlot::Waiting(waker) = prev {
                waker.wake();
            }
            return;
        }
        tracing::warn!(xid, "received reply for unknown xid");
    }

    pub(crate) fn poison(&mut self) {
        self.poisoned = true;
        for (_xid, slot) in self.pending_requests.drain() {
            if let CallbackSlot::Waiting(waker) = slot {
                waker.wake();
            }
        }
    }
}

impl Drop for ConnectionState {
    fn drop(&mut self) {
        self.poison();
    }
}

/// Handle for making RPC calls over an established connection.
///
/// Manages pending requests and provides methods for sending calls
/// and graceful shutdown via poisoning.
#[derive(Debug)]
pub struct ConnectionHandle {
    pub(crate) connection_state: StdMutex<ConnectionState>,
    write_tx: mpsc::Sender<Vec<u8>>,
    writer_handle: StdMutex<Option<JoinHandle<()>>>,
    pub(crate) buf_pool: Arc<BufPool>,
    pub(crate) connection_span: tracing::Span,
    #[cfg_attr(not(feature = "concurrent-dispatch"), allow(dead_code))]
    pub(crate) max_concurrent_dispatches: usize,
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
            writer_handle: StdMutex::new(Some(handle)),
            buf_pool: pool,
            connection_span: span,
            max_concurrent_dispatches: config.max_concurrent_dispatches,
        }
    }

    /// Poison the connection, failing all pending requests and closing the write half.
    pub async fn poison(&self) {
        // Fail all pending requests. Wakes callers parked in poll_fn,
        // which will see the entry removed and return Err(SystemError).
        self.connection_state.lock().unwrap().poison();

        // Abort the writer task and wait for it to finish. This drops the
        // WriteHalf (TCP FIN) and the channel Receiver. Any messages still
        // in the channel are discarded — their waiters were already resolved
        // above.
        let handle = self.writer_handle.lock().unwrap().take();
        if let Some(h) = handle {
            h.abort();
            let _ = h.await;
        }
    }

    /// Send a pre-serialized message through the write channel.
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
    ) -> Result<Vec<u8>, RpcError> {
        let span = tracing::debug_span!(
            parent: &self.connection_span,
            "rpc_call",
            xid = tracing::field::Empty,
            prog = progvers.prog,
            vers = progvers.vers,
            proc = proc,
        );

        let xid = self.connection_state.lock().unwrap().register_call()?;

        span.record("xid", xid);

        // Guard removes the pending entry on drop (cancellation safety).
        let guard = CallGuard {
            state: &self.connection_state,
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

            // Wait for the reader task to deliver the result into our slot.
            std::future::poll_fn(|cx| {
                let mut state = self.connection_state.lock().unwrap();
                match state.pending_requests.get_mut(&xid) {
                    Some(CallbackSlot::Ready(_)) => match state.pending_requests.remove(&xid) {
                        Some(CallbackSlot::Ready(result)) => std::task::Poll::Ready(result),
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
        .instrument(span)
        .await;

        guard.disarm();
        result
    }

    /// Returns the number of pending (in-flight) requests.
    pub fn num_pending_requests(&self) -> usize {
        self.connection_state.lock().unwrap().pending_requests.len()
    }
}

/// Drop guard for cancellation safety in `send_call`.
///
/// If the future is dropped before the response arrives, this removes
/// the pending entry from the HashMap so it doesn't leak.
struct CallGuard<'a> {
    state: &'a StdMutex<ConnectionState>,
    xid: u32,
    done: bool,
}

impl CallGuard<'_> {
    fn disarm(mut self) {
        self.done = true;
    }
}

impl Drop for CallGuard<'_> {
    fn drop(&mut self) {
        if !self.done {
            self.state
                .lock()
                .unwrap()
                .pending_requests
                .remove(&self.xid);
        }
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
        state.handle_reply(0, Ok(b"world".to_vec()));

        // Reply for our xid.
        state.handle_reply(xid, Ok(b"hello".to_vec()));

        // Slot should now be Ready.
        match state.pending_requests.remove(&xid) {
            Some(CallbackSlot::Ready(Ok(data))) => assert_eq!(data, b"hello"),
            other => panic!("expected Ready(Ok), got {other:?}"),
        }

        // Duplicate reply — entry already removed, should be ignored.
        state.handle_reply(xid, Ok(b"foo".to_vec()));
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
}
