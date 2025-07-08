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

    fn read_u64(&mut self) -> Result<u64>;
    fn read_u32(&mut self) -> Result<u32>;
    fn read_u16(&mut self) -> Result<u16>;

    /// Read a byte-array of known length, borrowing the contents from the reader
    fn read_opaque_fixed(&mut self, len: u32) -> Result<&'r [u8]>;

    /// Read a byte-sequence of specified max length, borrowing the contents from the reader
    fn read_opaque_variable(&mut self, max: u32) -> Result<&'r [u8]>;

    fn read_bool(&mut self) -> Result<bool> {
        match self.read_i32()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(Error::BadBool),
        }
    }

    /// Read a byte-array of known length, copying into a provided buffer.
    /// Panics if `dst.len()` is not equal to `len as usize`.
    fn read_opaque_fixed_into<'a>(
        &mut self,
        dst: &'a mut [MU<u8>],
        len: u32,
    ) -> Result<&'a mut [u8]> {
        debug_assert_eq!(dst.len(), len as usize);

        let data = self.read_opaque_fixed(len)?;

        let initialized = dst.write_copy_of_slice(data);

        Ok(initialized)
    }

    /// Read a byte-sequence of specified max length, copying into a provided buffer.
    /// Panics if `dst.len()` is less than the actual length of the input data.
    fn read_opaque_variable_into<'a>(
        &mut self,
        dst: &'a mut [MU<u8>],
        max: u32,
    ) -> Result<&'a mut [u8]> {
        debug_assert!(dst.len() >= max as usize);

        let len = self.read_u32()?;

        if len > max {
            return Err(Error::MaxLengthExceeded);
        }

        self.read_opaque_fixed_into(&mut dst[..len as usize], len)
    }

    #[cfg(feature = "alloc")]
    fn read_opaque_fixed_owned(&mut self, len: u32) -> Result<Vec<u8>> {
        let bytes = self.read_opaque_fixed(len)?;
        let buf = Vec::from(bytes); // copy from input
        Ok(buf)
    }

    #[cfg(feature = "alloc")]
    fn read_opaque_variable_owned(&mut self, max: u32) -> Result<Vec<u8>> {
        let bytes = self.read_opaque_variable(max)?;
        let buf = Vec::from(bytes); // copy from input
        Ok(buf)
    }
}

pub type Result<T, E = Error> = core::result::Result<T, E>;

#[derive(Copy, Clone, Debug, Error)]
pub enum Error {
    #[error(
        "unexpected end of input (requested: {}, remaining: {})",
        requested,
        remaining
    )]
    SourceEmpty { requested: u32, remaining: u32 },

    #[error("input remaining after decoding: {} bytes", remaining)]
    SourceRemaining { remaining: u32 },

    #[error("invalid header byte: 0x{h:02x}")]
    InvalidHeaderByte { h: u8 },

    #[error("decoded integer out of range")]
    IntegerOutOfRange,

    #[error("bad bool value")]
    BadBool,

    #[error("unexpected discriminator value: {0}")]
    BadDiscriminator(i32),

    #[error("length exceeds array maximum")]
    MaxLengthExceeded,

    #[error("bad UTF8")]
    BadUTF8,
}
