//! Write XDR values in some format

use thiserror::Error;

#[derive(Copy, Clone, Debug, Error)]
pub enum Error {
    #[error("space exhausted")]
    SpaceExhausted,

    #[error("invalid array length (too large)")]
    InvalidArrayLength,
}

pub type Result<T = (), E = Error> = core::result::Result<T, E>;

pub trait Writer<F> {
    /// Write a byte array of specified length
    fn write_opaque_fixed(&mut self, bytes: &[u8]) -> Result<()>;

    /// Write a byte sequence of unspecified length
    fn write_opaque_variable(&mut self, bytes: &[u8]) -> Result<()>;

    fn write_i64(&mut self, n: i64) -> Result<()>;
    fn write_i32(&mut self, n: i32) -> Result<()>;
    fn write_i16(&mut self, n: i16) -> Result<()>;
    fn write_i8(&mut self, n: i8) -> Result<()>;
    fn write_u64(&mut self, n: u64) -> Result<()>;
    fn write_u32(&mut self, n: u32) -> Result<()>;
    fn write_u16(&mut self, n: u16) -> Result<()>;
    fn write_u8(&mut self, n: u8) -> Result<()>;

    fn write_bool(&mut self, b: bool) -> Result<()>;
}
