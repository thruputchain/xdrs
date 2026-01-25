//! Program-level error wrapper.

use std::fmt;

/// Generic error type for RPC program implementations.
///
/// Wraps any error type and converts to `RpcError::SystemError` when returned.
#[derive(Debug, thiserror::Error)]
pub struct ProgramError(Box<dyn std::error::Error + Send + Sync>);

impl fmt::Display for ProgramError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "rpc program error: {}", self.0.to_string())
    }
}
