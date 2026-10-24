#![warn(missing_docs)]

//! Consumer fixture: a hand-written copy of the shape x23c emits for
//! protocol bindings, written against `thruput_rpc_api` instead of `thruput_rpc_driver`.
//!
//! The lib target depends on `thruput_rpc_api` only — compiling it proves that
//! generated clients and dispatchers need no direct `thruput_rpc_driver`
//! dependency. The integration tests (see `tests/`) wire this fixture
//! through a real `thruput_rpc_driver` server and client to prove both sides share
//! one type identity through the `thruput_rpc_api` boundary.

use std::sync::Arc;

use thruput_rpc_api::{ProgVers, ProgramError, RpcError, RpcHandler, RpcSender};

/// Program number for the fixture echo service.
pub const ECHO_PROG: u32 = 3123125;
/// Program version for the fixture echo service.
pub const ECHO_VERS: u32 = 1;

/// Service trait, mirroring the trait x23c generates per program version.
#[async_trait::async_trait]
pub trait EchoService: Send + Sync {
    /// Null procedure (proc 0).
    async fn null(&self) -> Result<(), ProgramError>;
    /// Echo the payload back (proc 1).
    async fn echo(&self, data: Vec<u8>) -> Result<Vec<u8>, ProgramError>;
}

/// Server-side dispatcher, mirroring the `*_Dispatcher` x23c generates.
pub struct EchoDispatcher {
    inner: Box<dyn EchoService>,
}

impl EchoDispatcher {
    /// Wrap a service implementation for registration with a service map.
    pub fn new(inner: Box<dyn EchoService>) -> Self {
        Self { inner }
    }
}

impl std::fmt::Debug for EchoDispatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EchoDispatcher").finish()
    }
}

#[async_trait::async_trait]
impl RpcHandler for EchoDispatcher {
    fn prog_vers(&self) -> ProgVers {
        ProgVers::new(ECHO_PROG, ECHO_VERS)
    }

    async fn dispatch(&self, proc: u32, data: &[u8]) -> Result<Vec<u8>, RpcError> {
        match proc {
            0 => {
                self.inner.null().await.map_err(|_| RpcError::SystemError)?;
                Ok(vec![])
            }
            1 => self
                .inner
                .echo(data.to_vec())
                .await
                .map_err(|_| RpcError::SystemError),
            _ => Err(RpcError::InvalidProcedure),
        }
    }
}

/// Client wrapper, mirroring the `*_Client` x23c generates.
#[derive(Clone)]
pub struct EchoClient {
    handle: Arc<dyn RpcSender>,
}

impl std::fmt::Debug for EchoClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EchoClient").finish()
    }
}

impl EchoClient {
    /// Wrap any [`RpcSender`] (a driver connection, a pool, or a mock).
    pub fn new(handle: Arc<dyn RpcSender>) -> Self {
        Self { handle }
    }

    fn prog_vers() -> ProgVers {
        ProgVers::new(ECHO_PROG, ECHO_VERS)
    }

    /// Call the null procedure (proc 0).
    pub async fn null(&self) -> Result<(), RpcError> {
        // SAFETY: the callback lives across the await and retains no slice.
        unsafe {
            self.handle
                .send_call_decoded(
                    Self::prog_vers(),
                    0,
                    self.handle.call_buf(),
                    &mut |_| Ok(()),
                )
                .await
        }
    }

    /// Call the echo procedure (proc 1).
    pub async fn echo(&self, data: &[u8]) -> Result<Vec<u8>, RpcError> {
        let mut call = self.handle.call_buf();
        call.extend_from_slice(data);
        let mut result = None;
        // SAFETY: the output lives in this pinned frame until the call finishes.
        unsafe {
            self.handle
                .send_call_decoded(Self::prog_vers(), 1, call, &mut |bytes| {
                    result = Some(bytes.to_vec());
                    Ok(())
                })
                .await?;
        }
        Ok(result.unwrap())
    }
}

impl From<Arc<dyn RpcSender>> for EchoClient {
    fn from(handle: Arc<dyn RpcSender>) -> Self {
        Self::new(handle)
    }
}
