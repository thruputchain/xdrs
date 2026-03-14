use std::sync::Arc;

use crate::{
    ProgVers,
    connection::{ConnectionHandle, RpcSender, log_rpc_timestamp},
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

// ---------------------------------------------------------------------------
// Shared helpers (used by both serial and concurrent paths)
// ---------------------------------------------------------------------------

/// Route an inbound REPLY message to the pending-call map.
fn route_reply(
    client_cbs: &ConnectionHandle,
    xid: u32,
    rbody: rpc_msg::reply_body,
    payload: Vec<u8>,
) {
    if client_cbs.log_timestamps {
        log_rpc_timestamp(xid, "client_recv");
    }
    match rbody {
        rpc_msg::reply_body::MSG_ACCEPTED { areply } => {
            if let rpc_msg::accepted_reply_reply_data::SUCCESS { results: _results } =
                areply.reply_data
            {
                client_cbs
                    .connection_state
                    .lock()
                    .unwrap()
                    .handle_reply(xid, Ok(payload));
            } else {
                client_cbs
                    .connection_state
                    .lock()
                    .unwrap()
                    .handle_reply(xid, Err(areply.reply_data.into()));
            }
        }
        rpc_msg::reply_body::MSG_DENIED { rreply } => {
            client_cbs
                .connection_state
                .lock()
                .unwrap()
                .handle_reply(xid, Err(rreply.into()));
        }
    }
}

/// Send a dispatch result (success or error) back to the client.
async fn send_response(
    client_cbs: &ConnectionHandle,
    xid: u32,
    result: Result<Vec<u8>, crate::rpc_error::RpcError>,
    write_buffer: &mut xdrs::txs::buf_writer::BufWriter,
) -> Result<(), ReadTaskError> {
    match result {
        Ok(response) => {
            prepare_success_response(xid, &response, write_buffer)?;
            if client_cbs.log_timestamps {
                log_rpc_timestamp(xid, "server_send");
            }
            client_cbs
                .send_msg(write_buffer.as_ref())
                .await
                .map_err(ReadTaskError::WriteError)?;
        }
        Err(e) => {
            if client_cbs.log_timestamps {
                log_rpc_timestamp(xid, "server_send");
            }
            let out_bytes = e.to_error_record(xid);
            client_cbs
                .send_msg(&out_bytes)
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
            eprintln!("[rpc] failed to parse XDR record: {e}");
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
    let mut write_buffer = xdrs::txs::buf_writer::BufWriter::default();
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
                eprintln!("[rpc] failed to parse XDR record: {e}");
                return Err(ReadTaskError::Shutdown);
            }
        };
        let remaining = &record[reader.position()..];

        match msg.body {
            rpc_msg::rpc_msg_body::CALL { cbody } => {
                let xid = msg.xid;
                if client_cbs.log_timestamps {
                    log_rpc_timestamp(xid, "server_recv");
                }
                let vers = ProgVers {
                    prog: cbody.prog,
                    vers: cbody.vers,
                };
                let sender: Arc<dyn RpcSender> = client_cbs.clone();
                let res = with_peer(sender, async {
                    service_map
                        .dispatch(cbody.rpcvers, vers, cbody.proc, remaining)
                        .await
                })
                .await;
                send_response(&client_cbs, xid, res, &mut write_buffer).await?;
            }
            rpc_msg::rpc_msg_body::REPLY { rbody } => {
                route_reply(&client_cbs, msg.xid, rbody, remaining.to_vec());
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
        // shedding.  E.g. max=1 → cap=5, max=64 → cap=320.
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
                    if client_cbs.log_timestamps {
                        log_rpc_timestamp(xid, "server_recv");
                    }

                    // Load shedding: reject if too many tasks are
                    // in-flight + waiting for a concurrency permit.
                    let count = pending.fetch_add(1, Ordering::Relaxed);
                    if count >= pending_cap {
                        pending.fetch_sub(1, Ordering::Relaxed);
                        eprintln!("[rpc] xid={xid} rejecting CALL: pending queue full ({count})",);
                        let error_bytes =
                            crate::rpc_error::RpcError::SystemError.to_error_record(xid);
                        client_cbs
                            .send_msg(&error_bytes)
                            .await
                            .map_err(ReadTaskError::WriteError)?;
                        continue;
                    }

                    let sem = semaphore.clone();
                    let svc = service_map.clone();
                    let cbs = client_cbs.clone();
                    let ctr = pending.clone();

                    tasks.spawn(async move {
                        let _pending = PendingGuard(ctr);

                        // Acquire concurrency permit (may wait if at capacity).
                        let _permit = match sem.acquire_owned().await {
                            Ok(p) => p,
                            Err(_) => return, // semaphore closed — shutting down
                        };

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
                                eprintln!("[rpc] xid={xid} handler panicked");
                                Err(crate::rpc_error::RpcError::SystemError)
                            }
                        };

                        let mut write_buffer = xdrs::txs::buf_writer::BufWriter::default();
                        if let Err(e) = send_response(&cbs, xid, result, &mut write_buffer).await {
                            eprintln!("[rpc] xid={xid} response write failed: {e}");
                        }
                    });
                }
                ParsedRecord::Reply {
                    xid,
                    rbody,
                    record,
                    payload_offset,
                } => {
                    route_reply(client_cbs, xid, rbody, record[payload_offset..].to_vec());
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
    // complete records over a channel.  This decouples TCP framing from
    // dispatch: the reader can be independently aborted on shutdown, and
    // the dispatch loop receives pre-framed records without needing to
    // manage partial reads.
    //
    // Channel capacity 8: the reader task may stay up to 8 records ahead of
    // the dispatch loop, smoothing latency spikes while still providing
    // backpressure when tasks are at capacity.
    let (record_tx, mut record_rx) =
        tokio::sync::mpsc::channel::<Result<Vec<u8>, ReadTaskError>>(8);

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
                    // Allocates: ReadHalf reuses its internal buffer across reads,
                    // so we must copy here to send over the channel. The
                    // offset-based parse downstream avoids a second copy.
                    if record_tx.send(Ok(data.to_vec())).await.is_err() {
                        // Main task dropped the receiver — connection is done.
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

fn prepare_success_response(
    xid: u32,
    results: &[u8],
    writer: &mut xdrs::txs::buf_writer::BufWriter,
) -> Result<(), ReadTaskError> {
    writer.clear();
    let header = get_success_header(xid);

    // placeholder for message length
    xdrs::write_xdr(&0u32, writer).unwrap();
    // write the header
    xdrs::write_xdr(&header, writer).unwrap();

    let header_len = writer.len() - 4;

    // Check for integer overflow when calculating message length
    // Total length must fit in u32 AND have top bit clear (for ORing with 0x8000_0000)
    let total_len = header_len
        .checked_add(results.len())
        .and_then(|sum| u32::try_from(sum).ok())
        .filter(|&len| len <= 0x7FFF_FFFF)
        .ok_or_else(|| {
            eprintln!(
                "Message too large: header={} + results={} exceeds max (0x7FFF_FFFF)",
                writer.len(),
                results.len()
            );
            ReadTaskError::Shutdown
        })?;

    let msg_len: u32 = total_len | 0x8000_0000;

    {
        let bytes: &mut [u8] = writer.as_mut();
        bytes[..4].copy_from_slice(&msg_len.to_be_bytes());
    }

    writer.as_mut().extend_from_slice(results);

    Ok(())
}
