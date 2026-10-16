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

/// Copy into reusable storage when available. Suspended handlers and caller
/// replies need owned data that outlives the socket read buffer.
#[inline]
fn copy_record(bytes: &[u8]) -> Vec<u8> {
    let mut buf = crate::connection::take_buffer();
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

    /// The handler future for one CALL, owning everything it touches.
    ///
    /// The record is moved in rather than borrowed, so the future is `'static`
    /// and can be polled on the read loop and then — if it suspends — moved
    /// into a task. `Box::pin` gives it a stable address across that move;
    /// the inner `async_trait` dispatch allocates its own box on first poll
    /// either way, so the ready path trades a task allocation for this one.
    type HandlerFuture =
        Pin<Box<dyn Future<Output = Result<HandlerResult, Panic>> + Send + 'static>>;
    type HandlerResult = Result<Vec<u8>, ReadTaskError>;
    type Panic = Box<dyn std::any::Any + Send>;

    /// Build the `'static` handler future for one CALL.
    fn handler_future(
        service_map: &Arc<ServiceMap>,
        client_cbs: &Arc<ConnectionHandle>,
        record: Vec<u8>,
        xid: u32,
        vers: ProgVers,
        rpcvers: u32,
        proc: u32,
    ) -> HandlerFuture {
        let svc = service_map.clone();
        let sender: Arc<dyn RpcSender> = client_cbs.clone();
        Box::pin(CatchUnwind(with_peer(sender, async move {
            let result = svc.dispatch(rpcvers, vers, proc, &record).await;
            // The handler no longer borrows its input. Reuse that allocation
            // for the wire reply instead of discarding it and taking another
            // buffer from the cache.
            match result {
                Ok(response) => prepare_success_response(record, xid, &response),
                Err(e) => Ok(crate::rpc_error::to_error_record(&e, xid)),
            }
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
                Ok(crate::rpc_error::to_error_record(
                    &crate::RpcError::SystemError,
                    xid,
                ))
            }
        }
    }

    pub(super) fn drop_handler(handler: HandlerFuture, xid: u32) -> bool {
        match std::panic::catch_unwind(AssertUnwindSafe(|| drop(handler))) {
            Ok(()) => false,
            Err(panic) => {
                tracing::error!(xid, "handler destructor panicked");
                discard_panic_payload(panic, xid, "handler destructor panic");
                true
            }
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
        let mut tasks = tokio::task::JoinSet::new();
        // Fast-path hit rate, reported once on exit. Loop-local: this task is
        // the only writer.
        let (mut ready_calls, mut suspended_calls) = (0u64, 0u64);
        let writer = &client_cbs.writer;
        writer.reader_active();
        let mut since_yield = 0usize;

        let result = loop {
            // Reap completed tasks to free memory in the JoinSet.
            while tasks.try_join_next().is_some() {}

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
                        copy_record(&record[payload_offset..]),
                        0,
                    );
                    continue;
                }
                rpc_msg::rpc_msg_body::CALL { cbody } => cbody,
            };
            let xid = msg.xid;

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

            let vers = ProgVers::new(cbody.prog, cbody.vers);
            let proc = cbody.proc;

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

            // The handler future must own its record so it can outlive the
            // read buffer if it suspends.
            let mut handler = handler_future(
                service_map,
                client_cbs,
                copy_record(&record[payload_offset..]),
                xid,
                vers,
                cbody.rpcvers,
                proc,
            );

            // Probe poll. Synchronous, so a plain span guard is correct —
            // nothing is awaited while it is held.
            //
            // The no-op waker is safe because a `Pending` handler is
            // either cancelled by the cap or polled again by the task below. The `Future`
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
                    if drop_handler(handler, xid) {
                        Ok(crate::rpc_error::to_error_record(
                            &crate::RpcError::SystemError,
                            xid,
                        ))
                    } else {
                        result
                    }
                };
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
                && tasks.len() >= limit
            {
                // Tasks may have finished during the reader's fairness yield
                // or the inline probe. Reap them before refusing this handler.
                while tasks.try_join_next().is_some() {}
                if tasks.len() >= limit {
                    drop_handler(handler, xid);
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
            }

            let cbs = client_cbs.clone();
            tasks.spawn(async move {
                async {
                    let result = handler_outcome(handler.await, xid);
                    if let Err(e) = send_response(&cbs, result) {
                        tracing::error!(xid, error = %e, "response write failed");
                        let _shutdown = cbs.begin_poison();
                    }
                }
                .instrument(span)
                .await;
            });
        };

        tracing::debug!(ready_calls, suspended_calls, "read loop exiting");
        result
    }
}

/// Runs the concurrent read-dispatch loop. Returns only on error or
/// connection close. Dropping the loop cancels any still-running handler
/// tasks with its `JoinSet`.
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

    struct ReadyTestService {
        entered: tokio::sync::mpsc::UnboundedSender<u32>,
    }

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
        #[async_trait::async_trait]
        impl crate::handler::RpcHandler for YieldingService {
            async fn dispatch(&self, proc: u32, data: &[u8]) -> Result<Vec<u8>, crate::RpcError> {
                if proc == 1 {
                    tokio::task::yield_now().await;
                }
                Ok(data.to_vec())
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
            type Output = Result<Result<Vec<u8>, ReadTaskError>, Box<dyn std::any::Any + Send>>;

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
                            Result<Vec<u8>, ReadTaskError>,
                            Box<dyn std::any::Any + Send>,
                        >,
                    > + Send,
            >,
        >;
        let handler: TestHandlerFuture = Box::pin(ReadyPanicOnDrop);
        assert!(dispatch::drop_handler(handler, 8));
    }
}
