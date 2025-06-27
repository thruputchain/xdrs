//! Write XDR values in some format

use crate::write_xdr::WriteXDR;
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
    type SW: StructWriter<F>;
    type EW: EnumWriter<F>;

    fn write_struct(self, n_fields: usize) -> Result<Self::SW>;

    fn write_enum(self, discriminant: i32, nfields: usize) -> Result<Self::EW>;

    /// Write a byte array of specified length
    fn write_byte_array(self, bytes: &[u8]) -> Result<()>;

    /// Write a byte sequence of unspecified length
    fn write_bytes(self, bytes: &[u8]) -> Result<()>;

    fn write_opt<T: WriteXDR>(self, v: &Option<T>) -> Result<()>;

    fn write_array<T, const N: usize>(self, array: &[T; N]) -> Result<()>
    where
        T: WriteXDR;

    fn write_i64(self, n: i64) -> Result<()>;
    fn write_i32(self, n: i32) -> Result<()>;
    fn write_i16(self, n: i16) -> Result<()>;
    fn write_i8(self, n: i8) -> Result<()>;
    fn write_u64(self, n: u64) -> Result<()>;
    fn write_u32(self, n: u32) -> Result<()>;
    fn write_u16(self, n: u16) -> Result<()>;
    fn write_u8(self, n: u8) -> Result<()>;

    fn write_bool(self, b: bool) -> Result<()>;
}

pub trait StructWriter<F> {
    fn write_field<T: WriteXDR>(&mut self, value: &T) -> Result<()>;
}

pub trait EnumWriter<F> {
    fn write_field<T: WriteXDR>(&mut self, value: &T) -> Result<()>;
}
