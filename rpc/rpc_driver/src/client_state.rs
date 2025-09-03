use async_trait::async_trait;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::RwLock;

use crate::rpc_error::RpcError;

#[async_trait]
pub trait Callback: Send + Sync {
    async fn on_reply(self: Box<Self>, reply: Result<Vec<u8>, RpcError>);
}

pub struct ClientState {
    pub pending_requests: HashMap<u32, Box<dyn Callback>>,
    last_xid: u32,
}

impl ClientState {
    pub fn new() -> Self {
        Self {
            pending_requests: HashMap::new(),
            last_xid: 0,
        }
    }

    pub fn add_callback(&mut self, callback: Box<dyn Callback>) {
        loop {
            self.last_xid = self.last_xid.wrapping_add(1);
            if !self.pending_requests.contains_key(&self.last_xid) {
                self.pending_requests.insert(self.last_xid, callback);
                break;
            }
        }
    }

    pub fn handle_reply(&mut self, xid: u32, reply: Result<Vec<u8>, RpcError>) -> Option<()> {
        if let Some(callback) = self.pending_requests.remove(&xid) {
            tokio::spawn(async move {
                callback.on_reply(reply).await;
            });
            return Some(());
        }
        None
    }
}

impl Drop for ClientState {
    fn drop(&mut self) {
        for (_xid, callback) in self.pending_requests.drain() {
            tokio::spawn(async move {
                callback.on_reply(Err(RpcError::SystemError)).await;
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::oneshot;

    struct PrintCallback {
        tx: oneshot::Sender<Vec<u8>>,
    }

    #[async_trait]
    impl Callback for PrintCallback {
        async fn on_reply(self: Box<Self>, reply: Result<Vec<u8>, RpcError>) {
            println!(
                "Got reply: {:?}",
                String::from_utf8_lossy(&reply.as_ref().unwrap())
            );
            self.tx.send(reply.unwrap()).unwrap();
        }
    }

    #[tokio::test]
    async fn test_callback_roundtrip() {
        let mut state = ClientState::new();

        let (tx, rx) = oneshot::channel();

        state.add_callback(Box::new(PrintCallback { tx }));
        assert!(state.handle_reply(0, Ok(b"world".to_vec())).is_none());
        state.handle_reply(1, Ok(b"hello".to_vec())).unwrap();

        assert_eq!(rx.await.unwrap(), b"hello");
    }
}
