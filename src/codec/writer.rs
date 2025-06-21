//! Write XDR values in some format

use thiserror::Error;
use crate::write_xdr::WriteXDR;

#[derive(Copy, Clone, Debug, Error)]
pub enum Error {
    #[error("space exhausted")]
    SpaceExhausted,

    #[error("invalid array length (too large)")]
    InvalidArrayLength,
}

pub type Result<T = (), E = Error> = core::result::Result<T, E>;

pub trait Writer {
    type SW: StructWriter;

    fn write_struct(self) -> Result<Self::SW>;

    /// Write a byte array of specified length
    fn write_byte_array(self, bytes: &[u8]) -> Result<()>;

    /// Write a byte sequence of unspecified length
    fn write_bytes(self, bytes: &[u8]) -> Result<()>;

    fn write_i64(self, n: i64) -> Result<()>;
    fn write_i32(self, n: i32) -> Result<()>;
    fn write_i16(self, n: i16) -> Result<()>;
    fn write_i8(self, n: i8) -> Result<()>;
    fn write_u64(self, n: u64) -> Result<()>;
    fn write_u32(self, n: u32) -> Result<()>;
    fn write_u16(self, n: u16) -> Result<()>;
    fn write_u8(self, n: u8) -> Result<()>;
}

pub trait StructWriter {
    fn write_field<T: WriteXDR>(&mut self, value: &T) -> Result<()>;
}
