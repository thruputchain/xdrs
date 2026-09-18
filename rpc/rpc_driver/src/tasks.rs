use std::sync::Arc;

use tokio::task::JoinHandle;
use tracing::Instrument;

use crate::{
    ProgVers,
    connection::{ConnectionHandle, RpcSender},
    context::with_peer,
    handler::ServiceMap,
    rpc_error::DEFAULT_OPAQUE_AUTH,
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
// Shared helpers (used by both serial and concurrent paths)
// ---------------------------------------------------------------------------

/// Buffered records a read loop handles before it flushes and yields.
///
/// A burst that is already in the socket buffer never touches the socket
/// again, so nothing else would make the loop give the runtime a turn. Yield
/// every so often so other connections on the worker progress. The
/// concurrent loop also yields whenever its spawned handlers hold every
/// permit, which is what lets them run on a current-thread runtime; this
/// coarser bound is only for fairness, and each yield is preceded by a flush
/// so batching is not lost.
const YIELD_EVERY: usize = 256;

/// Flush what the read loop has queued and give the runtime a turn, then
/// resume the burst.
async fn yield_mid_burst(writer: &Arc<crate::connection::Writer>) {
    writer.reader_yield();
    tokio::task::yield_now().await;
    writer.reader_busy();
}

/// Route an inbound REPLY message to the pending-call map.
#[inline(always)]
fn route_reply(
    client_cbs: &ConnectionHandle,
    xid: u32,
    rbody: rpc_msg::reply_body,
    record: Vec<u8>,
    payload_offset: usize,
) {
    use crate::connection::ReplyResult;
    let was_fire_and_forget = match rbody {
        rpc_msg::reply_body::MSG_ACCEPTED { areply } => {
            if let rpc_msg::accepted_reply_reply_data::SUCCESS { results: _results } =
                areply.reply_data
            {
                client_cbs
                    .connection_state
                    .lock()
                    .unwrap()
                    .handle_reply(xid, ReplyResult::Ok(record, payload_offset))
            } else {
                client_cbs
                    .connection_state
                    .lock()
                    .unwrap()
                    .handle_reply(xid, ReplyResult::Err(areply.reply_data.into()))
            }
        }
        rpc_msg::reply_body::MSG_DENIED { rreply } => client_cbs
            .connection_state
            .lock()
            .unwrap()
            .handle_reply(xid, ReplyResult::Err(rreply.into())),
    };
    if was_fire_and_forget {
        client_cbs.decrement_pending();
    }
}

#[inline]
fn prepare_response(
    client_cbs: &ConnectionHandle,
    xid: u32,
    result: Result<Vec<u8>, crate::RpcError>,
) -> Result<Vec<u8>, ReadTaskError> {
    match result {
        Ok(response) => {
            let buf = client_cbs.buf_pool.take();
            prepare_success_response(buf, xid, &response)
        }
        Err(e) => Ok(crate::rpc_error::to_error_record(&e, xid)),
    }
}

/// Send a dispatch result (success or error) back to the client, waiting for
/// writer room if the connection's queue is full.
#[inline]
async fn send_response(
    client_cbs: &ConnectionHandle,
    xid: u32,
    result: Result<Vec<u8>, crate::RpcError>,
) -> Result<(), ReadTaskError> {
    let msg = prepare_response(client_cbs, xid, result)?;
    client_cbs
        .send_reply(msg)
        .await
        .map_err(ReadTaskError::WriteError)
}

/// Copy `bytes` into a buffer recycled from the connection's pool, so
/// records that must outlive the read buffer do not allocate.
#[inline]
fn pooled_copy(client_cbs: &ConnectionHandle, bytes: &[u8]) -> Vec<u8> {
    let mut buf = client_cbs.buf_pool.take();
    buf.extend_from_slice(bytes);
    buf
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
    #[cfg(not(feature = "concurrent-dispatch"))]
    {
        serve_serial(
            &service_map,
            &client_handle,
            &mut read_half,
            Some(&mut shutdown_rx),
        )
        .await
    }

    #[cfg(feature = "concurrent-dispatch")]
    {
        tokio::select! {
            _ = shutdown_rx.changed() => Ok(()),
            result = concurrent::serve(
                &service_map,
                &client_handle,
                &mut read_half,
                client_handle.max_concurrent_dispatches,
                client_handle.inline_ready_dispatch,
            ) => result,
        }
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

// ---------------------------------------------------------------------------
// Serial dispatch (default — no concurrent-dispatch feature)
// ---------------------------------------------------------------------------

/// Runs the serial read-dispatch loop. Returns only on error or connection
/// close, or `Ok(())` when `shutdown` fires.
///
/// Records are parsed straight out of the read buffer. Replies are queued on
/// the connection's writer and flushed together when the loop is about to
/// wait for the socket, x23's `dwrite`: a burst of pipelined calls whose
/// handlers finish on their first poll is answered with one `writev`. A
/// handler that suspends flushes the replies queued before it first.
#[cfg(not(feature = "concurrent-dispatch"))]
async fn serve_serial(
    service_map: &Arc<ServiceMap>,
    client_cbs: &Arc<ConnectionHandle>,
    read_half: &mut ReadHalf,
    mut shutdown: Option<&mut tokio::sync::watch::Receiver<bool>>,
) -> Result<(), ReadTaskError> {
    use crate::tcp_connection::TryRecord;
    let mut since_yield = 0usize;
    loop {
        if since_yield >= YIELD_EVERY {
            since_yield = 0;
            yield_mid_burst(&client_cbs.writer).await;
        }
        let record = match read_half.try_read_record()? {
            TryRecord::Ready(record) => {
                since_yield += 1;
                record
            }
            TryRecord::Invalid => return Err(ReadTaskError::Shutdown),
            TryRecord::WouldBlock => {
                since_yield = 0;
                client_cbs.writer.reader_waiting();
                match shutdown.as_deref_mut() {
                    Some(rx) => tokio::select! {
                        _ = rx.changed() => return Ok(()),
                        ready = read_half.readable() => ready?,
                    },
                    None => read_half.readable().await?,
                }
                client_cbs.writer.reader_busy();
                continue;
            }
        };

        let (msg, payload_offset) = parse_header(record)?;
        let remaining = &record[payload_offset..];

        match msg.body {
            rpc_msg::rpc_msg_body::CALL { cbody } => {
                let xid = msg.xid;
                let vers = ProgVers::new(cbody.prog, cbody.vers);
                let span = tracing::debug_span!(
                    "rpc_call",
                    xid,
                    prog = vers.prog(),
                    vers = vers.vers(),
                    proc = cbody.proc,
                );
                let sender: Arc<dyn RpcSender> = client_cbs.clone();
                let work = async {
                    let dispatch = with_peer(sender, async {
                        service_map
                            .dispatch(cbody.rpcvers, vers, cbody.proc, remaining)
                            .await
                    });
                    let mut dispatch = std::pin::pin!(dispatch);
                    // Poll once right here. A handler that finishes on its
                    // first poll keeps the reader busy, so its reply batches
                    // with the rest of the burst. One that suspends may run
                    // for a while and may call the peer: flush everything
                    // queued so far and stop holding calls before waiting.
                    // The no-op waker is safe because the real await below
                    // re-registers its waker, per the Future contract.
                    let probe = dispatch
                        .as_mut()
                        .poll(&mut std::task::Context::from_waker(std::task::Waker::noop()));
                    let res = match probe {
                        std::task::Poll::Ready(res) => res,
                        std::task::Poll::Pending => {
                            client_cbs.writer.reader_yield();
                            let res = dispatch.await;
                            client_cbs.writer.reader_busy();
                            res
                        }
                    };
                    send_response(client_cbs, xid, res).await
                }
                .instrument(span);
                match shutdown.as_deref_mut() {
                    Some(rx) => tokio::select! {
                        _ = rx.changed() => return Ok(()),
                        result = work => result?,
                    },
                    None => work.await?,
                }
            }
            rpc_msg::rpc_msg_body::REPLY { rbody } => {
                route_reply(
                    client_cbs,
                    msg.xid,
                    rbody,
                    pooled_copy(client_cbs, remaining),
                    0,
                );
            }
        }
    }
}

/// Runs the serial read-dispatch loop. Returns only on error or connection close.
#[cfg(not(feature = "concurrent-dispatch"))]
pub async fn launch_read_task(
    service_map: Arc<ServiceMap>,
    client_cbs: Arc<ConnectionHandle>,
    mut read_half: ReadHalf,
) -> Result<(), ReadTaskError> {
    serve_serial(&service_map, &client_cbs, &mut read_half, None).await
}

// ---------------------------------------------------------------------------
// Concurrent dispatch (feature = "concurrent-dispatch")
// ---------------------------------------------------------------------------

#[cfg(feature = "concurrent-dispatch")]
mod concurrent {
    use super::*;
    use crate::tcp_connection::TryRecord;
    use std::future::Future;
    use std::panic::AssertUnwindSafe;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::{Context, Poll, Waker};

    // -- Panic-catching future wrapper (replaces futures-util dependency) --

    /// Wraps a future so that panics during `poll` are caught and returned
    /// as `Err` instead of unwinding the task.  Same approach as
    /// `futures_util::FutureExt::catch_unwind` but with zero dependencies.
    ///
    /// `AssertUnwindSafe` is applied internally so the connection machinery
    /// can recover from a handler panic. Handlers remain responsible for the
    /// consistency of their own state if they panic while mutating it.
    struct CatchUnwind<F>(F);

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

    // -- Drop guard for the pending-tasks counter ----------------------------

    /// Decrements the pending-tasks counter on drop, ensuring cleanup on all
    /// exit paths (normal completion, semaphore close, and panic).
    struct PendingGuard(Arc<AtomicUsize>);

    impl Drop for PendingGuard {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::Relaxed);
        }
    }

    fn defer_response(
        tasks: &mut tokio::task::JoinSet<()>,
        client_cbs: &Arc<ConnectionHandle>,
        deferred: &Arc<AtomicUsize>,
        deferred_cap: usize,
        xid: u32,
        msg: Vec<u8>,
        span: tracing::Span,
    ) -> Result<(), ReadTaskError> {
        let count = deferred.fetch_add(1, Ordering::Relaxed);
        if count >= deferred_cap {
            deferred.fetch_sub(1, Ordering::Relaxed);
            return Err(ReadTaskError::WriteError(std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                "deferred response queue full",
            )));
        }

        let cbs = client_cbs.clone();
        let deferred = deferred.clone();
        tasks.spawn(
            async move {
                let _deferred = PendingGuard(deferred);
                if let Err(e) = cbs.send_reply(msg).await {
                    tracing::error!(xid, error = %e, "response write failed");
                    let _shutdown = cbs.begin_poison();
                }
            }
            .instrument(span),
        );
        Ok(())
    }

    // -- Handler futures -----------------------------------------------------

    /// The handler future for one CALL, owning everything it touches.
    ///
    /// The record is moved in rather than borrowed, so the future is `'static`
    /// and can be polled on the read loop and then — if it suspends — moved
    /// into a task. `Box::pin` gives it a stable address across that move;
    /// the inner `async_trait` dispatch allocates its own box on first poll
    /// either way, so the ready path trades a task allocation for this one.
    type HandlerFuture =
        Pin<Box<dyn Future<Output = Result<HandlerResult, Panic>> + Send + 'static>>;
    type HandlerResult = Result<Vec<u8>, crate::RpcError>;
    type Panic = Box<dyn std::any::Any + Send>;

    /// Build the `'static` handler future for one CALL.
    fn handler_future(
        service_map: &Arc<ServiceMap>,
        client_cbs: &Arc<ConnectionHandle>,
        record: Vec<u8>,
        payload_offset: usize,
        vers: ProgVers,
        rpcvers: u32,
        proc: u32,
    ) -> HandlerFuture {
        let svc = service_map.clone();
        let sender: Arc<dyn RpcSender> = client_cbs.clone();
        Box::pin(CatchUnwind(with_peer(sender, async move {
            svc.dispatch(rpcvers, vers, proc, &record[payload_offset..])
                .await
        })))
    }

    fn discard_panic_payload(panic: Panic, xid: u32, context: &'static str) {
        if let Err(drop_panic) = std::panic::catch_unwind(AssertUnwindSafe(|| drop(panic))) {
            tracing::error!(xid, context, "panic payload destructor panicked");
            // Dropping a second malicious panic payload could unwind the
            // connection task again. Leak only on this pathological path.
            std::mem::forget(drop_panic);
        }
    }

    /// Collapse a handler panic into a `SystemError` reply.
    pub(super) fn handler_outcome(result: Result<HandlerResult, Panic>, xid: u32) -> HandlerResult {
        match result {
            Ok(r) => r,
            Err(panic) => {
                tracing::error!(xid, "handler panicked");
                discard_panic_payload(panic, xid, "handler panic");
                Err(crate::RpcError::SystemError)
            }
        }
    }

    pub(super) fn drop_ready_handler(handler: HandlerFuture, xid: u32) -> bool {
        match std::panic::catch_unwind(AssertUnwindSafe(|| drop(handler))) {
            Ok(()) => false,
            Err(panic) => {
                tracing::error!(xid, "ready handler destructor panicked");
                discard_panic_payload(panic, xid, "ready handler destructor panic");
                true
            }
        }
    }

    /// Enqueue a ready reply without blocking the read loop: a full writer
    /// defers it to a task rather than stalling the reader ahead of a
    /// bidirectional REPLY further down the stream.
    fn enqueue_ready_reply(
        tasks: &mut tokio::task::JoinSet<()>,
        client_cbs: &Arc<ConnectionHandle>,
        deferred: &Arc<AtomicUsize>,
        deferred_cap: usize,
        xid: u32,
        msg: Vec<u8>,
        span: tracing::Span,
    ) -> Result<(), ReadTaskError> {
        match client_cbs.try_send_reply(msg) {
            Ok(()) => Ok(()),
            Err(crate::connection::TrySendError::Full(msg)) => {
                defer_response(tasks, client_cbs, deferred, deferred_cap, xid, msg, span)
            }
            Err(crate::connection::TrySendError::Closed) => Err(ReadTaskError::WriteError(
                std::io::Error::new(std::io::ErrorKind::BrokenPipe, "writer closed"),
            )),
        }
    }

    // -- Read loop -----------------------------------------------------------

    /// Run the connection's read loop: parse each record out of the read
    /// buffer, route REPLYs, and dispatch CALLs.
    ///
    /// Each CALL's handler is polled once right here. Handlers that complete
    /// on that first poll are answered inline; handlers that suspend are
    /// moved into their own tokio task, where a semaphore limits how many
    /// run concurrently. This is x23's `await_ready()` shape: create
    /// asynchronous work only when the service actually suspends.
    ///
    /// Replies queue on the connection's writer and are flushed together
    /// when this loop is about to wait for the socket (x23's `dwrite`), so a
    /// burst of pipelined calls costs one `writev`. Ready responses use a
    /// nonblocking enqueue so write backpressure cannot stop this loop from
    /// draining CALLs that precede a bidirectional REPLY in the TCP stream.
    ///
    /// Set `inline_ready` to false for an unconditional-spawn control. This
    /// still uses the common boxed-handler construction introduced by the fast
    /// path, so exact before/after comparisons require separate builds.
    pub(super) async fn serve(
        service_map: &Arc<ServiceMap>,
        client_cbs: &Arc<ConnectionHandle>,
        read_half: &mut ReadHalf,
        max_concurrent: usize,
        inline_ready: bool,
    ) -> Result<(), ReadTaskError> {
        let semaphore = Arc::new(tokio::sync::Semaphore::new(max_concurrent));
        // Advisory counter for load shedding — Relaxed is intentional because
        // the counter is a soft heuristic, not a synchronization mechanism.
        let pending = Arc::new(AtomicUsize::new(0));
        // Total spawned tasks (executing + waiting for a permit) before load
        // shedding.  E.g. max=1 -> cap=5, max=64 -> cap=320.
        let pending_cap = max_concurrent.saturating_mul(5);
        // Responses that cannot enter the bounded writer immediately are
        // deferred without blocking this loop. Bound that extra backlog
        // separately from handler admission; exhausting it fails the
        // connection closed rather than accumulating unbounded response tasks.
        let deferred_responses = Arc::new(AtomicUsize::new(0));
        let deferred_response_cap = client_cbs.write_channel_capacity.saturating_mul(5);
        let mut tasks = tokio::task::JoinSet::new();
        // Fast-path hit rate, reported once on exit. Loop-local: this task is
        // the only writer.
        let (mut ready_calls, mut suspended_calls, mut unprobed_calls) = (0u64, 0u64, 0u64);
        let writer = &client_cbs.writer;
        let mut since_yield = 0usize;

        let result = loop {
            // Reap completed tasks to free memory in the JoinSet.
            while tasks.try_join_next().is_some() {}

            // Spawned handlers hold every permit: anything admitted now would
            // only queue behind them, and on a current-thread runtime they
            // cannot run at all until this loop yields. Let them, before the
            // pending budget fills and calls are shed instead of served.
            if since_yield >= YIELD_EVERY || pending.load(Ordering::Relaxed) >= max_concurrent {
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
                    writer.reader_busy();
                    continue;
                }
                Err(e) => break Err(ReadTaskError::ReadError(e)),
            };

            let (msg, payload_offset) = match parse_header(record) {
                Ok(parsed) => parsed,
                Err(e) => break Err(e),
            };

            let cbody = match msg.body {
                rpc_msg::rpc_msg_body::REPLY { rbody } => {
                    route_reply(
                        client_cbs,
                        msg.xid,
                        rbody,
                        pooled_copy(client_cbs, &record[payload_offset..]),
                        0,
                    );
                    continue;
                }
                rpc_msg::rpc_msg_body::CALL { cbody } => cbody,
            };
            let xid = msg.xid;
            let vers = ProgVers::new(cbody.prog, cbody.vers);
            let proc = cbody.proc;

            // Load shedding: reject if too many tasks are in-flight + waiting
            // for a concurrency permit. Checked before the handler is built or
            // polled, so a shed CALL never runs its handler even partially —
            // the client can safely retry. Only suspended handlers occupy
            // this budget, so a purely ready workload never sheds.
            let count = pending.load(Ordering::Relaxed);
            if count >= pending_cap {
                tracing::warn!(xid, pending = count, "rejecting CALL: pending queue full");
                let error_bytes =
                    crate::rpc_error::to_error_record(&crate::RpcError::SystemError, xid);
                if let Err(e) = enqueue_ready_reply(
                    &mut tasks,
                    client_cbs,
                    &deferred_responses,
                    deferred_response_cap,
                    xid,
                    error_bytes,
                    tracing::Span::none(),
                ) {
                    break Err(e);
                }
                continue;
            }

            // Explicit parent: required because JoinSet::spawn doesn't
            // propagate tracing context. `ready` is recorded once the probe
            // poll has run. `ready` remains empty when probing is disabled or
            // saturation makes the fast path stand down.
            let span = tracing::debug_span!(
                parent: &client_cbs.connection_span,
                "rpc_call",
                xid,
                prog = vers.prog(),
                vers = vers.vers(),
                proc = proc,
                probed = tracing::field::Empty,
                ready = tracing::field::Empty,
            );

            // The handler future must own its record so it can outlive the
            // read buffer if it suspends.
            let mut handler = handler_future(
                service_map,
                client_cbs,
                pooled_copy(client_cbs, record),
                payload_offset,
                vers,
                cbody.rpcvers,
                proc,
            );

            // Reserve a permit *before* probing. A handler's first poll can
            // start real work — a timer, a `spawn_blocking`, plain CPU work —
            // before it ever suspends, so probing without a permit would let
            // that work escape `max_concurrent_dispatches` entirely.
            //
            // Probe only while `pending < max_concurrent`, which guarantees a
            // free permit exists. That matters because tokio's `try_acquire`
            // checks only the permit count and not the waiter queue, so it
            // can barge ahead of a spawned handler already queued on
            // `acquire_owned`. A waiter can only exist once every permit is
            // held by a spawned task, i.e. `pending >= max_concurrent` — so
            // under saturation the fast path stands down instead of barging
            // ahead of queued work.
            let permit = if inline_ready && count < max_concurrent {
                semaphore.clone().try_acquire_owned().ok()
            } else {
                None
            };

            if permit.is_some() {
                span.record("probed", true);
                // Probe poll. Synchronous, so a plain span guard is correct —
                // nothing is awaited while it is held.
                //
                // The no-op waker is safe because a `Pending` handler is
                // always polled again by the task below, and the `Future`
                // contract requires a future to re-register the waker it is
                // given on each poll. Any wake that lands between the two
                // polls is therefore observed as readiness by the second poll.
                let probe = {
                    let _enter = span.enter();
                    handler
                        .as_mut()
                        .poll(&mut Context::from_waker(Waker::noop()))
                };

                if let Poll::Ready(result) = probe {
                    ready_calls += 1;
                    span.record("ready", true);
                    let result = {
                        let _enter = span.enter();
                        let result = handler_outcome(result, xid);
                        if drop_ready_handler(handler, xid) {
                            Err(crate::RpcError::SystemError)
                        } else {
                            result
                        }
                    };
                    // Permit released here — a ready handler holds it only for
                    // its own synchronous poll.
                    drop(permit);
                    let enqueued = {
                        let _enter = span.enter();
                        prepare_response(client_cbs, xid, result).and_then(|msg| {
                            enqueue_ready_reply(
                                &mut tasks,
                                client_cbs,
                                &deferred_responses,
                                deferred_response_cap,
                                xid,
                                msg,
                                span.clone(),
                            )
                        })
                    };
                    if let Err(e) = enqueued {
                        break Err(e);
                    }
                    continue;
                }

                suspended_calls += 1;
                span.record("ready", false);
            } else {
                unprobed_calls += 1;
                span.record("probed", false);
            }

            // Suspended (or fast path stood down): commit to a task.
            pending.fetch_add(1, Ordering::Relaxed);

            let sem = semaphore.clone();
            let cbs = client_cbs.clone();
            let ctr = pending.clone();

            tasks.spawn(async move {
                let _pending = PendingGuard(ctr);

                // Reuse the probe's permit when we have one; otherwise queue
                // for one (may wait if at capacity).
                let _permit = match permit {
                    Some(p) => p,
                    None => match sem.acquire_owned().await {
                        Ok(p) => p,
                        Err(_) => return, // semaphore closed — shutting down
                    },
                };

                async {
                    let result = handler_outcome(handler.await, xid);
                    if let Err(e) = send_response(&cbs, xid, result).await {
                        tracing::error!(xid, error = %e, "response write failed");
                        let _shutdown = cbs.begin_poison();
                    }
                }
                .instrument(span)
                .await;
            });
        };

        tracing::debug!(
            ready_calls,
            suspended_calls,
            unprobed_calls,
            inline_ready,
            "read loop exiting"
        );
        result
    }
}

/// Runs the concurrent read-dispatch loop. Returns only on error or
/// connection close. Dropping the loop cancels any still-running handler
/// tasks with its `JoinSet`.
#[cfg(feature = "concurrent-dispatch")]
pub async fn launch_read_task(
    service_map: Arc<ServiceMap>,
    client_cbs: Arc<ConnectionHandle>,
    mut read_half: ReadHalf,
) -> Result<(), ReadTaskError> {
    concurrent::serve(
        &service_map,
        &client_cbs,
        &mut read_half,
        client_cbs.max_concurrent_dispatches,
        client_cbs.inline_ready_dispatch,
    )
    .await
}

// ---------------------------------------------------------------------------
// Response serialization helpers
// ---------------------------------------------------------------------------

#[inline(always)]
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

#[inline]
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

    #[cfg(feature = "concurrent-dispatch")]
    struct ReadyTestService {
        entered: tokio::sync::mpsc::UnboundedSender<u32>,
    }

    #[cfg(feature = "concurrent-dispatch")]
    #[async_trait::async_trait]
    impl crate::handler::RpcHandler for ReadyTestService {
        async fn dispatch(&self, proc: u32, data: &[u8]) -> Result<Vec<u8>, crate::RpcError> {
            self.entered.send(proc).unwrap();
            Ok(data.to_vec())
        }

        fn prog_vers(&self) -> crate::ProgVers {
            crate::ProgVers::new(1, 1)
        }
    }

    /// A framed CALL record for `proc` carrying `payload`.
    #[cfg(feature = "concurrent-dispatch")]
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

    /// A framed reply record with no results, distinguishable by `xid`.
    #[cfg(feature = "concurrent-dispatch")]
    fn filler_record(xid: u32) -> Vec<u8> {
        prepare_success_response(Vec::new(), xid, &[]).unwrap()
    }

    /// Read one record from the peer socket and return its xid, or `None` if
    /// nothing arrives within 200ms.
    #[cfg(feature = "concurrent-dispatch")]
    async fn next_xid(peer: &mut ReadHalf) -> Option<u32> {
        match tokio::time::timeout(std::time::Duration::from_millis(200), peer.read_record()).await
        {
            Ok(Ok(Some(record))) => Some(u32::from_be_bytes(record[..4].try_into().unwrap())),
            Ok(other) => panic!("unexpected read result: {other:?}"),
            Err(_) => None,
        }
    }

    /// A stalled connection whose peer has already sent CALLs 1..=`calls`,
    /// plus the peer's halves.
    #[cfg(feature = "concurrent-dispatch")]
    async fn stalled_connection_with_calls(
        config: &crate::ConnectionConfig,
        calls: u32,
    ) -> (
        Arc<ConnectionHandle>,
        ReadHalf,
        ReadHalf,
        tokio::net::tcp::OwnedWriteHalf,
    ) {
        use tokio::io::AsyncWriteExt;
        let (client, read_half, peer) = ConnectionHandle::new_stalled_for_test(config).await;
        let (peer_read, mut peer_write) = peer.into_split();
        for xid in 1..=calls {
            peer_write
                .write_all(&test_call_record(xid, xid, &[xid as u8]))
                .await
                .unwrap();
        }
        (
            Arc::new(client),
            read_half,
            ReadHalf::new_for_test(peer_read),
            peer_write,
        )
    }

    #[cfg(feature = "concurrent-dispatch")]
    #[tokio::test]
    async fn ready_responses_do_not_block_dispatch_when_write_channel_is_full() {
        use tokio::io::AsyncWriteExt;
        let config = crate::ConnectionConfig::default()
            .with_max_concurrent_dispatches(1)
            .with_write_channel_capacity(1);
        let (client, mut read_half, mut peer, mut peer_write) =
            stalled_connection_with_calls(&config, 5).await;
        // The one queue slot is taken and the socket is stalled, so every
        // response the read loop produces finds the writer full.
        client.try_send_reply(filler_record(0xAA)).ok().unwrap();

        let (entered_tx, mut entered_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut services = ServiceMap::new();
        services.add_service(Box::new(ReadyTestService {
            entered: entered_tx,
        }));
        let services = Arc::new(services);

        let serve_client = client.clone();
        let serve = tokio::spawn(async move {
            concurrent::serve(&services, &serve_client, &mut read_half, 1, true).await
        });

        for expected in 1..=5 {
            let entered =
                tokio::time::timeout(std::time::Duration::from_secs(1), entered_rx.recv())
                    .await
                    .expect("read loop blocked on writer capacity")
                    .unwrap();
            assert_eq!(entered, expected);
        }

        // Release the socket: the filler goes first, then the deferred
        // responses trickle through the one-record queue.
        client.writer.unstall_for_test();
        assert_eq!(next_xid(&mut peer).await, Some(0xAA));
        let mut response_xids = Vec::new();
        for _ in 0..5 {
            response_xids.push(
                next_xid(&mut peer)
                    .await
                    .expect("deferred response was not written"),
            );
        }
        response_xids.sort_unstable();
        assert_eq!(response_xids, [1, 2, 3, 4, 5]);

        peer_write.shutdown().await.unwrap();
        assert!(matches!(
            serve.await.unwrap(),
            Err(ReadTaskError::ReadError(_))
        ));
    }

    #[cfg(feature = "concurrent-dispatch")]
    #[tokio::test]
    async fn deferred_response_cap_fails_closed_and_cancels_backlog() {
        let config = crate::ConnectionConfig::default()
            .with_max_concurrent_dispatches(1)
            .with_write_channel_capacity(1);
        let (client, mut read_half, mut peer, _peer_write) =
            stalled_connection_with_calls(&config, 6).await;
        client.try_send_reply(filler_record(0xAA)).ok().unwrap();

        let (entered_tx, mut entered_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut services = ServiceMap::new();
        services.add_service(Box::new(ReadyTestService {
            entered: entered_tx,
        }));
        let services = Arc::new(services);

        let result = concurrent::serve(&services, &client, &mut read_half, 1, true).await;
        assert!(matches!(
            result,
            Err(ReadTaskError::WriteError(ref error))
                if error.kind() == std::io::ErrorKind::WouldBlock
        ));
        for expected in 1..=6 {
            assert_eq!(entered_rx.recv().await.unwrap(), expected);
        }

        // The deferred responses were cancelled with the read loop's JoinSet,
        // so only the filler ever reaches the wire.
        client.writer.unstall_for_test();
        assert_eq!(next_xid(&mut peer).await, Some(0xAA));
        assert_eq!(next_xid(&mut peer).await, None);
    }

    /// One socket write carrying `calls` ready echo CALLs, with the
    /// unconditional-spawn control (`inline_ready = false`) on a
    /// current-thread runtime: spawned handlers can only run when the read
    /// loop yields, so without a mid-burst yield the pending budget fills
    /// and calls past it are shed. Every call must be served.
    #[cfg(feature = "concurrent-dispatch")]
    #[tokio::test]
    async fn a_large_burst_of_spawned_handlers_is_fully_served() {
        use tokio::io::AsyncWriteExt;
        let calls = 1000u32;
        let config = crate::ConnectionConfig::default().with_max_concurrent_dispatches(64);
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
        let serve = tokio::spawn(async move {
            concurrent::serve(&services, &serve_client, &mut read_half, 64, false).await
        });

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

        peer_write.shutdown().await.unwrap();
        assert!(matches!(
            serve.await.unwrap(),
            Err(ReadTaskError::ReadError(_))
        ));
    }

    #[cfg(feature = "concurrent-dispatch")]
    #[test]
    fn panic_payload_destructor_cannot_unwind_dispatch() {
        struct PanicOnDrop;
        impl Drop for PanicOnDrop {
            fn drop(&mut self) {
                panic!("panic payload destructor");
            }
        }

        let result = concurrent::handler_outcome(Err(Box::new(PanicOnDrop)), 7);
        assert_eq!(result.unwrap_err(), crate::RpcError::SystemError);
    }

    #[cfg(feature = "concurrent-dispatch")]
    #[test]
    fn ready_handler_destructor_cannot_unwind_dispatch() {
        struct ReadyPanicOnDrop;
        impl std::future::Future for ReadyPanicOnDrop {
            type Output = Result<Result<Vec<u8>, crate::RpcError>, Box<dyn std::any::Any + Send>>;

            fn poll(
                self: std::pin::Pin<&mut Self>,
                _cx: &mut std::task::Context<'_>,
            ) -> std::task::Poll<Self::Output> {
                std::task::Poll::Ready(Ok(Ok(Vec::new())))
            }
        }
        impl Drop for ReadyPanicOnDrop {
            fn drop(&mut self) {
                panic!("ready handler destructor");
            }
        }

        type TestHandlerFuture = std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<
                            Result<Vec<u8>, crate::RpcError>,
                            Box<dyn std::any::Any + Send>,
                        >,
                    > + Send,
            >,
        >;
        let handler: TestHandlerFuture = Box::pin(ReadyPanicOnDrop);
        assert!(concurrent::drop_ready_handler(handler, 8));
    }
}
