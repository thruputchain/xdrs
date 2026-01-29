//! Server-side service dispatch and registration.

use crate::{ProgVers, rpc_error::RpcError};
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
    impl ServerDispatch for TestService {
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

        // RPC version 1 should fail
        let result = map.dispatch(1, ProgVers::new(100, 1), 0, b"").await;
        assert_eq!(result, Err(RpcError::RpcMismatch));

        // RPC version 3 should fail
        let result = map.dispatch(3, ProgVers::new(100, 1), 0, b"").await;
        assert_eq!(result, Err(RpcError::RpcMismatch));

        // RPC version 0 should fail
        let result = map.dispatch(0, ProgVers::new(100, 1), 0, b"").await;
        assert_eq!(result, Err(RpcError::RpcMismatch));
    }

    #[tokio::test]
    async fn test_exact_prog_vers_match() {
        let mut map = ServiceMap::new();
        map.add_service(Box::new(TestService { prog: 100, vers: 1 }));

        // Exact match should dispatch successfully
        let result = map.dispatch(2, ProgVers::new(100, 1), 0, b"").await;
        assert_eq!(result, Ok(vec![]));

        // Echo procedure
        let result = map.dispatch(2, ProgVers::new(100, 1), 1, b"hello").await;
        assert_eq!(result, Ok(b"hello".to_vec()));
    }

    #[tokio::test]
    async fn test_program_unavailable() {
        let mut map = ServiceMap::new();
        map.add_service(Box::new(TestService { prog: 100, vers: 1 }));

        // Different program number should return ProgUnavail
        let result = map.dispatch(2, ProgVers::new(200, 1), 0, b"").await;
        assert_eq!(result, Err(RpcError::ProgUnavail));

        // Program 99 (before 100) should return ProgUnavail
        let result = map.dispatch(2, ProgVers::new(99, 1), 0, b"").await;
        assert_eq!(result, Err(RpcError::ProgUnavail));
    }

    #[tokio::test]
    async fn test_empty_service_map() {
        let map = ServiceMap::new();

        // Any request to empty map should return ProgUnavail
        let result = map.dispatch(2, ProgVers::new(100, 1), 0, b"").await;
        assert_eq!(result, Err(RpcError::ProgUnavail));
    }

    #[tokio::test]
    async fn test_version_mismatch_single_version() {
        let mut map = ServiceMap::new();
        map.add_service(Box::new(TestService { prog: 100, vers: 2 }));

        // Request version 1 when only version 2 exists
        let result = map.dispatch(2, ProgVers::new(100, 1), 0, b"").await;
        assert_eq!(result, Err(RpcError::InvalidVersion { low: 2, high: 2 }));

        // Request version 3 when only version 2 exists
        let result = map.dispatch(2, ProgVers::new(100, 3), 0, b"").await;
        assert_eq!(result, Err(RpcError::InvalidVersion { low: 2, high: 2 }));
    }

    #[tokio::test]
    async fn test_version_mismatch_multiple_versions() {
        let mut map = ServiceMap::new();
        map.add_service(Box::new(TestService { prog: 100, vers: 1 }));
        map.add_service(Box::new(TestService { prog: 100, vers: 3 }));
        map.add_service(Box::new(TestService { prog: 100, vers: 5 }));

        // Request version 2 (between 1 and 3)
        let result = map.dispatch(2, ProgVers::new(100, 2), 0, b"").await;
        assert_eq!(result, Err(RpcError::InvalidVersion { low: 1, high: 5 }));

        // Request version 4 (between 3 and 5)
        let result = map.dispatch(2, ProgVers::new(100, 4), 0, b"").await;
        assert_eq!(result, Err(RpcError::InvalidVersion { low: 1, high: 5 }));

        // Request version 0 (below lowest)
        let result = map.dispatch(2, ProgVers::new(100, 0), 0, b"").await;
        assert_eq!(result, Err(RpcError::InvalidVersion { low: 1, high: 5 }));

        // Request version 6 (above highest)
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

        // Procedure 2 is not implemented in TestService
        let result = map.dispatch(2, ProgVers::new(100, 1), 2, b"").await;
        assert_eq!(result, Err(RpcError::InvalidProcedure));

        // Large procedure number
        let result = map.dispatch(2, ProgVers::new(100, 1), 999, b"").await;
        assert_eq!(result, Err(RpcError::InvalidProcedure));
    }

    #[tokio::test]
    async fn test_multiple_programs() {
        let mut map = ServiceMap::new();
        map.add_service(Box::new(TestService { prog: 100, vers: 1 }));
        map.add_service(Box::new(TestService { prog: 200, vers: 1 }));
        map.add_service(Box::new(TestService { prog: 200, vers: 2 }));

        // Program 100 version 1 should work
        let result = map.dispatch(2, ProgVers::new(100, 1), 0, b"").await;
        assert_eq!(result, Ok(vec![]));

        // Program 100 version 2 should return version mismatch (only v1 exists)
        let result = map.dispatch(2, ProgVers::new(100, 2), 0, b"").await;
        assert_eq!(result, Err(RpcError::InvalidVersion { low: 1, high: 1 }));

        // Program 200 version 1 should work
        let result = map.dispatch(2, ProgVers::new(200, 1), 0, b"").await;
        assert_eq!(result, Ok(vec![]));

        // Program 200 version 2 should work
        let result = map.dispatch(2, ProgVers::new(200, 2), 0, b"").await;
        assert_eq!(result, Ok(vec![]));

        // Program 200 version 3 should return version mismatch
        let result = map.dispatch(2, ProgVers::new(200, 3), 0, b"").await;
        assert_eq!(result, Err(RpcError::InvalidVersion { low: 1, high: 2 }));

        // Program 150 (between 100 and 200) should return ProgUnavail
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
