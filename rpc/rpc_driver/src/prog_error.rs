use std::fmt;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub struct ProgError(String);

impl fmt::Display for ProgError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "rpc program error: {}", self.0)
    }
}
