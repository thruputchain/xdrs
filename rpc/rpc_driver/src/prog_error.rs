use std::fmt;

// A generic error type that can be returned by an rpc implementation.
// This type is converted into an RpcError::SystemError automatically.
#[derive(Debug, thiserror::Error)]
pub struct ProgramError(Box<dyn std::error::Error + Send + Sync>);

impl fmt::Display for ProgramError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "rpc program error: {}", self.0.to_string())
    }
}
