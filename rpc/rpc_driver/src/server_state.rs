//! Server-side service dispatch and registration.

use crate::{rpc_error::RpcError, ProgVers};
use async_trait::async_trait;
use std::collections::BTreeMap;

/// Trait for implementing RPC service handlers.
///
/// Implement this trait to define how your service handles incoming RPC calls.
#[async_trait]
pub trait ServerDispatch: Send + Sync {
    /// Handle an incoming RPC call for the given procedure number.
    async fn dispatch(&self, proc: u32, data: &[u8]) -> Result<Vec<u8>, RpcError>;

    /// Return the program number and version this service handles.
    fn prog_vers(&self) -> ProgVers;
}

/// Registry of RPC services, keyed by program number and version.
pub struct ServiceMap {
    services: BTreeMap<ProgVers, Box<dyn ServerDispatch>>,
}

impl ServiceMap {
    /// Create an empty service map.
    pub fn new() -> Self {
        Self {
            services: BTreeMap::new(),
        }
    }

    /// Register a service handler.
    pub fn add_service(&mut self, dispatch: Box<dyn ServerDispatch>) {
        self.services.insert(dispatch.prog_vers(), dispatch);
    }

    /// Dispatch an incoming RPC call to the appropriate service.
    pub async fn dispatch(
        &self,
        rpc_vers: u32,
        srv: ProgVers,
        proc: u32,
        data: &[u8],
    ) -> Result<Vec<u8>, RpcError> {
        if rpc_vers != 2 {
            return Err(RpcError::RpcMismatch);
        }
        if let Some(srv) = self.services.get(&srv) {
            return srv.dispatch(proc, data).await;
        }

        // Find the first service with matching program number
        let first_match = self.services.range(
            ProgVers {
                prog: srv.prog,
                vers: 0,
            }..,
        );

        // If no match found or first match has different prog, return PROG_UNAVAIL
        if first_match
            .clone()
            .next()
            .map_or(true, |entry| entry.0.prog != srv.prog)
        {
            return Err(RpcError::ProgUnavail);
        }

        // Get version range for this program
        let low_vers = first_match.clone().next().unwrap().0.vers;
        let high_vers = self
            .services
            .range(
                ..=ProgVers {
                    prog: srv.prog,
                    vers: 0xffffffff,
                },
            )
            .next_back()
            .unwrap()
            .0
            .vers;

        // Return PROG_MISMATCH with version range
        return Err(RpcError::InvalidVersion {
            low: low_vers,
            high: high_vers,
        });
    }
}
