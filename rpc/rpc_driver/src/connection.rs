//! Connection state and RPC call handling.

use std::collections::HashMap;
use std::sync::Mutex as StdMutex;
use tokio::sync::{Mutex, oneshot};

use async_trait::async_trait;

use crate::{
    ProgVers,
    rpc_error::{DEFAULT_OPAQUE_AUTH, RpcError},
    rpc_msg,
    tcp_connection::WriteHalf,
};

/// Bundles the TCP write half with a reusable serialization buffer.
/// The buffer grows to fit the largest message and is reused across calls,
/// eliminating per-RPC heap allocations on the client send path.
pub(crate) struct WriteState {
    pub(crate) half: WriteHalf,
    pub(crate) buf: xdrs::txs::buf_writer::BufWriter,
}

impl std::fmt::Debug for WriteState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WriteState").finish_non_exhaustive()
    }
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

#[derive(Debug)]
pub(crate) struct CallbackHandle {
    tx: oneshot::Sender<Result<Vec<u8>, RpcError>>,
}

#[derive(Debug)]
pub(crate) struct ConnectionState {
    pub(crate) pending_requests: HashMap<u32, CallbackHandle>,
    last_xid: u32,
    poisoned: bool,
}

impl ConnectionState {
    pub fn new() -> Self {
        Self {
            pending_requests: HashMap::new(),
            last_xid: 0,
            poisoned: false,
        }
    }

    pub(crate) fn add_callback(&mut self, callback: CallbackHandle) -> Result<u32, RpcError> {
        if self.poisoned {
            return Err(RpcError::SystemError);
        }

        loop {
            self.last_xid = self.last_xid.wrapping_add(1);
            if !self.pending_requests.contains_key(&self.last_xid) {
                self.pending_requests.insert(self.last_xid, callback);
                return Ok(self.last_xid);
            }
        }
    }

    pub fn handle_reply(&mut self, xid: u32, reply: Result<Vec<u8>, RpcError>) {
        if let Some(callback) = self.pending_requests.remove(&xid) {
            let _ = callback.tx.send(reply);
            return;
        }
        eprintln!("Received reply for unknown xid: {}", xid);
    }

    pub fn poison(&mut self) {
        self.poisoned = true;
        for (_xid, callback) in self.pending_requests.drain() {
            // if these error out, then receiver must be dropped
            let _ = callback.tx.send(Err(RpcError::SystemError));
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
    write_half: Mutex<Option<WriteState>>,
    pub(crate) log_timestamps: bool,
    #[cfg_attr(not(feature = "concurrent-dispatch"), allow(dead_code))]
    pub(crate) max_concurrent_dispatches: usize,
}

impl ConnectionHandle {
    /// Create a new connection handle wrapping the given TCP write half.
    pub fn new(write_half: WriteHalf, config: &crate::ConnectionConfig) -> Self {
        Self {
            connection_state: StdMutex::new(ConnectionState::new()),
            write_half: Mutex::new(Some(WriteState {
                half: write_half,
                buf: xdrs::txs::buf_writer::BufWriter::default(),
            })),
            log_timestamps: config.log_timestamps,
            max_concurrent_dispatches: config.max_concurrent_dispatches,
        }
    }

    /// Poison the connection, failing all pending requests and closing the write half.
    pub async fn poison(&self) {
        // Drop the write half to send TCP FIN to the remote end.
        *self.write_half.lock().await = None;
        self.connection_state.lock().unwrap().poison();
    }

    /// Write bytes directly to the socket from the calling task — no channel hop.
    pub(crate) async fn send_msg(&self, msg: &[u8]) -> Result<(), std::io::Error> {
        match self.write_half.lock().await.as_mut() {
            Some(ws) => ws.half.write_data(msg).await,
            None => Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "connection poisoned",
            )),
        }
    }

    fn prepare_call_msg(
        xid: u32,
        progvers: ProgVers,
        proc: u32,
        data: &[u8],
        writer: &mut xdrs::txs::buf_writer::BufWriter,
    ) -> Result<(), RpcError> {
        writer.clear();
        let msg = rpc_msg::rpc_msg {
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

        // Write placeholder for record length (patched below).
        xdrs::write_xdr(&0u32, writer).map_err(|_| RpcError::SystemError)?;
        // Write RPC header directly — no intermediate Vec.
        xdrs::write_xdr(&msg, writer).map_err(|_| RpcError::SystemError)?;

        let header_len = writer.len().checked_sub(4).ok_or(RpcError::SystemError)?;
        // Check for integer overflow when calculating message length
        // Total length must fit in u32 AND have top bit clear (for ORing with 0x8000_0000)
        let total_len = header_len
            .checked_add(data.len())
            .and_then(|sum| u32::try_from(sum).ok())
            .filter(|&len| len <= 0x7FFF_FFFF)
            .ok_or_else(|| {
                eprintln!(
                    "Message too large: header={} + data={} exceeds max (0x7FFF_FFFF)",
                    header_len,
                    data.len()
                );
                RpcError::SystemError
            })?;

        let msg_len: u32 = total_len | 0x8000_0000;
        {
            let bytes: &mut Vec<u8> = writer.as_mut();
            bytes[..4].copy_from_slice(&msg_len.to_be_bytes());
            bytes.extend_from_slice(data);
        }
        Ok(())
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
        let (tx, rx) = oneshot::channel();
        let xid = self
            .connection_state
            .lock()
            .unwrap()
            .add_callback(CallbackHandle { tx })?;

        // Lock write state once: build message into reusable buffer, then flush.
        // Message construction is fast (XDR encode + memcpy), so this doesn't
        // significantly extend lock hold time vs. the write_all that follows.
        let send_result = {
            let mut guard = self.write_half.lock().await;
            match guard.as_mut() {
                Some(ws) => match Self::prepare_call_msg(xid, progvers, proc, data, &mut ws.buf) {
                    Ok(()) => {
                        if self.log_timestamps {
                            log_rpc_timestamp(xid, "client_send");
                        }
                        ws.half.write_data(ws.buf.as_ref()).await
                    }
                    Err(e) => {
                        self.connection_state
                            .lock()
                            .unwrap()
                            .pending_requests
                            .remove(&xid);
                        return Err(e);
                    }
                },
                None => Err(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "connection poisoned",
                )),
            }
        };

        if send_result.is_err() {
            self.connection_state
                .lock()
                .unwrap()
                .pending_requests
                .remove(&xid);
            return Err(RpcError::SystemError);
        }

        rx.await.map_err(|_| RpcError::SystemError)?
    }

    /// Returns the number of pending (in-flight) requests.
    pub fn num_pending_requests(&self) -> usize {
        self.connection_state.lock().unwrap().pending_requests.len()
    }
}

/// Log an RPC timestamp event to stderr.
///
/// Output format: `[rpc] xid=<xid> event=<event> ts=<secs>.<micros>`
pub(crate) fn log_rpc_timestamp(xid: u32, event: &str) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    eprintln!(
        "[rpc] xid={} event={} ts={}.{:06}",
        xid,
        event,
        ts.as_secs(),
        ts.subsec_micros()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_callback_roundtrip() {
        let mut state = ConnectionState::new();

        let (tx, rx) = oneshot::channel();

        state.add_callback(CallbackHandle { tx }).unwrap();
        state.handle_reply(0, Ok(b"world".to_vec()));

        state.handle_reply(1, Ok(b"hello".to_vec()));

        assert_eq!(rx.await.unwrap().unwrap(), b"hello".to_vec());

        // the rpc callback should have been removed
        state.handle_reply(1, Ok(b"foo".to_vec()));
    }
}
