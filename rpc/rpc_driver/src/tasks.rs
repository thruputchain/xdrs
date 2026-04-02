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

/// Send a dispatch result (success or error) back to the client.
#[inline]
async fn send_response(
    client_cbs: &ConnectionHandle,
    xid: u32,
    result: Result<Vec<u8>, crate::rpc_error::RpcError>,
) -> Result<(), ReadTaskError> {
    match result {
        Ok(response) => {
            let buf = client_cbs.buf_pool.take();
            let msg = prepare_success_response(buf, xid, &response)?;
            client_cbs
                .send_msg(msg)
                .await
                .map_err(ReadTaskError::WriteError)?;
        }
        Err(e) => {
            let out_bytes = e.to_error_record(xid);
            client_cbs
                .send_msg(out_bytes)
                .await
                .map_err(ReadTaskError::WriteError)?;
        }
    }
    Ok(())
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
            vers: ProgVers {
                prog: cbody.prog,
                vers: cbody.vers,
            },
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
                        let vers = ProgVers {
                            prog: cbody.prog,
                            vers: cbody.vers,
                        };
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
                let vers = ProgVers {
                    prog: cbody.prog,
                    vers: cbody.vers,
                };
                let span = tracing::debug_span!(
                    "rpc_call",
                    xid,
                    prog = vers.prog,
                    vers = vers.vers,
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
    use std::panic::AssertUnwindSafe;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // -- Panic-catching future wrapper (replaces futures-util dependency) --

    /// Wraps a future so that panics during `poll` are caught and returned
    /// as `Err` instead of unwinding the task.  Same approach as
    /// `futures_util::FutureExt::catch_unwind` but with zero dependencies.
    ///
    /// `AssertUnwindSafe` is applied internally — callers do not need to
    /// wrap their futures.  This is sound for our use because neither
    /// `ServiceMap` nor `ConnectionHandle` hold their locks during
    /// `dispatch()`, so a handler panic cannot poison shared state.
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

    // -- Dispatch loop -------------------------------------------------------

    /// Run the main dispatch loop using spawn-per-CALL.
    ///
    /// Each incoming CALL is spawned as its own tokio task, matching the
    /// fire-and-forget pattern from x23's C++ ONC-RPC implementation. A
    /// semaphore limits concurrent handler execution while the read loop
    /// never blocks on handlers — REPLYs are always routed immediately,
    /// avoiding deadlocks when handlers call back to the peer.
    ///
    /// Separated from `launch_read_task` so that `reader_handle.abort()` runs
    /// on all exit paths (the caller awaits this, then aborts the reader).
    pub(super) async fn dispatch_loop(
        service_map: &Arc<ServiceMap>,
        client_cbs: &Arc<ConnectionHandle>,
        record_rx: &mut tokio::sync::mpsc::Receiver<Result<Vec<u8>, ReadTaskError>>,
        max_concurrent: usize,
    ) -> Result<(), ReadTaskError> {
        let semaphore = Arc::new(tokio::sync::Semaphore::new(max_concurrent));
        // Advisory counter for load shedding — Relaxed is intentional because
        // the counter is a soft heuristic, not a synchronization mechanism.
        let pending = Arc::new(AtomicUsize::new(0));
        // Total spawned tasks (executing + waiting for a permit) before load
        // shedding.  E.g. max=1 -> cap=5, max=64 -> cap=320.
        let pending_cap = max_concurrent.saturating_mul(5);
        let mut tasks = tokio::task::JoinSet::new();

        loop {
            // Reap completed tasks to free memory in the JoinSet.
            while tasks.try_join_next().is_some() {}

            let record = match record_rx.recv().await {
                Some(Ok(data)) => data,
                Some(Err(e)) => return Err(e),
                None => return Err(ReadTaskError::Shutdown),
            };

            match parse_record(record)? {
                ParsedRecord::Call {
                    xid,
                    record,
                    payload_offset,
                    vers,
                    rpcvers,
                    proc,
                } => {
                    // Load shedding: reject if too many tasks are
                    // in-flight + waiting for a concurrency permit.
                    let count = pending.fetch_add(1, Ordering::Relaxed);
                    if count >= pending_cap {
                        pending.fetch_sub(1, Ordering::Relaxed);
                        tracing::warn!(xid, pending = count, "rejecting CALL: pending queue full");
                        let error_bytes =
                            crate::rpc_error::RpcError::SystemError.to_error_record(xid);
                        client_cbs
                            .send_msg(error_bytes)
                            .await
                            .map_err(ReadTaskError::WriteError)?;
                        continue;
                    }

                    let sem = semaphore.clone();
                    let svc = service_map.clone();
                    let cbs = client_cbs.clone();
                    let ctr = pending.clone();
                    let conn_span = client_cbs.connection_span.clone();

                    tasks.spawn(async move {
                        let _pending = PendingGuard(ctr);

                        // Acquire concurrency permit (may wait if at capacity).
                        let _permit = match sem.acquire_owned().await {
                            Ok(p) => p,
                            Err(_) => return, // semaphore closed — shutting down
                        };

                        // rpc_call span starts after permit — measures handler time only.
                        // Explicit parent: required because JoinSet::spawn doesn't
                        // propagate tracing context.
                        let span = tracing::debug_span!(
                            parent: &conn_span,
                            "rpc_call",
                            xid,
                            prog = vers.prog,
                            vers = vers.vers,
                            proc = proc,
                        );

                        async {
                            // Dispatch with panic isolation via CatchUnwind.
                            let sender: Arc<dyn RpcSender> = cbs.clone();
                            let result = CatchUnwind(with_peer(sender, async {
                                svc.dispatch(rpcvers, vers, proc, &record[payload_offset..])
                                    .await
                            }))
                            .await;

                            let result = match result {
                                Ok(r) => r,
                                Err(_panic) => {
                                    tracing::error!(xid, "handler panicked");
                                    Err(crate::rpc_error::RpcError::SystemError)
                                }
                            };

                            if let Err(e) = send_response(&cbs, xid, result).await {
                                tracing::error!(xid, error = %e, "response write failed");
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
        }
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
    let result =
        concurrent::dispatch_loop(&service_map, &client_cbs, &mut record_rx, max_concurrent).await;

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
}
