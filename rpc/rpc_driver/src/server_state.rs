use crate::{rpc_error::RpcError, ProcVers};
use async_trait::async_trait;
use std::collections::BTreeMap;

#[async_trait]
pub trait ServerDispatch: Send + Sync {
    async fn dispatch(&self, xid: u32, data: &[u8]) -> Result<Vec<u8>, RpcError>;
}

pub struct ServiceMap {
    services: BTreeMap<ProcVers, Box<dyn ServerDispatch>>,
}

impl ServiceMap {
    pub fn new() -> Self {
        Self {
            services: BTreeMap::new(),
        }
    }

    pub async fn dispatch(
        &self,
        rpc_vers: u32,
        srv: ProcVers,
        xid: u32,
        data: &[u8],
    ) -> Result<Vec<u8>, RpcError> {
        if rpc_vers != 2 {
            return Err(RpcError::RpcMismatch);
        }
        if let Some(srv) = self.services.get(&srv) {
            return srv.dispatch(xid, data).await;
        }
        Err(RpcError::InvalidProcedure)
    }
}
