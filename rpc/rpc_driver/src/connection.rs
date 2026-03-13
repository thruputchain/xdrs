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

/// Trait for making outgoing RPC calls.
///
/// Implemented by [`ConnectionHandle`] for real TCP connections,
/// and by [`FixedPool`](crate::FixedPool) and [`DynamicPool`](crate::DynamicPool)
/// for pooled connections.
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
    write_half: Mutex<Option<WriteHalf>>,
    pub(crate) log_timestamps: bool,
}

impl ConnectionHandle {
    /// Create a new connection handle wrapping the given TCP write half.
    pub fn new(write_half: WriteHalf, log_timestamps: bool) -> Self {
        Self {
            connection_state: StdMutex::new(ConnectionState::new()),
            write_half: Mutex::new(Some(write_half)),
            log_timestamps,
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
            Some(wh) => wh.write_data(msg).await,
            None => Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "connection poisoned",
            )),
        }
    }

    fn make_call_msg(
        &self,
        xid: u32,
        progvers: ProgVers,
        proc: u32,
        data: &[u8],
    ) -> Result<Vec<u8>, RpcError> {
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
        let header = xdrs::txs::encode(&msg).unwrap();

        // Check for integer overflow when calculating message length
        // Total length must fit in u32 AND have top bit clear (for ORing with 0x8000_0000)
        let total_len = header
            .len()
            .checked_add(data.len())
            .and_then(|sum| u32::try_from(sum).ok())
            .filter(|&len| len <= 0x7FFF_FFFF)
            .ok_or_else(|| {
                eprintln!(
                    "Message too large: header={} + data={} exceeds max (0x7FFF_FFFF)",
                    header.len(),
                    data.len()
                );
                RpcError::SystemError
            })?;

        let len = total_len | 0x8000_0000;
        let mut msg_bytes = Vec::with_capacity(4 + header.len() + data.len());
        msg_bytes.extend_from_slice(&len.to_be_bytes());
        msg_bytes.extend_from_slice(&header);
        msg_bytes.extend_from_slice(data);
        Ok(msg_bytes)
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

        let msg = match self.make_call_msg(xid, progvers, proc, data) {
            Ok(msg) => msg,
            Err(e) => {
                self.connection_state
                    .lock()
                    .unwrap()
                    .pending_requests
                    .remove(&xid);
                return Err(e);
            }
        };

        if self.log_timestamps {
            log_rpc_timestamp(xid, "client_send");
        }

        let send_result = self.send_msg(&msg).await;
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
