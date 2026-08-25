//! RPC handler dispatch and service registration.

use crate::{ProgVers, RpcError};
use std::collections::BTreeMap;

// The RpcHandler trait lives in the stable `rpc_api` crate; re-exported here
// at its historical path for source compatibility.
pub use rpc_api::RpcHandler;

/// Registry of RPC services, keyed by program number and version.
pub struct ServiceMap {
    services: BTreeMap<ProgVers, Box<dyn RpcHandler>>,
}

impl std::fmt::Debug for ServiceMap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServiceMap")
            .field(
                "services",
                &format_args!("{} registered", self.services.len()),
            )
            .finish()
    }
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
    ///
    /// Replaces any existing handler for the same program/version pair.
    pub fn add_service(&mut self, dispatch: Box<dyn RpcHandler>) {
        self.services.insert(dispatch.prog_vers(), dispatch);
    }

    pub(crate) async fn dispatch(
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

        let first_match = self.services.range(ProgVers::new(srv.prog(), 0)..);

        if first_match
            .clone()
            .next()
            .is_none_or(|entry| entry.0.prog() != srv.prog())
        {
            return Err(RpcError::ProgUnavail);
        }

        let low_vers = first_match.clone().next().unwrap().0.vers();
        let high_vers = self
            .services
            .range(..=ProgVers::new(srv.prog(), 0xffffffff))
            .next_back()
            .unwrap()
            .0
            .vers();

        Err(RpcError::InvalidVersion {
            low: low_vers,
            high: high_vers,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

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
