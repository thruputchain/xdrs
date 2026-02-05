//! Reading XDR values from a serialization format.
//!
//! This module defines the [`Reader`] trait, which provides methods for
//! reading [XDR][RFC-4506] primitive types (as defined in RFC 4506)
//! from some underlying serialization format.
//! Implementations of this trait can be used with [`ReadXDR`](crate::ReadXDR)
//! to deserialize complex types.
//!
//! [RFC-4506]: https://datatracker.ietf.org/doc/html/rfc4506

#[cfg(feature = "alloc")]
use alloc::vec::Vec;

use crate::mu;
use core::mem::MaybeUninit as MU;
use thiserror::Error;

/// A reader that can produce XDR values, some borrowed with lifetime `'r`.
///
/// This trait provides methods for reading XDR primitive types from an
/// underlying serialization format. The lifetime parameter `'r` represents
/// the lifetime of data borrowed from the reader.
///
/// Implementations of this trait are used by [`ReadXDR`](crate::ReadXDR)
/// to deserialize complex types.
///
pub trait Reader<'r>: Sized {
    /// Reads a signed 64-bit integer.
    fn read_i64(&mut self) -> Result<i64, ReadError>;

    /// Reads a signed 32-bit integer.
    fn read_i32(&mut self) -> Result<i32, ReadError>;

    /// Reads a signed 16-bit integer.
    fn read_i16(&mut self) -> Result<i16, ReadError>;

    /// Reads an unsigned 64-bit integer.
    fn read_u64(&mut self) -> Result<u64, ReadError>;

    /// Reads an unsigned 32-bit integer.
    fn read_u32(&mut self) -> Result<u32, ReadError>;

    /// Reads an unsigned 16-bit integer.
    fn read_u16(&mut self) -> Result<u16, ReadError>;

    /// Reads a byte array of known length, borrowing the contents from the reader.
    fn read_opaque_fixed(&mut self, len: u32) -> Result<&'r [u8], ReadError>;

    /// Reads a byte sequence of variable length (not exceeding `max`), borrowing the contents from the reader.
    fn read_opaque_variable(&mut self, max: u32) -> Result<&'r [u8], ReadError>;

    /// Reads a boolean value.
    fn read_bool(&mut self) -> Result<bool, ReadError> {
        match self.read_i32()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(ReadError::BadBool),
        }
    }

    /// Reads a byte array of known length, copying into a provided buffer
    /// rather than borrowing from the reader.
    ///
    /// Panics if `dst.len()` is not equal to `len as usize`.
    ///
    fn read_opaque_fixed_into<'a>(
        &mut self,
        dst: &'a mut [MU<u8>],
        len: u32,
    ) -> Result<&'a mut [u8], ReadError> {
        debug_assert_eq!(dst.len(), len as usize);

        let data = self.read_opaque_fixed(len)?;

        let initialized = mu::write_copy_of_slice(dst, data);

        Ok(initialized)
    }

    /// Reads a byte sequence of variable length, copying into a provided buffer.
    /// rather than borrowing from the reader.
    ///
    /// Panics if `dst.len()` is less than the actual length of the input data.
    ///
    fn read_opaque_variable_into<'a>(
        &mut self,
        dst: &'a mut [MU<u8>],
        max: u32,
    ) -> Result<&'a mut [u8], ReadError> {
        debug_assert!(dst.len() >= max as usize);

        let len = self.read_u32()?;

        if len > max {
            return Err(ReadError::MaxLengthExceeded);
        }

        self.read_opaque_fixed_into(&mut dst[..len as usize], len)
    }

    /// Reads a byte array of known length, returning an owned `Vec<u8>`.
    ///
    /// *Requires feature `alloc`.*
    #[cfg(feature = "alloc")]
    #[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
    fn read_opaque_fixed_owned(&mut self, len: u32) -> Result<Vec<u8>, ReadError> {
        let bytes = self.read_opaque_fixed(len)?;
        let buf = Vec::from(bytes); // copy from input
        Ok(buf)
    }

    /// Reads a byte sequence of variable length, returning an owned `Vec<u8>`.
    ///
    /// *Requires feature `alloc`.*
    #[cfg(feature = "alloc")]
    #[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
    fn read_opaque_variable_owned(&mut self, max: u32) -> Result<Vec<u8>, ReadError> {
        let bytes = self.read_opaque_variable(max)?;
        let buf = Vec::from(bytes); // copy from input
        Ok(buf)
    }
}

/// Errors that can occur when reading XDR data.
#[derive(Copy, Clone, Debug, Error)]
pub enum ReadError {
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

    #[error("not the minimal-size encoding")]
    NotMinimalEncoding,
}
