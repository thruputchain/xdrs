//! Read XDR values from some format

#[cfg(feature = "alloc")]
use alloc::vec::Vec;

use core::mem::MaybeUninit as MU;
use thiserror::Error;

/// Something that can produce XDR values, some borrowed with lifetime 'r
pub trait Reader<'r>: Sized {
    fn read_i64(&mut self) -> Result<i64>;
    fn read_i32(&mut self) -> Result<i32>;
    fn read_i16(&mut self) -> Result<i16>;
    fn read_i8(&mut self) -> Result<i8>;

    fn read_u64(&mut self) -> Result<u64>;
    fn read_u32(&mut self) -> Result<u32>;
    fn read_u16(&mut self) -> Result<u16>;
    fn read_u8(&mut self) -> Result<u8>;

    /// Read a byte-array of known length, borrowing the contents from the reader
    fn read_byte_array(&mut self, len: u32) -> Result<&'r [u8]>;

    /// Read a byte-array of known length into a provided buffer
    fn read_byte_array_into(&mut self, dst: &mut [MU<u8>], len: u32) -> Result<()>;

    /// Read a byte-sequence of unknown length, borrowing the contents from the reader
    fn read_bytes(&mut self) -> Result<&'r [u8]>;

    fn read_array<V>(
        &mut self,
        len: usize,
        visit_element: V,
    ) -> Result<()>
        where V: FnMut (&mut Self) -> Result<()>;

    fn read_optional<V>(&mut self, visit_opt: V) -> Result<()>
        where V: FnOnce (Option<&mut Self>) -> Result<()>;

    fn read_struct<V>(
        &mut self,
        nfields: u32,
        visit_field: V
    ) -> Result<()>
        where V: FnMut (u32, &mut Self) -> Result<()>;

    // Provided methods

    fn read_bool(&mut self) -> Result<bool> {
        match self.read_i32()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(Error::BadBool)
        }
    }

    #[cfg(feature = "alloc")]
    fn read_byte_array_owned(&mut self, len: u32) -> Result<Vec<u8>> {
        let bytes = self.read_byte_array(len)?;
        let buf = Vec::from(bytes); // copy from input
        Ok(buf)
    }
}

pub type Result<T, E = Error> = core::result::Result<T, E>;

#[derive(Copy, Clone, Debug, Error)]
pub enum Error {
    #[error("unexpected end of input (requested: {}, remaining: {})",
        requested,
        remaining,
        )]
    SourceEmpty {
        requested: u32,
        remaining: u32,
    },

    #[error("input remaining after decoding: {} bytes", remaining)]
    SourceRemaining {
        remaining: u32,
    },

    #[error("invalid header byte: 0x{h:02x}")]
    InvalidHeaderByte {
        h: u8
    },

    #[error("encoded integer out of range")]
    IntegerOutOfRange,

    #[error("bad bool value")]
    BadBool,

    #[error("bad field index")]
    BadFieldIndex,
}
