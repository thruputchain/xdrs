use async_trait::async_trait;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{mpsc, oneshot, RwLock};

use crate::{
    rpc_error::{RpcError, DEFAULT_OPAQUE_AUTH},
    rpc_msg, ProgVers,
};

pub struct CallbackHandle {
    tx: oneshot::Sender<Result<Vec<u8>, RpcError>>,
}

pub struct ClientState {
    pub pending_requests: HashMap<u32, CallbackHandle>,
    last_xid: u32,
}

impl ClientState {
    pub fn new() -> Self {
        Self {
            pending_requests: HashMap::new(),
            last_xid: 0,
        }
    }

    pub fn add_callback(&mut self, callback: CallbackHandle) -> u32 {
        loop {
            self.last_xid = self.last_xid.wrapping_add(1);
            if !self.pending_requests.contains_key(&self.last_xid) {
                self.pending_requests.insert(self.last_xid, callback);
                return self.last_xid;
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
}

impl Drop for ClientState {
    fn drop(&mut self) {
        for (_xid, callback) in self.pending_requests.drain() {
            // if these error out, then receiver must be dropped
            let _ = callback.tx.send(Err(RpcError::SystemError));
        }
    }
}

pub struct ClientHandle {
    pub client_state: RwLock<ClientState>,
    pub channel: mpsc::UnboundedSender<Vec<u8>>,
}

impl ClientHandle {
    pub fn new(channel: mpsc::UnboundedSender<Vec<u8>>) -> Self {
        Self {
            client_state: RwLock::new(ClientState::new()),
            channel,
        }
    }

    pub fn make_call_msg(&self, xid: u32, progvers: ProgVers, proc: u32, data: &[u8]) -> Vec<u8> {
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
        let header = xdrs::serialize_txs::encode(&msg).unwrap();
        let len = header.len() as u32 + data.len() as u32;
        msg_bytes.extend_from_slice(&len.to_be_bytes());
        msg_bytes.extend_from_slice(&header);
        msg_bytes.extend_from_slice(data);
        msg_bytes
    }

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
            .add_callback(CallbackHandle { tx });
        self.channel
            .send(self.make_call_msg(xid, progvers, proc, data))
            .map_err(|_| RpcError::SystemError)?;
        rx.await.map_err(|_| RpcError::SystemError)?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::oneshot;

    #[tokio::test]
    async fn test_callback_roundtrip() {
        let mut state = ClientState::new();

        let (tx, rx) = oneshot::channel();

        state.add_callback(CallbackHandle { tx });
        state.handle_reply(0, Ok(b"world".to_vec()));

        state.handle_reply(1, Ok(b"hello".to_vec()));

        assert_eq!(rx.await.unwrap().unwrap(), b"hello".to_vec());

        // the rpc callback should have been removed
        state.handle_reply(1, Ok(b"foo".to_vec()));
    }
}
