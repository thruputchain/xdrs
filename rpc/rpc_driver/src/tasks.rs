use std::sync::Arc;

use tokio::task::JoinHandle;
use tracing::Instrument;

#[cfg(test)]
use crate::rpc_error::DEFAULT_OPAQUE_AUTH;
use crate::{
    ProgVers, RpcError,
    connection::{ConnectionHandle, RpcSender},
    context::with_peer,
    handler::{FutureSlot, ReplyBuf, RpcFuture, ServiceMap},
    rpc_msg,
    tcp_connection::ReadHalf,
};

#[derive(Debug, thiserror::Error)]
pub enum ReadTaskError {
    #[error("Shutdown")]
    Shutdown,
    #[error("Read error: {0}")]
    ReadError(#[from] std::io::Error),
    #[error("Write error: {0}")]
    WriteError(std::io::Error),
}

#[derive(Debug)]
pub(crate) struct ClientReadRunner {
    shutdown_tx: tokio::sync::watch::Sender<bool>,
    join_handle: Option<JoinHandle<()>>,
}

impl ClientReadRunner {
    pub(crate) fn begin_shutdown(&mut self) {
        let _ = self.shutdown_tx.send(true);
        if let Some(join) = &self.join_handle {
            join.abort();
        }
    }

    pub(crate) async fn shutdown(&mut self) {
        self.begin_shutdown();
        if let Some(join) = self.join_handle.take() {
            let _ = join.await;
        }
    }
}

// ---------------------------------------------------------------------------
// Read/dispatch helpers
// ---------------------------------------------------------------------------

/// Fairness budget for a continuously readable connection. This is not a
/// coalescing delay: queued output is flushed *before* yielding the worker.
const YIELD_EVERY: usize = 256;

/// Flush what the read loop has queued and give the runtime a turn, then
/// resume the burst.
async fn yield_mid_burst(writer: &Arc<crate::connection::Writer>) {
    writer.reader_waiting();
    tokio::task::yield_now().await;
    writer.reader_active();
}

/// Route an inbound REPLY message to the pending-call map.
#[inline(always)]
fn route_reply(
    client_cbs: &ConnectionHandle,
    xid: u32,
    rbody: rpc_msg::reply_body,
    payload: &[u8],
) {
    let reply = match rbody {
        rpc_msg::reply_body::MSG_ACCEPTED { areply } => match areply.reply_data {
            rpc_msg::accepted_reply_reply_data::SUCCESS { results: _results } => Ok(payload),
            reply_data => Err(reply_data.into()),
        },
        rpc_msg::reply_body::MSG_DENIED { rreply } => Err(rreply.into()),
    };
    if client_cbs.deliver_reply(xid, reply) {
        client_cbs.decrement_pending();
    }
}

/// Route an accepted SUCCESS reply whose payload is `payload`.
#[inline]
fn route_success(client_cbs: &ConnectionHandle, xid: u32, payload: &[u8]) {
    if client_cbs.deliver_reply(xid, Ok(payload)) {
        client_cbs.decrement_pending();
    }
}

/// Send a dispatch result (success or error) back to the client. Never
/// waits; an error means the connection has failed.
#[inline]
fn send_response(
    client_cbs: &ConnectionHandle,
    result: Result<Vec<u8>, ReadTaskError>,
) -> Result<(), ReadTaskError> {
    client_cbs
        .send_reply(result?)
        .map_err(ReadTaskError::WriteError)
}

/// The CALL header fields dispatch needs.
struct CallHeader {
    xid: u32,
    rpcvers: u32,
    prog: u32,
    vers: u32,
    proc: u32,
}

/// The headers nearly every record carries, read without the general decoder.
enum FastHeader {
    /// A CALL with `AUTH_NONE` credentials and verifier.
    Call(CallHeader),
    /// A REPLY accepted with SUCCESS and an `AUTH_NONE` verifier.
    Success { xid: u32 },
}

/// Recognize the common fixed-layout headers by their words; anything else
/// returns `None` for [`parse_header`] to decode. The payload starts at the
/// returned offset.
#[inline]
fn fast_header(record: &[u8]) -> Option<(FastHeader, usize)> {
    const CALL_WORDS: usize = 10;
    const REPLY_WORDS: usize = 6;
    let words = |n: usize| -> Option<[u32; CALL_WORDS]> {
        let bytes = record.get(..4 * n)?;
        let mut words = [0u32; CALL_WORDS];
        for (word, chunk) in words.iter_mut().zip(bytes.chunks_exact(4)) {
            *word = u32::from_be_bytes(chunk.try_into().unwrap());
        }
        Some(words)
    };
    let none = rpc_msg::auth_flavor::AUTH_NONE.0 as u32;
    match u32::from_be_bytes(record.get(4..8)?.try_into().unwrap()) {
        t if t == rpc_msg::msg_type::CALL.0 as u32 => {
            let [
                xid,
                _,
                rpcvers,
                prog,
                vers,
                proc,
                cred,
                cred_len,
                verf,
                verf_len,
            ] = words(CALL_WORDS)?;
            (cred == none && cred_len == 0 && verf == none && verf_len == 0).then_some((
                FastHeader::Call(CallHeader {
                    xid,
                    rpcvers,
                    prog,
                    vers,
                    proc,
                }),
                4 * CALL_WORDS,
            ))
        }
        t if t == rpc_msg::msg_type::REPLY.0 as u32 => {
            let [xid, _, stat, verf, verf_len, accept, ..] = words(REPLY_WORDS)?;
            (stat == rpc_msg::reply_stat::MSG_ACCEPTED.0 as u32
                && verf == none
                && verf_len == 0
                && accept == rpc_msg::accept_stat::SUCCESS.0 as u32)
                .then_some((FastHeader::Success { xid }, 4 * REPLY_WORDS))
        }
        _ => None,
    }
}

/// Decode the RPC header at the front of `record`. The payload starts at the
/// returned offset.
#[inline]
fn parse_header(record: &[u8]) -> Result<(rpc_msg::rpc_msg, usize), ReadTaskError> {
    let mut reader = xdrs::txs::buf_reader::BufReader::new(record);
    let msg: rpc_msg::rpc_msg = xdrs::read_xdr(&mut reader).map_err(|e| {
        tracing::error!(error = %e, "failed to parse XDR record");
        ReadTaskError::Shutdown
    })?;
    Ok((msg, reader.position()))
}

async fn run_client_read_task(
    service_map: Arc<ServiceMap>,
    client_handle: Arc<ConnectionHandle>,
    mut read_half: ReadHalf,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
) -> Result<(), ReadTaskError> {
    tokio::select! {
        _ = shutdown_rx.changed() => Ok(()),
        result = dispatch::serve(&service_map, &client_handle, &mut read_half) => result,
    }
}

pub(crate) fn spawn_client_read_task(
    service_map: Arc<ServiceMap>,
    client_handle: Arc<ConnectionHandle>,
    read_half: ReadHalf,
    span: tracing::Span,
) -> ClientReadRunner {
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let join_handle = tokio::spawn(
        async move {
            let result =
                run_client_read_task(service_map, client_handle.clone(), read_half, shutdown_rx)
                    .await;
            if let Err(e) = result {
                tracing::error!(error = %e, "client read task failed");
                let _shutdown = client_handle.begin_poison();
            }
        }
        .instrument(span),
    );
    ClientReadRunner {
        shutdown_tx,
        join_handle: Some(join_handle),
    }
}

pub(crate) mod dispatch {
    use super::*;
    use crate::tcp_connection::TryRecord;
    use std::future::Future;
    use std::panic::AssertUnwindSafe;
    use std::pin::Pin;
    use std::task::{Context, Poll, Waker};

    // -- Panic-catching future wrapper (replaces futures-util dependency) --

    /// Wraps a future so that panics during `poll` are caught and returned
    /// as `Err` instead of unwinding the task.  Same approach as
    /// `futures_util::FutureExt::catch_unwind` but with zero dependencies.
    ///
    /// `AssertUnwindSafe` is applied internally so the connection machinery
    /// can recover from a handler panic. Handlers remain responsible for the
    /// consistency of their own state if they panic while mutating it.
    #[allow(unreachable_pub)]
    pub struct CatchUnwind<F>(pub F);

    impl<F: std::future::Future> std::future::Future for CatchUnwind<F> {
        type Output = Result<F::Output, Box<dyn std::any::Any + Send>>;

        fn poll(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Self::Output> {
            // SAFETY: structural pin projection — CatchUnwind is a newtype
            // over F and never moves F after pinning.
            let inner = unsafe { self.map_unchecked_mut(|s| &mut s.0) };
            std::panic::catch_unwind(AssertUnwindSafe(|| inner.poll(cx)))?.map(Ok)
        }
    }

    // -- Handler futures -----------------------------------------------------

    type HandlerResult = Result<Vec<u8>, ReadTaskError>;
    type InvocationResult = Result<ReplyBuf, RpcError>;
    type Panic = Box<dyn std::any::Any + Send>;

    /// Owns one CALL's handler future, which lives in a [`FutureSlot`]'s heap
    /// storage and so never moves when this guard moves into a task.
    pub(super) struct HandlerFuture {
        future: Option<RpcFuture<'static>>,
        xid: u32,
        /// Storage recovered from a completed future, for the next call.
        spare: Option<FutureSlot>,
    }

    /// Detach an invocation from its borrow of the service map.
    ///
    /// The read loop holds the map for every inline poll and drop, and a
    /// suspended handler's task owns a reference to it in [`Detached`], so
    /// the map outlives the future without a per-call reference count.
    fn detach(future: RpcFuture<'_>) -> RpcFuture<'static> {
        // SAFETY: see above; only the lifetime changes.
        unsafe { std::mem::transmute(future) }
    }

    impl HandlerFuture {
        /// Decode and probe under one unwind boundary. Only an invocation
        /// that suspends survives this call.
        fn start(
            xid: u32,
            dispatch: impl FnOnce() -> Result<RpcFuture<'static>, RpcError>,
            cx: &mut Context<'_>,
        ) -> (Self, Poll<HandlerResult>) {
            let mut handler = Self {
                future: None,
                xid,
                spare: None,
            };
            let result = std::panic::catch_unwind(AssertUnwindSafe(|| match dispatch() {
                Ok(future) => Pin::new(handler.future.insert(future)).poll(cx),
                Err(error) => Poll::Ready(Err(error)),
            }));
            let result = handler.finish_poll(result);
            (handler, result)
        }

        fn finish_poll(
            &mut self,
            result: Result<Poll<InvocationResult>, Panic>,
        ) -> Poll<HandlerResult> {
            let result = match result {
                Ok(Poll::Pending) => return Poll::Pending,
                Ok(Poll::Ready(result)) => Ok(result),
                Err(panic) => Err(panic),
            };
            let response = handler_outcome(result, self.xid);
            // Drop while still in the peer/span context, keeping its storage
            // for the next call. A destructor panic overrides a successful
            // result, just as a polling panic does.
            if let Some(future) = self.future.take() {
                match retire_handler(future, self.xid) {
                    Some(slot) => self.spare = Some(slot),
                    None => {
                        return Poll::Ready(Ok(crate::rpc_error::to_error_record(
                            &crate::RpcError::SystemError,
                            self.xid,
                        )));
                    }
                }
            }
            Poll::Ready(response)
        }
    }

    impl Future for HandlerFuture {
        type Output = HandlerResult;

        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
                Pin::new(self.future.as_mut().expect("polled after completion")).poll(cx)
            }));
            self.finish_poll(result)
        }
    }

    impl Drop for HandlerFuture {
        fn drop(&mut self) {
            // Cancellation and admission rejection also run user destructors.
            if let Some(future) = self.future.take() {
                drop_handler(future, self.xid);
            }
        }
    }

    fn discard_panic_payload(panic: Panic, xid: u32, context: &'static str) {
        if let Err(drop_panic) = std::panic::catch_unwind(AssertUnwindSafe(|| drop(panic))) {
            tracing::error!(xid, context, "panic payload destructor panicked");
            // Dropping a second malicious panic payload could unwind the
            // connection task again. Leak only on this pathological path.
            std::mem::forget(drop_panic);
        }
    }

    /// Frame a handler's reply, or collapse its error or panic into an
    /// error reply.
    pub(super) fn handler_outcome(
        result: Result<InvocationResult, Panic>,
        xid: u32,
    ) -> HandlerResult {
        match result {
            Ok(Ok(reply)) => frame_reply(reply, xid),
            Ok(Err(error)) => Ok(crate::rpc_error::to_error_record(&error, xid)),
            Err(panic) => {
                tracing::error!(xid, "handler panicked");
                discard_panic_payload(panic, xid, "handler panic");
                Ok(crate::rpc_error::to_error_record(
                    &crate::RpcError::SystemError,
                    xid,
                ))
            }
        }
    }

    /// Drop a finished handler and recover its storage, unless its
    /// destructor panics.
    fn retire_handler(handler: RpcFuture<'_>, xid: u32) -> Option<FutureSlot> {
        match std::panic::catch_unwind(AssertUnwindSafe(|| handler.into_slot())) {
            Ok(slot) => Some(slot),
            Err(panic) => {
                tracing::error!(xid, "handler destructor panicked");
                discard_panic_payload(panic, xid, "handler destructor panic");
                None
            }
        }
    }

    pub(super) fn drop_handler(handler: RpcFuture<'_>, xid: u32) -> bool {
        match std::panic::catch_unwind(AssertUnwindSafe(|| drop(handler))) {
            Ok(()) => false,
            Err(panic) => {
                tracing::error!(xid, "handler destructor panicked");
                discard_panic_payload(panic, xid, "handler destructor panic");
                true
            }
        }
    }

    // -- Suspended handlers --------------------------------------------------

    /// A read loop's suspended handler tasks: counted for the admission
    /// cap and cancelled together when the loop ends, without a `JoinSet`'s
    /// per-task node or per-record reaping.
    #[derive(Default)]
    struct Suspended {
        running: std::sync::atomic::AtomicUsize,
        closed: std::sync::atomic::AtomicBool,
        cancel: tokio::sync::Notify,
    }

    impl Suspended {
        /// Run `work` until it finishes or the read loop ends.
        async fn until_closed(&self, work: impl Future<Output = ()>) {
            let cancelled = self.cancel.notified();
            tokio::pin!(cancelled);
            // Register before checking the flag so a close in between is seen.
            cancelled.as_mut().enable();
            if self.closed.load(std::sync::atomic::Ordering::Acquire) {
                return;
            }
            tokio::select! {
                biased;
                () = cancelled => {}
                () = work => {}
            }
        }
    }

    /// Cancels a read loop's suspended handlers when the loop returns or is
    /// dropped.
    struct CancelOnExit(Arc<Suspended>);

    impl Drop for CancelOnExit {
        fn drop(&mut self) {
            self.0
                .closed
                .store(true, std::sync::atomic::Ordering::Release);
            self.0.cancel.notify_waiters();
        }
    }

    /// A suspended handler's task, owning the service map its future
    /// borrows. Counted until the task is gone, whether it finished, was
    /// cancelled, or was dropped before its first poll. Fields drop in
    /// declaration order, so the handler goes before the map.
    struct Detached<F> {
        future: F,
        _services: Arc<ServiceMap>,
        suspended: Arc<Suspended>,
    }

    impl<F> Drop for Detached<F> {
        fn drop(&mut self) {
            self.suspended
                .running
                .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
        }
    }

    impl<F: Future> Future for Detached<F> {
        type Output = F::Output;

        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<F::Output> {
            // SAFETY: structural pin projection; `future` is never moved.
            unsafe { self.map_unchecked_mut(|d| &mut d.future) }.poll(cx)
        }
    }

    // -- Read loop -----------------------------------------------------------

    /// Ready handlers complete inline; only suspended handlers become tasks.
    /// The reader never waits for a handler or for output capacity, so it can
    /// always route a bidirectional reply while the connection remains open.
    pub(super) async fn serve(
        service_map: &Arc<ServiceMap>,
        client_cbs: &Arc<ConnectionHandle>,
        read_half: &mut ReadHalf,
    ) -> Result<(), ReadTaskError> {
        // One peer scope for the whole loop: calls that complete inline need
        // no per-call scope or sender clone.
        let sender: Arc<dyn RpcSender> = client_cbs.clone();
        with_peer(sender, serve_inner(service_map, client_cbs, read_half)).await
    }

    async fn serve_inner(
        service_map: &Arc<ServiceMap>,
        client_cbs: &Arc<ConnectionHandle>,
        read_half: &mut ReadHalf,
    ) -> Result<(), ReadTaskError> {
        let suspended = Arc::new(Suspended::default());
        let _cancel_on_exit = CancelOnExit(suspended.clone());
        // Fast-path hit rate, reported once on exit. Loop-local: this task is
        // the only writer.
        let (mut ready_calls, mut suspended_calls) = (0u64, 0u64);
        let writer = &client_cbs.writer;
        writer.reader_active();
        let mut since_yield = 0usize;
        // Storage for the next handler future. Inline completions hand it
        // back; a suspended handler takes it along to its task.
        let mut spare_slot = None;

        let result = loop {
            if since_yield >= YIELD_EVERY {
                since_yield = 0;
                yield_mid_burst(writer).await;
            }

            let record = match read_half.try_read_record() {
                Ok(TryRecord::Ready(record)) => {
                    since_yield += 1;
                    record
                }
                Ok(TryRecord::Invalid) => break Err(ReadTaskError::Shutdown),
                Ok(TryRecord::WouldBlock) => {
                    since_yield = 0;
                    // Out of buffered input: flush every reply queued during
                    // this pass, then park on the socket.
                    writer.reader_waiting();
                    if let Err(e) = read_half.readable().await {
                        break Err(ReadTaskError::ReadError(e));
                    }
                    writer.reader_active();
                    continue;
                }
                Err(e) => break Err(ReadTaskError::ReadError(e)),
            };

            let (call, payload_offset) = match fast_header(record) {
                Some((FastHeader::Success { xid }, offset)) => {
                    route_success(client_cbs, xid, &record[offset..]);
                    continue;
                }
                Some((FastHeader::Call(call), offset)) => (call, offset),
                None => match parse_header(record) {
                    Ok((msg, offset)) => match msg.body {
                        rpc_msg::rpc_msg_body::REPLY { rbody } => {
                            route_reply(client_cbs, msg.xid, rbody, &record[offset..]);
                            continue;
                        }
                        rpc_msg::rpc_msg_body::CALL { cbody } => (
                            CallHeader {
                                xid: msg.xid,
                                rpcvers: cbody.rpcvers,
                                prog: cbody.prog,
                                vers: cbody.vers,
                                proc: cbody.proc,
                            },
                            offset,
                        ),
                    },
                    Err(e) => break Err(e),
                },
            };
            let xid = call.xid;

            // Refuse new work before invoking its handler. Both successful and
            // error replies always bypass this threshold; never block routing
            // an incoming reply behind output capacity.
            if !writer.accepts_inbound_call() {
                if let Err(e) = send_response(
                    client_cbs,
                    Ok(crate::rpc_error::to_error_record(
                        &crate::RpcError::SystemError,
                        xid,
                    )),
                ) {
                    break Err(e);
                }
                continue;
            }

            let vers = ProgVers::new(call.prog, call.vers);
            let proc = call.proc;

            // Preserve the connection span when a suspended handler is spawned.
            let span = tracing::debug_span!(
                parent: &client_cbs.connection_span,
                "rpc_call",
                xid,
                prog = vers.prog(),
                vers = vers.vers(),
                proc = proc,
                ready = tracing::field::Empty,
            );

            // Decode straight from the receive buffer, then probe the owned
            // invocation. Both run in the read loop's peer scope.
            //
            // The no-op waker is safe because a `Pending` handler is either
            // cancelled by the cap or polled again by the task below, and the
            // `Future` contract requires it to re-register the waker it is
            // given on each poll.
            let slot = spare_slot.take().unwrap_or_else(FutureSlot::with_block);
            let (mut handler, probe) = {
                let _enter = span.enter();
                HandlerFuture::start(
                    xid,
                    || {
                        service_map
                            .dispatch(
                                call.rpcvers,
                                vers,
                                proc,
                                &record[payload_offset..],
                                ReplyBuf::from_spare(crate::connection::take_buffer()),
                                slot,
                            )
                            .map(detach)
                    },
                    &mut Context::from_waker(Waker::noop()),
                )
            };
            spare_slot = handler.spare.take();

            if let Poll::Ready(result) = probe {
                ready_calls += 1;
                span.record("ready", true);
                let enqueued = {
                    let _enter = span.enter();
                    send_response(client_cbs, result)
                };
                if let Err(e) = enqueued {
                    break Err(e);
                }
                continue;
            }

            suspended_calls += 1;
            span.record("ready", false);

            if let Some(limit) = client_cbs.max_suspended_handlers
                && suspended.running.load(std::sync::atomic::Ordering::Relaxed) >= limit
            {
                {
                    let _enter = span.enter();
                    drop(handler);
                }
                if let Err(e) = send_response(
                    client_cbs,
                    Ok(crate::rpc_error::to_error_record(
                        &crate::RpcError::SystemError,
                        xid,
                    )),
                ) {
                    break Err(e);
                }
                continue;
            }

            let cbs = client_cbs.clone();
            // A suspended handler leaves the read loop's scope, so its task
            // installs the peer itself.
            let sender: Arc<dyn RpcSender> = client_cbs.clone();
            let work = with_peer(sender, async move {
                let result = handler.await;
                if let Err(e) = send_response(&cbs, result) {
                    tracing::error!(xid, error = %e, "response write failed");
                    let _shutdown = cbs.begin_poison();
                }
            });
            let task_suspended = suspended.clone();
            suspended
                .running
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            tokio::spawn(Detached {
                future: async move { task_suspended.until_closed(work).await }.instrument(span),
                _services: service_map.clone(),
                suspended: suspended.clone(),
            });
        };

        tracing::debug!(ready_calls, suspended_calls, "read loop exiting");
        result
    }
}

/// Runs the concurrent read-dispatch loop. Returns only on error or
/// connection close. Ending or dropping the loop cancels any still-running
/// handler tasks.
pub async fn launch_read_task(
    service_map: Arc<ServiceMap>,
    client_cbs: Arc<ConnectionHandle>,
    mut read_half: ReadHalf,
) -> Result<(), ReadTaskError> {
    dispatch::serve(&service_map, &client_cbs, &mut read_half).await
}

// ---------------------------------------------------------------------------
// Response serialization helpers
// ---------------------------------------------------------------------------

/// Frame a successful reply in place.
///
/// Fills the prefix a [`ReplyBuf`] reserves with the
/// record mark and the RFC 5531 accepted-SUCCESS header (`AUTH_NONE`
/// verifier), and returns the buffer as the complete single-part record.
/// The payload is neither copied nor moved.
#[inline]
fn frame_reply(reply: ReplyBuf, xid: u32) -> Result<Vec<u8>, ReadTaskError> {
    const REPLY_HEADER_WORDS: usize = 6;
    const { assert!(ReplyBuf::PREFIX_LEN == 4 + 4 * REPLY_HEADER_WORDS) };

    let mut record = reply.into_vec();
    let body_len = record.len() - 4;
    let Some(msg_len) = u32::try_from(body_len)
        .ok()
        .filter(|&len| len <= 0x7FFF_FFFF)
    else {
        tracing::warn!(results_len = body_len, "message too large");
        return Err(ReadTaskError::Shutdown);
    };
    let words: [u32; 1 + REPLY_HEADER_WORDS] = [
        msg_len | 0x8000_0000,
        xid,
        rpc_msg::msg_type::REPLY.0 as u32,
        rpc_msg::reply_stat::MSG_ACCEPTED.0 as u32,
        rpc_msg::auth_flavor::AUTH_NONE.0 as u32, // verf.flavor
        0,                                        // verf.body length
        rpc_msg::accept_stat::SUCCESS.0 as u32,
    ];
    for (slot, word) in record[..ReplyBuf::PREFIX_LEN]
        .chunks_exact_mut(4)
        .zip(words)
    {
        slot.copy_from_slice(&word.to_be_bytes());
    }
    Ok(record)
}

#[cfg(test)]
fn get_success_header(xid: u32) -> rpc_msg::rpc_msg {
    rpc_msg::rpc_msg {
        xid,
        body: rpc_msg::rpc_msg_body::REPLY {
            rbody: rpc_msg::reply_body::MSG_ACCEPTED {
                areply: rpc_msg::accepted_reply {
                    verf: DEFAULT_OPAQUE_AUTH,
                    reply_data: rpc_msg::accepted_reply_reply_data::SUCCESS { results: [0u8; 0] },
                },
            },
        },
    }
}

#[cfg(test)]
fn prepare_success_response(
    buf: Vec<u8>,
    xid: u32,
    results: &[u8],
) -> Result<Vec<u8>, ReadTaskError> {
    let header = get_success_header(xid);
    crate::connection::frame_rpc_record(buf, &header, results).map_err(|()| {
        tracing::warn!(results_len = results.len(), "message too large");
        ReadTaskError::Shutdown
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ReadyTestService {
        entered: tokio::sync::mpsc::UnboundedSender<u32>,
    }

    impl crate::handler::RpcHandler for ReadyTestService {
        fn dispatch(
            &self,
            proc: u32,
            data: &[u8],
            reply: ReplyBuf,
            slot: FutureSlot,
        ) -> Result<RpcFuture<'_>, RpcError> {
            self.entered.send(proc).unwrap();
            let reply = reply.with(data);
            Ok(slot.put(async move { Ok(reply) }))
        }

        fn prog_vers(&self) -> crate::ProgVers {
            crate::ProgVers::new(1, 1)
        }
    }

    /// A framed CALL record for `proc` carrying `payload`.
    fn test_call_record(xid: u32, proc: u32, payload: &[u8]) -> Vec<u8> {
        let header = rpc_msg::rpc_msg {
            xid,
            body: rpc_msg::rpc_msg_body::CALL {
                cbody: rpc_msg::call_body {
                    rpcvers: 2,
                    prog: 1,
                    vers: 1,
                    proc,
                    cred: DEFAULT_OPAQUE_AUTH,
                    verf: DEFAULT_OPAQUE_AUTH,
                },
            },
        };
        crate::connection::frame_rpc_record(Vec::new(), &header, payload).unwrap()
    }

    #[test]
    fn fast_header_agrees_with_the_decoder_or_defers_to_it() {
        // AUTH_NONE CALL.
        let call = test_call_record(7, 3, b"args");
        let (decoded, offset) = parse_header(&call[4..]).unwrap();
        let Some((FastHeader::Call(fast), fast_offset)) = fast_header(&call[4..]) else {
            panic!("AUTH_NONE call should take the fast path");
        };
        let rpc_msg::rpc_msg_body::CALL { cbody } = decoded.body else {
            panic!("decoded a reply");
        };
        assert_eq!(
            (
                fast.xid,
                fast.rpcvers,
                fast.prog,
                fast.vers,
                fast.proc,
                fast_offset
            ),
            (
                decoded.xid,
                cbody.rpcvers,
                cbody.prog,
                cbody.vers,
                cbody.proc,
                offset
            )
        );
        assert_eq!(&call[4 + fast_offset..], b"args");

        // Accepted SUCCESS REPLY.
        let reply = prepare_success_response(Vec::new(), 9, b"results").unwrap();
        let (decoded, offset) = parse_header(&reply[4..]).unwrap();
        let Some((FastHeader::Success { xid }, fast_offset)) = fast_header(&reply[4..]) else {
            panic!("SUCCESS reply should take the fast path");
        };
        assert_eq!((xid, fast_offset), (decoded.xid, offset));
        assert_eq!(&reply[4 + fast_offset..], b"results");

        // Error replies and non-AUTH_NONE credentials take the decoder.
        let error = crate::rpc_error::to_error_record(&crate::RpcError::InvalidProcedure, 5);
        assert!(fast_header(&error[4..]).is_none());
        let mut sys_cred = call.clone();
        sys_cred[4 + 24..4 + 28].copy_from_slice(&1u32.to_be_bytes());
        assert!(fast_header(&sys_cred[4..]).is_none());
        assert!(fast_header(&call[4..20]).is_none());
    }

    #[tokio::test]
    async fn client_read_runner_begin_shutdown_aborts_stuck_task() {
        let (shutdown_tx, _shutdown_rx) = tokio::sync::watch::channel(false);
        let join_handle = tokio::spawn(async {
            std::future::pending::<()>().await;
        });
        let mut runner = ClientReadRunner {
            shutdown_tx,
            join_handle: Some(join_handle),
        };

        runner.begin_shutdown();

        tokio::time::timeout(std::time::Duration::from_millis(100), runner.shutdown())
            .await
            .expect("shutdown should complete after begin_shutdown aborts the task");
    }

    #[tokio::test]
    async fn output_admission_rejects_before_dispatch_and_preserves_reply_routing() {
        use std::future::Future;
        use std::task::Poll;
        use tokio::io::AsyncWriteExt;

        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let config = crate::ConnectionConfig::default().with_output_admission_threshold_bytes(
                Some(crate::CallBuf::new().into_vec().capacity()),
            );
            let (client, mut read_half, peer) =
                ConnectionHandle::new_stalled_for_test(&config).await;
            let client = Arc::new(client);
            let (peer_read, mut peer_write) = peer.into_split();
            let mut peer = ReadHalf::new_for_test(peer_read);

            // Start a callback that must still receive its reply while output
            // admission refuses new inbound work.
            let mut pending =
                std::pin::pin!(client.send_call(ProgVers::new(1, 1), 0, crate::CallBuf::new(),));
            std::future::poll_fn(|cx| {
                assert!(pending.as_mut().poll(cx).is_pending());
                Poll::Ready(())
            })
            .await;
            client.writer.unstall_for_test();
            let call = peer.read_record().await.unwrap().unwrap();
            let xid = u32::from_be_bytes(call[..4].try_into().unwrap());

            client.writer.stall_for_test();
            let reply =
                prepare_success_response(Vec::with_capacity(1024), 777, b"accepted").unwrap();
            client.send_reply(reply).unwrap();
            assert!(!client.writer.accepts_inbound_call());

            let (entered_tx, mut entered_rx) = tokio::sync::mpsc::unbounded_channel();
            let mut services = ServiceMap::new();
            services.add_service(Box::new(ReadyTestService {
                entered: entered_tx,
            }));
            let services = Arc::new(services);
            let serve_client = client.clone();
            let serve = tokio::spawn(async move {
                dispatch::serve(&services, &serve_client, &mut read_half).await
            });

            let mut burst = test_call_record(1, 1, b"refused");
            burst.extend(test_call_record(2, 2, b"also refused"));
            burst.extend(prepare_success_response(Vec::new(), xid, b"callback completed").unwrap());
            peer_write.write_all(&burst).await.unwrap();
            assert_eq!(pending.await.unwrap(), b"callback completed");
            assert!(
                entered_rx.try_recv().is_err(),
                "refused handlers must never run"
            );
            assert_eq!(client.writer.queued_for_test(), 3);
            assert!(!client.is_poisoned());

            client.writer.unstall_for_test();
            let record = peer.read_record().await.unwrap().unwrap();
            assert_eq!(
                record,
                &prepare_success_response(Vec::new(), 777, b"accepted").unwrap()[4..]
            );
            for xid in [1, 2] {
                let record = peer.read_record().await.unwrap().unwrap();
                assert_eq!(
                    record,
                    &crate::rpc_error::to_error_record(&crate::RpcError::SystemError, xid)[4..]
                );
            }

            // The same connection resumes dispatch once output drains.
            peer_write
                .write_all(&test_call_record(3, 3, b"resumed"))
                .await
                .unwrap();
            let record = peer.read_record().await.unwrap().unwrap();
            assert_eq!(
                record,
                &prepare_success_response(Vec::new(), 3, b"resumed").unwrap()[4..]
            );
            assert_eq!(entered_rx.recv().await, Some(3));
            peer_write.shutdown().await.unwrap();
            assert!(serve.await.unwrap().is_err());
        })
        .await
        .expect("output admission blocked replies or dropped the connection");
    }

    /// One socket write carrying `calls` ready echo CALLs, with the
    /// current-thread runtime. Every call must be served without an
    /// admission cap or an explicit dispatch-mode setting.
    #[tokio::test]
    async fn a_large_burst_of_ready_handlers_is_fully_served() {
        use tokio::io::AsyncWriteExt;
        let calls = 1000u32;
        let config = crate::ConnectionConfig::default();
        let (client, mut read_half, peer) = ConnectionHandle::new_stalled_for_test(&config).await;
        let client = Arc::new(client);
        client.writer.unstall_for_test();
        let (peer_read, mut peer_write) = peer.into_split();
        let mut peer = ReadHalf::new_for_test(peer_read);
        let burst: Vec<u8> = (1..=calls)
            .flat_map(|xid| test_call_record(xid, xid, &xid.to_be_bytes()))
            .collect();
        peer_write.write_all(&burst).await.unwrap();

        let (entered_tx, mut entered_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut services = ServiceMap::new();
        services.add_service(Box::new(ReadyTestService {
            entered: entered_tx,
        }));
        let services = Arc::new(services);
        let serve_client = client.clone();
        let serve =
            tokio::spawn(
                async move { dispatch::serve(&services, &serve_client, &mut read_half).await },
            );

        let mut ok = 0;
        let mut shed = 0;
        for _ in 0..calls {
            let record =
                tokio::time::timeout(std::time::Duration::from_secs(10), peer.read_record())
                    .await
                    .expect("burst stalled")
                    .unwrap()
                    .unwrap();
            match u32::from_be_bytes(record[20..24].try_into().unwrap()) {
                0 => ok += 1,
                _ => shed += 1,
            }
        }
        assert_eq!((ok, shed), (calls, 0), "shed {shed} of {calls} calls");
        let entered = std::iter::from_fn(|| entered_rx.try_recv().ok()).count();
        assert_eq!(entered as u32, calls);
        let (writes, largest_batch) = client.writer.write_stats();
        eprintln!(
            "{calls} ready replies: {writes} write attempts, largest writev {largest_batch} records"
        );
        assert!(
            largest_batch > 1,
            "ready replies were not batched with writev"
        );
        assert!(writes < calls as usize, "one write per reply: {writes}");

        peer_write.shutdown().await.unwrap();
        assert!(matches!(
            serve.await.unwrap(),
            Err(ReadTaskError::ReadError(_))
        ));
    }

    #[tokio::test]
    async fn completed_handler_slot_is_reused_after_reader_yields() {
        use tokio::io::AsyncWriteExt;

        struct YieldingService;
        impl crate::handler::RpcHandler for YieldingService {
            fn dispatch(
                &self,
                proc: u32,
                data: &[u8],
                reply: ReplyBuf,
                slot: FutureSlot,
            ) -> Result<RpcFuture<'_>, RpcError> {
                let reply = reply.with(data);
                Ok(slot.put(async move {
                    if proc == 1 {
                        tokio::task::yield_now().await;
                    }
                    Ok(reply)
                }))
            }
            fn prog_vers(&self) -> ProgVers {
                ProgVers::new(1, 1)
            }
        }

        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let config = crate::ConnectionConfig::default().with_max_suspended_handlers(Some(1));
            let (client, mut read_half, peer) =
                ConnectionHandle::new_stalled_for_test(&config).await;
            let client = Arc::new(client);
            client.writer.unstall_for_test();
            let (peer_read, mut peer_write) = peer.into_split();
            let mut peer = ReadHalf::new_for_test(peer_read);
            let mut services = ServiceMap::new();
            services.add_service(Box::new(YieldingService));
            let services = Arc::new(services);

            // The first handler finishes during the fairness yield after 256
            // records. The next suspended handler must use its released slot.
            let burst: Vec<_> = (0..=YIELD_EVERY)
                .flat_map(|xid| {
                    test_call_record(xid as u32, u32::from(xid == 0 || xid == YIELD_EVERY), &[])
                })
                .collect();
            peer_write.write_all(&burst).await.unwrap();
            let serve =
                tokio::spawn(
                    async move { dispatch::serve(&services, &client, &mut read_half).await },
                );
            for _ in 0..=YIELD_EVERY {
                let record = peer.read_record().await.unwrap().unwrap();
                assert_eq!(
                    u32::from_be_bytes(record[20..24].try_into().unwrap()),
                    0,
                    "completed tasks must not cause SYSTEM_ERR"
                );
            }
            peer_write.shutdown().await.unwrap();
            assert!(serve.await.unwrap().is_err());
        })
        .await
        .expect("reader yield failed to release the handler slot");
    }

    #[tokio::test]
    async fn ending_the_read_loop_cancels_suspended_handlers() {
        use tokio::io::AsyncWriteExt;

        struct DropFlag(Option<tokio::sync::oneshot::Sender<()>>);
        impl Drop for DropFlag {
            fn drop(&mut self) {
                let _ = self.0.take().unwrap().send(());
            }
        }

        struct StuckService(std::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>);
        impl crate::handler::RpcHandler for StuckService {
            fn dispatch(
                &self,
                _proc: u32,
                _data: &[u8],
                _reply: ReplyBuf,
                slot: FutureSlot,
            ) -> Result<RpcFuture<'_>, RpcError> {
                Ok(slot.put(async move {
                    let _flag = DropFlag(self.0.lock().unwrap().take());
                    std::future::pending().await
                }))
            }
            fn prog_vers(&self) -> ProgVers {
                ProgVers::new(1, 1)
            }
        }

        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let (dropped_tx, dropped_rx) = tokio::sync::oneshot::channel();
            let config = crate::ConnectionConfig::default();
            let (client, mut read_half, peer) =
                ConnectionHandle::new_stalled_for_test(&config).await;
            let client = Arc::new(client);
            client.writer.unstall_for_test();
            let (_peer_read, mut peer_write) = peer.into_split();
            let mut services = ServiceMap::new();
            services.add_service(Box::new(StuckService(std::sync::Mutex::new(Some(
                dropped_tx,
            )))));
            let services = Arc::new(services);

            peer_write
                .write_all(&test_call_record(1, 0, &[]))
                .await
                .unwrap();
            let serve =
                tokio::spawn(
                    async move { dispatch::serve(&services, &client, &mut read_half).await },
                );
            // Give the handler time to suspend into its own task.
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            peer_write.shutdown().await.unwrap();
            assert!(serve.await.unwrap().is_err());
            dropped_rx.await.unwrap();
        })
        .await
        .expect("a suspended handler outlived its read loop");
    }

    #[test]
    fn panic_payload_destructor_cannot_unwind_dispatch() {
        struct PanicOnDrop;
        impl Drop for PanicOnDrop {
            fn drop(&mut self) {
                panic!("panic payload destructor");
            }
        }

        let result = dispatch::handler_outcome(Err(Box::new(PanicOnDrop)), 7);
        assert_eq!(
            result.unwrap(),
            crate::rpc_error::to_error_record(&crate::RpcError::SystemError, 7)
        );
    }

    #[test]
    fn ready_handler_destructor_cannot_unwind_dispatch() {
        struct ReadyPanicOnDrop;
        impl std::future::Future for ReadyPanicOnDrop {
            type Output = Result<ReplyBuf, RpcError>;

            fn poll(
                self: std::pin::Pin<&mut Self>,
                _cx: &mut std::task::Context<'_>,
            ) -> std::task::Poll<Self::Output> {
                std::task::Poll::Ready(Ok(ReplyBuf::new()))
            }
        }
        impl Drop for ReadyPanicOnDrop {
            fn drop(&mut self) {
                panic!("ready handler destructor");
            }
        }

        let handler = FutureSlot::with_block().put(ReadyPanicOnDrop);
        assert!(dispatch::drop_handler(handler, 8));
    }
}
