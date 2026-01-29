//! Client connection state and RPC call handling.

use std::collections::HashMap;
use tokio::sync::{RwLock, mpsc, oneshot};

use crate::{
    ProgVers,
    rpc_error::{DEFAULT_OPAQUE_AUTH, RpcError},
    rpc_msg,
};

#[derive(Debug)]
pub(crate) struct CallbackHandle {
    tx: oneshot::Sender<Result<Vec<u8>, RpcError>>,
}

#[derive(Debug)]
pub(crate) struct ClientState {
    pub(crate) pending_requests: HashMap<u32, CallbackHandle>,
    last_xid: u32,
    poisoned: bool,
    shutdown_tx: Option<oneshot::Sender<()>>,
}

impl ClientState {
    pub fn new(shutdown_tx: oneshot::Sender<()>) -> Self {
        Self {
            pending_requests: HashMap::new(),
            last_xid: 0,
            poisoned: false,
            shutdown_tx: Some(shutdown_tx),
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
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }
    }
}

impl Drop for ClientState {
    fn drop(&mut self) {
        self.poison();
    }
}

/// Handle for making RPC calls over an established connection.
///
/// Manages pending requests and provides methods for sending calls
/// and graceful shutdown via poisoning.
#[derive(Debug)]
pub struct ClientHandle {
    pub(crate) client_state: RwLock<ClientState>,
    channel: mpsc::UnboundedSender<Vec<u8>>,
}

impl ClientHandle {
    /// Create a new client handle with the given write channel.
    ///
    /// Returns the handle and a shutdown receiver for the write task.
    pub fn new(channel: mpsc::UnboundedSender<Vec<u8>>) -> (Self, oneshot::Receiver<()>) {
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        (
            Self {
                client_state: RwLock::new(ClientState::new(shutdown_tx)),
                channel,
            },
            shutdown_rx,
        )
    }

    /// Poison the connection, failing all pending requests and signaling shutdown.
    pub async fn poison(&self) {
        self.client_state.write().await.poison();
    }

    pub(crate) fn send_msg(
        &self,
        msg: &[u8],
    ) -> Result<(), tokio::sync::mpsc::error::SendError<Vec<u8>>> {
        self.channel.send(msg.to_vec())
    }

    fn make_call_msg(&self, xid: u32, progvers: ProgVers, proc: u32, data: &[u8]) -> Vec<u8> {
        let mut msg_bytes = Vec::new();

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
        let len = (header.len() as u32 + data.len() as u32) | 0x8000_0000;
        msg_bytes.extend_from_slice(&len.to_be_bytes());
        msg_bytes.extend_from_slice(&header);
        msg_bytes.extend_from_slice(data);
        msg_bytes
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
            .client_state
            .write()
            .await
            .add_callback(CallbackHandle { tx })?;
        self.send_msg(&self.make_call_msg(xid, progvers, proc, data))
            .map_err(|_| RpcError::SystemError)?;
        rx.await.map_err(|_| RpcError::SystemError)?
    }

    /// Returns the number of pending (in-flight) requests.
    pub async fn num_pending_requests(&self) -> usize {
        self.client_state.read().await.pending_requests.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::oneshot;

    #[tokio::test]
    async fn test_callback_roundtrip() {
        let (shutdown_tx, _shutdown_rx) = oneshot::channel();
        let mut state = ClientState::new(shutdown_tx);

        let (tx, rx) = oneshot::channel();

        state.add_callback(CallbackHandle { tx }).unwrap();
        state.handle_reply(0, Ok(b"world".to_vec()));

        state.handle_reply(1, Ok(b"hello".to_vec()));

        assert_eq!(rx.await.unwrap().unwrap(), b"hello".to_vec());

        // the rpc callback should have been removed
        state.handle_reply(1, Ok(b"foo".to_vec()));
    }
}
