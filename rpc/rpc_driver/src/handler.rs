//! RPC handler dispatch and service registration.

use crate::{ProgVers, rpc_error::RpcError};
use async_trait::async_trait;
use std::collections::BTreeMap;

/// Trait for implementing RPC service handlers.
///
/// Implement this trait to define how your service handles incoming RPC calls.
///
/// For bidirectional RPC (calling back to the peer from within a handler),
/// use [`peer()`](crate::peer) to obtain the per-connection sender.
///
/// With the default serial dispatch, calling back to the peer inline will
/// deadlock — spawn a separate task instead. With the `concurrent-dispatch`
/// feature enabled, inline callbacks work without deadlocking.
#[async_trait]
pub trait RpcHandler: Send + Sync {
    /// Handle an incoming RPC call for the given procedure number.
    async fn dispatch(&self, proc: u32, data: &[u8]) -> Result<Vec<u8>, RpcError>;

    /// Return the program number and version this service handles.
    fn prog_vers(&self) -> ProgVers;
}

/// Registry of RPC services, keyed by program number and version.
pub struct ServiceMap {
    services: BTreeMap<ProgVers, Box<dyn RpcHandler>>,
}

impl Default for ServiceMap {
    fn default() -> Self {
        Self::new()
    }
}

impl ServiceMap {
    /// Create an empty service map.
    pub fn new() -> Self {
        Self {
            services: BTreeMap::new(),
        }
    }

    /// Register a service handler.
    pub fn add_service(&mut self, dispatch: Box<dyn RpcHandler>) {
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
        if let Some(handler) = self.services.get(&srv) {
            return handler.dispatch(proc, data).await;
        }

        // Find the first service with matching program number
        let first_match = self.services.range(ProgVers::new(srv.prog(), 0)..);

        // If no match found or first match has different prog, return PROG_UNAVAIL
        if first_match
            .clone()
            .next()
            .is_none_or(|entry| entry.0.prog() != srv.prog())
        {
            return Err(RpcError::ProgUnavail);
        }

        // Get version range for this program
        let low_vers = first_match.clone().next().unwrap().0.vers();
        let high_vers = self
            .services
            .range(..=ProgVers::new(srv.prog(), 0xffffffff))
            .next_back()
            .unwrap()
            .0
            .vers();

        // Return PROG_MISMATCH with version range
        Err(RpcError::InvalidVersion {
            low: low_vers,
            high: high_vers,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test service that echoes data back or returns the proc number as a u32.
    struct TestService {
        prog: u32,
        vers: u32,
    }

    #[async_trait]
    impl RpcHandler for TestService {
        async fn dispatch(&self, proc: u32, data: &[u8]) -> Result<Vec<u8>, RpcError> {
            match proc {
                0 => Ok(vec![]),        // null procedure
                1 => Ok(data.to_vec()), // echo
                _ => Err(RpcError::InvalidProcedure),
            }
        }

        fn prog_vers(&self) -> ProgVers {
            ProgVers::new(self.prog, self.vers)
        }
    }

    #[tokio::test]
    async fn test_rpc_version_mismatch() {
        let mut map = ServiceMap::new();
        map.add_service(Box::new(TestService { prog: 100, vers: 1 }));

        let result = map.dispatch(1, ProgVers::new(100, 1), 0, b"").await;
        assert_eq!(result, Err(RpcError::RpcMismatch));

        let result = map.dispatch(3, ProgVers::new(100, 1), 0, b"").await;
        assert_eq!(result, Err(RpcError::RpcMismatch));

        let result = map.dispatch(0, ProgVers::new(100, 1), 0, b"").await;
        assert_eq!(result, Err(RpcError::RpcMismatch));
    }

    #[tokio::test]
    async fn test_exact_prog_vers_match() {
        let mut map = ServiceMap::new();
        map.add_service(Box::new(TestService { prog: 100, vers: 1 }));

        let result = map.dispatch(2, ProgVers::new(100, 1), 0, b"").await;
        assert_eq!(result, Ok(vec![]));

        let result = map.dispatch(2, ProgVers::new(100, 1), 1, b"hello").await;
        assert_eq!(result, Ok(b"hello".to_vec()));
    }

    #[tokio::test]
    async fn test_program_unavailable() {
        let mut map = ServiceMap::new();
        map.add_service(Box::new(TestService { prog: 100, vers: 1 }));

        let result = map.dispatch(2, ProgVers::new(200, 1), 0, b"").await;
        assert_eq!(result, Err(RpcError::ProgUnavail));

        let result = map.dispatch(2, ProgVers::new(99, 1), 0, b"").await;
        assert_eq!(result, Err(RpcError::ProgUnavail));
    }

    #[tokio::test]
    async fn test_empty_service_map() {
        let map = ServiceMap::new();

        let result = map.dispatch(2, ProgVers::new(100, 1), 0, b"").await;
        assert_eq!(result, Err(RpcError::ProgUnavail));
    }

    #[tokio::test]
    async fn test_version_mismatch_single_version() {
        let mut map = ServiceMap::new();
        map.add_service(Box::new(TestService { prog: 100, vers: 2 }));

        let result = map.dispatch(2, ProgVers::new(100, 1), 0, b"").await;
        assert_eq!(result, Err(RpcError::InvalidVersion { low: 2, high: 2 }));

        let result = map.dispatch(2, ProgVers::new(100, 3), 0, b"").await;
        assert_eq!(result, Err(RpcError::InvalidVersion { low: 2, high: 2 }));
    }

    #[tokio::test]
    async fn test_version_mismatch_multiple_versions() {
        let mut map = ServiceMap::new();
        map.add_service(Box::new(TestService { prog: 100, vers: 1 }));
        map.add_service(Box::new(TestService { prog: 100, vers: 3 }));
        map.add_service(Box::new(TestService { prog: 100, vers: 5 }));

        let result = map.dispatch(2, ProgVers::new(100, 2), 0, b"").await;
        assert_eq!(result, Err(RpcError::InvalidVersion { low: 1, high: 5 }));

        let result = map.dispatch(2, ProgVers::new(100, 4), 0, b"").await;
        assert_eq!(result, Err(RpcError::InvalidVersion { low: 1, high: 5 }));

        let result = map.dispatch(2, ProgVers::new(100, 0), 0, b"").await;
        assert_eq!(result, Err(RpcError::InvalidVersion { low: 1, high: 5 }));

        let result = map.dispatch(2, ProgVers::new(100, 6), 0, b"").await;
        assert_eq!(result, Err(RpcError::InvalidVersion { low: 1, high: 5 }));

        // Exact matches should still work
        let result = map.dispatch(2, ProgVers::new(100, 1), 0, b"").await;
        assert_eq!(result, Ok(vec![]));
        let result = map.dispatch(2, ProgVers::new(100, 3), 0, b"").await;
        assert_eq!(result, Ok(vec![]));
        let result = map.dispatch(2, ProgVers::new(100, 5), 0, b"").await;
        assert_eq!(result, Ok(vec![]));
    }

    #[tokio::test]
    async fn test_invalid_procedure() {
        let mut map = ServiceMap::new();
        map.add_service(Box::new(TestService { prog: 100, vers: 1 }));

        let result = map.dispatch(2, ProgVers::new(100, 1), 2, b"").await;
        assert_eq!(result, Err(RpcError::InvalidProcedure));

        let result = map.dispatch(2, ProgVers::new(100, 1), 999, b"").await;
        assert_eq!(result, Err(RpcError::InvalidProcedure));
    }

    #[tokio::test]
    async fn test_multiple_programs() {
        let mut map = ServiceMap::new();
        map.add_service(Box::new(TestService { prog: 100, vers: 1 }));
        map.add_service(Box::new(TestService { prog: 200, vers: 1 }));
        map.add_service(Box::new(TestService { prog: 200, vers: 2 }));

        let result = map.dispatch(2, ProgVers::new(100, 1), 0, b"").await;
        assert_eq!(result, Ok(vec![]));

        let result = map.dispatch(2, ProgVers::new(100, 2), 0, b"").await;
        assert_eq!(result, Err(RpcError::InvalidVersion { low: 1, high: 1 }));

        let result = map.dispatch(2, ProgVers::new(200, 1), 0, b"").await;
        assert_eq!(result, Ok(vec![]));

        let result = map.dispatch(2, ProgVers::new(200, 2), 0, b"").await;
        assert_eq!(result, Ok(vec![]));

        let result = map.dispatch(2, ProgVers::new(200, 3), 0, b"").await;
        assert_eq!(result, Err(RpcError::InvalidVersion { low: 1, high: 2 }));

        let result = map.dispatch(2, ProgVers::new(150, 1), 0, b"").await;
        assert_eq!(result, Err(RpcError::ProgUnavail));
    }

    #[tokio::test]
    async fn test_default_impl() {
        let map = ServiceMap::default();
        let result = map.dispatch(2, ProgVers::new(100, 1), 0, b"").await;
        assert_eq!(result, Err(RpcError::ProgUnavail));
    }
}
