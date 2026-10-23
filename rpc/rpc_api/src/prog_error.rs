//! Program-level error wrapper.

use std::fmt;

/// Generic error type for RPC program implementations.
///
/// Wraps any thread-safe error for use by generated RPC service traits.
/// Generated dispatchers map a returned `ProgramError` to
/// [`RpcError::SystemError`](crate::RpcError::SystemError); the underlying
/// error is not sent to the peer.
///
/// # Example
///
/// ```no_run
/// use thruput_rpc_api::ProgramError;
///
/// fn load_config() -> Result<Vec<u8>, ProgramError> {
///     let bytes = std::fs::read("config.bin")?;
///     Ok(bytes)
/// }
/// ```
#[derive(Debug)]
pub struct ProgramError(Box<dyn std::error::Error + Send + Sync>);

impl ProgramError {
    /// Create a new ProgramError from any error type.
    pub fn new<E>(err: E) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Self(Box::new(err))
    }

    /// Create a ProgramError from a string message.
    pub fn msg(msg: impl Into<String>) -> Self {
        Self(msg.into().into())
    }
}

impl std::error::Error for ProgramError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.0.source()
    }
}

impl fmt::Display for ProgramError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "rpc program error: {}", self.0)
    }
}

impl From<std::io::Error> for ProgramError {
    fn from(err: std::io::Error) -> Self {
        Self::new(err)
    }
}

impl From<Box<dyn std::error::Error + Send + Sync>> for ProgramError {
    fn from(err: Box<dyn std::error::Error + Send + Sync>) -> Self {
        Self(err)
    }
}
