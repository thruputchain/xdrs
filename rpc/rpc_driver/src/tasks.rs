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

/// Send a dispatch result (success or error) back to the client.
#[inline]
async fn send_response(
    client_cbs: &ConnectionHandle,
    xid: u32,
    result: Result<Vec<u8>, crate::RpcError>,
) -> Result<(), ReadTaskError> {
    let msg = prepare_response(client_cbs, xid, result)?;
    client_cbs
        .send_msg(msg)
        .await
        .map_err(ReadTaskError::WriteError)
}

/// Parsed inbound record — either a CALL to dispatch or a REPLY to route.
/// The `record` Vec is carried alongside a `payload_offset` to avoid an
/// O(n) memmove that `drain(..pos)` would require.
#[cfg(feature = "concurrent-dispatch")]
enum ParsedRecord {
    Call {
        xid: u32,
        record: Vec<u8>,
        payload_offset: usize,
        vers: ProgVers,
        rpcvers: u32,
        proc: u32,
    },
    Reply {
        xid: u32,
        rbody: rpc_msg::reply_body,
        record: Vec<u8>,
        payload_offset: usize,
    },
}

/// Parse an XDR record into a CALL or REPLY.  Carries the offset into the
/// record where the payload begins, avoiding the O(n) memmove from `drain`.
#[cfg(feature = "concurrent-dispatch")]
fn parse_record(record: Vec<u8>) -> Result<ParsedRecord, ReadTaskError> {
    let mut reader = xdrs::txs::buf_reader::BufReader::new(&record);
    let msg: rpc_msg::rpc_msg = match xdrs::read_xdr(&mut reader) {
        Ok(msg) => msg,
        Err(e) => {
            tracing::error!(error = %e, "failed to parse XDR record");
            return Err(ReadTaskError::Shutdown);
        }
    };
    let payload_offset = reader.position();

    match msg.body {
        rpc_msg::rpc_msg_body::CALL { cbody } => Ok(ParsedRecord::Call {
            xid: msg.xid,
            record,
            payload_offset,
            vers: ProgVers::new(cbody.prog, cbody.vers),
            rpcvers: cbody.rpcvers,
            proc: cbody.proc,
        }),
        rpc_msg::rpc_msg_body::REPLY { rbody } => Ok(ParsedRecord::Reply {
            xid: msg.xid,
            rbody,
            record,
            payload_offset,
        }),
    }
}

async fn run_client_read_task(
    service_map: Arc<ServiceMap>,
    client_handle: Arc<ConnectionHandle>,
    mut read_half: ReadHalf,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
) -> Result<(), ReadTaskError> {
    #[cfg(not(feature = "concurrent-dispatch"))]
    loop {
        tokio::select! {
            _ = shutdown_rx.changed() => return Ok(()),
            record = read_half.read_record() => {
                let record = match record? {
                    Some(record) => record,
                    None => return Err(ReadTaskError::Shutdown),
                };

                let mut reader = xdrs::txs::buf_reader::BufReader::new(record);
                let msg: rpc_msg::rpc_msg = match xdrs::read_xdr(&mut reader) {
                    Ok(msg) => msg,
                    Err(e) => {
                        tracing::error!(error = %e, "failed to parse XDR record");
                        return Err(ReadTaskError::Shutdown);
                    }
                };
                let remaining = &record[reader.position()..];

                match msg.body {
                    rpc_msg::rpc_msg_body::CALL { cbody } => {
                        let xid = msg.xid;
                        let vers = ProgVers::new(cbody.prog, cbody.vers);
                        let sender: Arc<dyn RpcSender> = client_handle.clone();
                        let res = with_peer(sender, async {
                            service_map
                                .dispatch(cbody.rpcvers, vers, cbody.proc, remaining)
                                .await
                        })
                        .await;
                        send_response(&client_handle, xid, res).await?;
                    }
                    rpc_msg::rpc_msg_body::REPLY { rbody } => {
                        route_reply(&client_handle, msg.xid, rbody, remaining.to_vec(), 0);
                    }
                }
            }
        }
    }

    #[cfg(feature = "concurrent-dispatch")]
    {
        let (record_tx, mut record_rx) = tokio::sync::mpsc::channel::<Result<Vec<u8>, ReadTaskError>>(
            client_handle.read_channel_capacity,
        );
        let cbs_for_reader = client_handle.clone();
        let mut shutdown_for_reader = shutdown_rx.clone();
        let reader_handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = shutdown_for_reader.changed() => break,
                    record = read_half.read_record() => {
                        match record {
                            Err(e) => {
                                let _ = record_tx.send(Err(ReadTaskError::ReadError(e))).await;
                                break;
                            }
                            Ok(None) => {
                                let _ = record_tx.send(Err(ReadTaskError::Shutdown)).await;
                                break;
                            }
                            Ok(Some(data)) => {
                                let record = data.to_vec();
                                let is_reply = record.len() >= 8
                                    && u32::from_be_bytes(record[4..8].try_into().unwrap()) == 1;

                                if is_reply {
                                    match parse_record(record) {
                                        Ok(ParsedRecord::Reply {
                                            xid,
                                            rbody,
                                            record,
                                            payload_offset,
                                        }) => {
                                            route_reply(
                                                &cbs_for_reader,
                                                xid,
                                                rbody,
                                                record,
                                                payload_offset,
                                            );
                                        }
                                        Ok(ParsedRecord::Call { record, .. }) => {
                                            if record_tx.send(Ok(record)).await.is_err() {
                                                break;
                                            }
                                        }
                                        Err(e) => {
                                            let _ = record_tx.send(Err(e)).await;
                                            break;
                                        }
                                    }
                                } else if record_tx.send(Ok(record)).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        });

        let dispatch_result = tokio::select! {
            _ = shutdown_rx.changed() => Ok(()),
            result = concurrent::dispatch_loop(
                &service_map,
                &client_handle,
                &mut record_rx,
                client_handle.max_concurrent_dispatches,
                client_handle.inline_ready_dispatch,
            ) => result,
        };

        reader_handle.abort();
        let _ = reader_handle.await;
        dispatch_result
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

/// Runs the serial read-dispatch loop. Returns only on error or connection close.
#[cfg(not(feature = "concurrent-dispatch"))]
pub async fn launch_read_task(
    service_map: Arc<ServiceMap>,
    client_cbs: Arc<ConnectionHandle>,
    mut read_half: ReadHalf,
) -> Result<(), ReadTaskError> {
    loop {
        let record = match read_half.read_record().await? {
            Some(data) => data,
            None => return Err(ReadTaskError::Shutdown),
        };

        // Parse inline from the borrowed slice — zero-copy for CALLs since
        // the record lives in ReadHalf's buffer until dispatch completes.
        let mut reader = xdrs::txs::buf_reader::BufReader::new(record);
        let msg: rpc_msg::rpc_msg = match xdrs::read_xdr(&mut reader) {
            Ok(msg) => msg,
            Err(e) => {
                tracing::error!(error = %e, "failed to parse XDR record");
                return Err(ReadTaskError::Shutdown);
            }
        };
        let remaining = &record[reader.position()..];

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
                async {
                    let res = with_peer(sender, async {
                        service_map
                            .dispatch(cbody.rpcvers, vers, cbody.proc, remaining)
                            .await
                    })
                    .await;
                    send_response(&client_cbs, xid, res).await
                }
                .instrument(span)
                .await?;
            }
            rpc_msg::rpc_msg_body::REPLY { rbody } => {
                route_reply(&client_cbs, msg.xid, rbody, remaining.to_vec(), 0);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Concurrent dispatch (feature = "concurrent-dispatch")
// ---------------------------------------------------------------------------

#[cfg(feature = "concurrent-dispatch")]
mod concurrent {
    use super::*;
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
                if let Err(e) = cbs.send_msg(msg).await {
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
    /// and can be polled on the dispatch loop and then — if it suspends —
    /// moved into a task. `Box::pin` gives it a stable address across that
    /// move; the inner `async_trait` dispatch allocates its own box on first
    /// poll either way, so the ready path trades a task allocation for this
    /// one.
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

    // -- Dispatch loop -------------------------------------------------------

    /// Run the main dispatch loop.
    ///
    /// Each incoming CALL's handler is polled once on this loop. Handlers that
    /// complete on that first poll are answered inline; handlers that suspend
    /// are moved into their own tokio task, where a semaphore limits how many
    /// run concurrently. This is x23's `await_ready()` shape: create
    /// asynchronous work only when the service actually suspends.
    ///
    /// REPLYs are routed by the reader task, not here. Ready responses use a
    /// nonblocking enqueue so write backpressure cannot stop this loop from
    /// draining CALLs that precede a bidirectional REPLY in the TCP stream.
    ///
    /// Set `inline_ready` to false for an unconditional-spawn control. This
    /// still uses the common boxed-handler construction introduced by the fast
    /// path, so exact before/after comparisons require separate builds.
    ///
    /// Separated from `launch_read_task` so that `reader_handle.abort()` runs
    /// on all exit paths (the caller awaits this, then aborts the reader).
    pub(super) async fn dispatch_loop(
        service_map: &Arc<ServiceMap>,
        client_cbs: &Arc<ConnectionHandle>,
        record_rx: &mut tokio::sync::mpsc::Receiver<Result<Vec<u8>, ReadTaskError>>,
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
        // Responses that cannot enter the bounded writer channel immediately
        // are deferred without blocking this loop. Bound that extra backlog
        // separately from handler admission; exhausting it fails the
        // connection closed rather than accumulating unbounded response tasks.
        let deferred_responses = Arc::new(AtomicUsize::new(0));
        let deferred_response_cap = client_cbs.write_channel_capacity.saturating_mul(5);
        let mut tasks = tokio::task::JoinSet::new();
        // Fast-path hit rate, reported once on exit. Loop-local: this task is
        // the only writer.
        let (mut ready_calls, mut suspended_calls, mut unprobed_calls) = (0u64, 0u64, 0u64);

        let result = loop {
            // Reap completed tasks to free memory in the JoinSet.
            while tasks.try_join_next().is_some() {}

            let record = match record_rx.recv().await {
                Some(Ok(data)) => data,
                Some(Err(e)) => break Err(e),
                None => break Err(ReadTaskError::Shutdown),
            };

            let parsed = match parse_record(record) {
                Ok(parsed) => parsed,
                Err(e) => break Err(e),
            };

            match parsed {
                ParsedRecord::Call {
                    xid,
                    record,
                    payload_offset,
                    vers,
                    rpcvers,
                    proc,
                } => {
                    // Load shedding: reject if too many tasks are in-flight +
                    // waiting for a concurrency permit. Checked before the
                    // handler is built or polled, so a shed CALL never runs its
                    // handler even partially — the client can safely retry.
                    // Only suspended handlers occupy this budget, so a purely
                    // ready workload never sheds.
                    let count = pending.load(Ordering::Relaxed);
                    if count >= pending_cap {
                        tracing::warn!(xid, pending = count, "rejecting CALL: pending queue full");
                        let error_bytes =
                            crate::rpc_error::to_error_record(&crate::RpcError::SystemError, xid);
                        match client_cbs.try_send_msg(error_bytes) {
                            Ok(()) => {}
                            Err(tokio::sync::mpsc::error::TrySendError::Full(msg)) => {
                                if let Err(e) = defer_response(
                                    &mut tasks,
                                    client_cbs,
                                    &deferred_responses,
                                    deferred_response_cap,
                                    xid,
                                    msg,
                                    tracing::Span::none(),
                                ) {
                                    break Err(e);
                                }
                            }
                            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                                break Err(ReadTaskError::WriteError(std::io::Error::new(
                                    std::io::ErrorKind::BrokenPipe,
                                    "writer closed",
                                )));
                            }
                        }
                        continue;
                    }

                    // Explicit parent: required because JoinSet::spawn doesn't
                    // propagate tracing context. `ready` is recorded once the
                    // probe poll has run. `ready` remains empty when probing is
                    // disabled or saturation makes the fast path stand down.
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

                    let mut handler = handler_future(
                        service_map,
                        client_cbs,
                        record,
                        payload_offset,
                        vers,
                        rpcvers,
                        proc,
                    );

                    // Reserve a permit *before* probing. A handler's first poll
                    // can start real work — a timer, a `spawn_blocking`, plain
                    // CPU work — before it ever suspends, so probing without a
                    // permit would let that work escape
                    // `max_concurrent_dispatches` entirely.
                    //
                    // Probe only while `pending < max_concurrent`, which
                    // guarantees a free permit exists. That matters because
                    // tokio's `try_acquire` checks only the permit count and
                    // not the waiter queue, so it can barge ahead of a spawned
                    // handler already queued on `acquire_owned`. A waiter can
                    // only exist once every permit is held by a spawned task,
                    // i.e. `pending >= max_concurrent` — so under saturation the
                    // fast path stands down instead of barging ahead of queued
                    // work.
                    let permit = if inline_ready && count < max_concurrent {
                        semaphore.clone().try_acquire_owned().ok()
                    } else {
                        None
                    };

                    if permit.is_some() {
                        span.record("probed", true);
                        // Probe poll. Synchronous, so a plain span guard is
                        // correct — nothing is awaited while it is held.
                        //
                        // The no-op waker is safe because a `Pending` handler
                        // is always polled again by the task below, and the
                        // `Future` contract requires a future to re-register
                        // the waker it is given on each poll. Any wake that
                        // lands between the two polls is therefore observed as
                        // readiness by the second poll.
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
                            // Permit released here — a ready handler holds it
                            // only for its own synchronous poll.
                            drop(permit);
                            let msg = {
                                let _enter = span.enter();
                                match prepare_response(client_cbs, xid, result) {
                                    Ok(msg) => msg,
                                    Err(e) => break Err(e),
                                }
                            };
                            let enqueue = {
                                let _enter = span.enter();
                                client_cbs.try_send_msg(msg)
                            };
                            match enqueue {
                                Ok(()) => {}
                                Err(tokio::sync::mpsc::error::TrySendError::Full(msg)) => {
                                    // Keep the dispatch loop live under output
                                    // backpressure so the reader can reach
                                    // bidirectional REPLYs behind queued CALLs.
                                    if let Err(e) = defer_response(
                                        &mut tasks,
                                        client_cbs,
                                        &deferred_responses,
                                        deferred_response_cap,
                                        xid,
                                        msg,
                                        span,
                                    ) {
                                        break Err(e);
                                    }
                                }
                                Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                                    break Err(ReadTaskError::WriteError(std::io::Error::new(
                                        std::io::ErrorKind::BrokenPipe,
                                        "writer closed",
                                    )));
                                }
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

                        // Reuse the probe's permit when we have one; otherwise
                        // queue for one (may wait if at capacity).
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
                }
                ParsedRecord::Reply {
                    xid,
                    rbody,
                    record,
                    payload_offset,
                } => {
                    // Pass the raw record + offset — no drain/memmove.
                    // The consumer slices when reading the result.
                    route_reply(client_cbs, xid, rbody, record, payload_offset);
                }
            }
        };

        tracing::debug!(
            ready_calls,
            suspended_calls,
            unprobed_calls,
            inline_ready,
            "dispatch loop exiting"
        );
        result
    }
}

#[cfg(feature = "concurrent-dispatch")]
pub async fn launch_read_task(
    service_map: Arc<ServiceMap>,
    client_cbs: Arc<ConnectionHandle>,
    mut read_half: ReadHalf,
) -> Result<(), ReadTaskError> {
    // Spawn a dedicated reader task that owns the ReadHalf and forwards
    // CALL records over a channel to the dispatch loop. REPLY records are
    // routed directly in the reader task — no channel hop — since they
    // just write a result into the pending-call HashMap and wake the
    // caller. This cuts one async task wakeup per reply.
    //
    // Channel capacity is configurable via ConnectionConfig::read_channel_capacity
    // (default 256). The reader task may stay this many records ahead of the
    // dispatch loop, smoothing latency spikes while still providing backpressure.
    let (record_tx, mut record_rx) = tokio::sync::mpsc::channel::<Result<Vec<u8>, ReadTaskError>>(
        client_cbs.read_channel_capacity,
    );

    let cbs_for_reader = client_cbs.clone();
    let reader_handle = tokio::spawn(async move {
        loop {
            match read_half.read_record().await {
                Err(e) => {
                    let _ = record_tx.send(Err(ReadTaskError::ReadError(e))).await;
                    break;
                }
                Ok(None) => {
                    let _ = record_tx.send(Err(ReadTaskError::Shutdown)).await;
                    break;
                }
                Ok(Some(data)) => {
                    let record = data.to_vec();

                    // Peek at msg_type (bytes 4..8): CALL=0, REPLY=1.
                    // Route REPLYs directly in the reader task — skips
                    // the dispatch channel + dispatch loop wakeup.
                    let is_reply = record.len() >= 8
                        && u32::from_be_bytes(record[4..8].try_into().unwrap()) == 1;

                    if is_reply {
                        match parse_record(record) {
                            Ok(ParsedRecord::Reply {
                                xid,
                                rbody,
                                record,
                                payload_offset,
                            }) => {
                                route_reply(&cbs_for_reader, xid, rbody, record, payload_offset);
                            }
                            Ok(ParsedRecord::Call { record, .. }) => {
                                if record_tx.send(Ok(record)).await.is_err() {
                                    break;
                                }
                            }
                            Err(e) => {
                                let _ = record_tx.send(Err(e)).await;
                                break;
                            }
                        }
                        continue;
                    }

                    if record_tx.send(Ok(record)).await.is_err() {
                        break;
                    }
                }
            }
        }
    });

    let max_concurrent = client_cbs.max_concurrent_dispatches;

    // Run the dispatch loop, then abort the reader task on any exit path
    // (error, shutdown, or write failure). Dropping the JoinSet inside the
    // loop cancels any still-running handler tasks.
    let result = concurrent::dispatch_loop(
        &service_map,
        &client_cbs,
        &mut record_rx,
        max_concurrent,
        client_cbs.inline_ready_dispatch,
    )
    .await;

    reader_handle.abort();
    result
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
        let framed = crate::connection::frame_rpc_record(Vec::new(), &header, payload).unwrap();
        framed[4..].to_vec()
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

    #[cfg(feature = "concurrent-dispatch")]
    #[tokio::test]
    async fn ready_responses_do_not_block_dispatch_when_write_channel_is_full() {
        let config = crate::ConnectionConfig::default()
            .with_max_concurrent_dispatches(1)
            .with_write_channel_capacity(1);
        let (write_tx, mut write_rx) = tokio::sync::mpsc::channel(1);
        write_tx.try_send(vec![0xAA]).unwrap();
        let client = Arc::new(ConnectionHandle::new_with_write_sender_for_test(
            write_tx, &config,
        ));

        let (entered_tx, mut entered_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut services = ServiceMap::new();
        services.add_service(Box::new(ReadyTestService {
            entered: entered_tx,
        }));
        let services = Arc::new(services);

        let (record_tx, mut record_rx) = tokio::sync::mpsc::channel(5);
        for xid in 1..=5 {
            record_tx
                .send(Ok(test_call_record(xid, xid, &[xid as u8])))
                .await
                .unwrap();
        }

        let dispatch_client = client.clone();
        let dispatch_services = services.clone();
        let dispatch = tokio::spawn(async move {
            concurrent::dispatch_loop(
                &dispatch_services,
                &dispatch_client,
                &mut record_rx,
                1,
                true,
            )
            .await
        });

        for expected in 1..=5 {
            let entered =
                tokio::time::timeout(std::time::Duration::from_secs(1), entered_rx.recv())
                    .await
                    .expect("dispatch loop blocked on write-channel capacity")
                    .unwrap();
            assert_eq!(entered, expected);
        }

        assert_eq!(write_rx.recv().await.unwrap(), vec![0xAA]);
        let mut response_xids = Vec::new();
        for _ in 0..5 {
            let response = tokio::time::timeout(std::time::Duration::from_secs(1), write_rx.recv())
                .await
                .expect("deferred response was not enqueued")
                .unwrap();
            response_xids.push(u32::from_be_bytes(response[4..8].try_into().unwrap()));
        }
        response_xids.sort_unstable();
        assert_eq!(response_xids, [1, 2, 3, 4, 5]);

        drop(record_tx);
        assert!(matches!(
            dispatch.await.unwrap(),
            Err(ReadTaskError::Shutdown)
        ));
    }

    #[cfg(feature = "concurrent-dispatch")]
    #[tokio::test]
    async fn deferred_response_cap_fails_closed_and_cancels_backlog() {
        let config = crate::ConnectionConfig::default()
            .with_max_concurrent_dispatches(1)
            .with_write_channel_capacity(1);
        let (write_tx, mut write_rx) = tokio::sync::mpsc::channel(1);
        write_tx.try_send(vec![0xAA]).unwrap();
        let client = Arc::new(ConnectionHandle::new_with_write_sender_for_test(
            write_tx, &config,
        ));

        let (entered_tx, mut entered_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut services = ServiceMap::new();
        services.add_service(Box::new(ReadyTestService {
            entered: entered_tx,
        }));
        let services = Arc::new(services);
        let (record_tx, mut record_rx) = tokio::sync::mpsc::channel(6);
        for xid in 1..=6 {
            record_tx
                .send(Ok(test_call_record(xid, xid, &[xid as u8])))
                .await
                .unwrap();
        }

        let result = concurrent::dispatch_loop(&services, &client, &mut record_rx, 1, true).await;
        assert!(matches!(
            result,
            Err(ReadTaskError::WriteError(ref error))
                if error.kind() == std::io::ErrorKind::WouldBlock
        ));
        for expected in 1..=6 {
            assert_eq!(entered_rx.recv().await.unwrap(), expected);
        }

        assert_eq!(write_rx.recv().await.unwrap(), vec![0xAA]);
        assert!(matches!(
            write_rx.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));
        drop(record_tx);
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
