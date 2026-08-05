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

use core::mem::MaybeUninit as MU;

/// A reader that can produce XDR values, some borrowed with lifetime `'r`.
///
/// This trait provides methods for reading XDR primitive types from an
/// underlying serialization format. The lifetime parameter `'r` represents
/// the lifetime of data borrowed from the reader.
///
/// Implementations of this trait are used by [`ReadXDR`](crate::ReadXDR)
/// to deserialize complex types.
///
/// # Safety
///
/// [`read_opaque_fixed`](Reader::read_opaque_fixed) must return a slice
/// of exactly `len` bytes on success. Unsafe code in this crate relies on
/// that invariant.
pub unsafe trait Reader<'r>: Sized {
    /// Reads a signed 64-bit integer.
    fn read_i64(&mut self) -> ReadResult<i64>;

    /// Reads a signed 32-bit integer.
    fn read_i32(&mut self) -> ReadResult<i32>;

    /// Reads a signed 16-bit integer.
    fn read_i16(&mut self) -> ReadResult<i16>;

    /// Reads an unsigned 64-bit integer.
    fn read_u64(&mut self) -> ReadResult<u64>;

    /// Reads an unsigned 32-bit integer.
    fn read_u32(&mut self) -> ReadResult<u32>;

    /// Reads an unsigned 16-bit integer.
    fn read_u16(&mut self) -> ReadResult<u16>;

    /// Reads a 32-bit IEEE 754 floating-point value.
    fn read_f32(&mut self) -> ReadResult<f32>;

    /// Reads a 64-bit IEEE 754 floating-point value.
    fn read_f64(&mut self) -> ReadResult<f64>;

    /// Reads a byte array of known length, borrowing the contents from the reader.
    ///
    /// On success, the returned slice must have exactly `len` bytes.
    /// Callers may rely on this invariant for soundness.
    fn read_opaque_fixed(&mut self, len: usize) -> ReadResult<&'r [u8]>;

    /// Reads a byte sequence of variable length (not exceeding `max`), borrowing the contents from the reader.
    fn read_opaque_variable(&mut self, max: u32) -> ReadResult<&'r [u8]>;

    /// Reads a boolean value.
    fn read_bool(&mut self) -> ReadResult<bool> {
        match self.read_i32()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(ReadError::bad_bool()),
        }
    }

    /// Reads a byte array of known length, copying into a provided buffer
    /// rather than borrowing from the reader.
    ///
    /// Panics if `dst.len()` does not equal `len as usize`.
    ///
    fn read_opaque_fixed_into<'a>(
        &mut self,
        dst: &'a mut [MU<u8>],
        len: u32,
    ) -> ReadResult<&'a mut [u8]> {
        debug_assert_eq!(dst.len(), len as usize);

        const _: () = assert!(core::mem::size_of::<usize>() >= core::mem::size_of::<u32>());

        let data = self.read_opaque_fixed(len as usize)?;

        // Cannot panic: read_opaque_fixed returns exactly `len` bytes, and
        // dst.len() == len as usize (caller invariant).
        let initialized = dst.write_copy_of_slice(data);

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
    ) -> ReadResult<&'a mut [u8]> {
        debug_assert!(dst.len() >= max as usize);

        let len = self.read_u32()?;

        if len > max {
            return Err(ReadError::max_length_exceeded());
        }

        // Panic-safe: dst[..len as usize] has exactly `len` elements.
        self.read_opaque_fixed_into(&mut dst[..len as usize], len)
    }

    /// Reads a byte array of known length, returning an owned `Vec<u8>`.
    ///
    /// *Requires feature `alloc`.*
    #[cfg(feature = "alloc")]
    #[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
    fn read_opaque_fixed_owned(&mut self, len: u32) -> ReadResult<Vec<u8>> {
        let bytes = self.read_opaque_fixed(len as usize)?;
        let buf = Vec::from(bytes); // copy from input
        Ok(buf)
    }

    /// Reads a byte sequence of variable length, returning an owned `Vec<u8>`.
    ///
    /// *Requires feature `alloc`.*
    #[cfg(feature = "alloc")]
    #[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
    fn read_opaque_variable_owned(&mut self, max: u32) -> ReadResult<Vec<u8>> {
        let bytes = self.read_opaque_variable(max)?;
        let buf = Vec::from(bytes); // copy from input
        Ok(buf)
    }
}

/// A result type for reader operations.
pub type ReadResult<T> = core::result::Result<T, ReadError>;

/// Errors that can occur when reading XDR data.
#[cfg(feature = "detailed-errors")]
#[derive(Copy, Clone, Debug)]
pub enum ReadError {
    /// The input ended before the requested number of bytes could be read.
    SourceEmpty {
        /// The number of bytes the reader tried to consume.
        requested: usize,
        /// The number of bytes left in the input.
        remaining: usize,
    },

    /// Decoding completed but bytes remain unconsumed in the input.
    SourceRemaining {
        /// The number of unconsumed bytes left in the input.
        remaining: u64,
    },

    /// A CXS integer header byte used a reserved combination of its
    /// size/value bit fields (see `CXS.md`).
    InvalidHeaderByte {
        /// The invalid header byte.
        h: u8,
    },

    /// The decoded integer does not fit in the target integer type.
    IntegerOutOfRange,

    /// A decoded boolean was neither 0 (`FALSE`) nor 1 (`TRUE`).
    BadBool,

    /// A decoded union discriminant did not match any known variant.
    BadDiscriminator(i32),

    /// A decoded length exceeds the maximum declared for the
    /// variable-length array, opaque data, or string.
    MaxLengthExceeded,

    /// A TXS padding byte was non-zero (rejected unless feature
    /// `permissive` is enabled).
    NonZeroPadding,

    /// A decoded string was not valid UTF-8.
    BadUTF8,

    /// A CXS integer was not encoded in the minimal number of bytes
    /// (rejected unless feature `permissive` is enabled).
    NotMinimalEncoding,
}

/// An error that occurred while reading XDR data.
///
/// Compact builds intentionally omit diagnostic details to minimize contract
/// code size. Enable the default `detailed-errors` feature to retain precise
/// error variants and payloads.
#[cfg(not(feature = "detailed-errors"))]
#[derive(Copy, Clone, Debug)]
pub struct ReadError;

impl ReadError {
    #[doc(hidden)]
    #[inline]
    pub fn source_empty(requested: usize, remaining: usize) -> Self {
        #[cfg(feature = "detailed-errors")]
        {
            Self::SourceEmpty {
                requested,
                remaining,
            }
        }
        #[cfg(not(feature = "detailed-errors"))]
        {
            let _ = (requested, remaining);
            Self
        }
    }

    #[doc(hidden)]
    #[inline]
    pub fn source_remaining(remaining: u64) -> Self {
        #[cfg(feature = "detailed-errors")]
        {
            Self::SourceRemaining { remaining }
        }
        #[cfg(not(feature = "detailed-errors"))]
        {
            let _ = remaining;
            Self
        }
    }

    #[doc(hidden)]
    #[inline]
    pub fn invalid_header_byte(h: u8) -> Self {
        #[cfg(feature = "detailed-errors")]
        {
            Self::InvalidHeaderByte { h }
        }
        #[cfg(not(feature = "detailed-errors"))]
        {
            let _ = h;
            Self
        }
    }

    #[doc(hidden)]
    #[inline]
    pub fn integer_out_of_range() -> Self {
        #[cfg(feature = "detailed-errors")]
        {
            Self::IntegerOutOfRange
        }
        #[cfg(not(feature = "detailed-errors"))]
        {
            Self
        }
    }

    #[doc(hidden)]
    #[inline]
    pub fn bad_bool() -> Self {
        #[cfg(feature = "detailed-errors")]
        {
            Self::BadBool
        }
        #[cfg(not(feature = "detailed-errors"))]
        {
            Self
        }
    }

    #[doc(hidden)]
    #[inline]
    pub fn bad_discriminator(discriminant: i32) -> Self {
        #[cfg(feature = "detailed-errors")]
        {
            Self::BadDiscriminator(discriminant)
        }
        #[cfg(not(feature = "detailed-errors"))]
        {
            let _ = discriminant;
            Self
        }
    }

    #[doc(hidden)]
    #[inline]
    pub fn max_length_exceeded() -> Self {
        #[cfg(feature = "detailed-errors")]
        {
            Self::MaxLengthExceeded
        }
        #[cfg(not(feature = "detailed-errors"))]
        {
            Self
        }
    }

    #[doc(hidden)]
    #[inline]
    pub fn non_zero_padding() -> Self {
        #[cfg(feature = "detailed-errors")]
        {
            Self::NonZeroPadding
        }
        #[cfg(not(feature = "detailed-errors"))]
        {
            Self
        }
    }

    #[doc(hidden)]
    #[inline]
    pub fn bad_utf8() -> Self {
        #[cfg(feature = "detailed-errors")]
        {
            Self::BadUTF8
        }
        #[cfg(not(feature = "detailed-errors"))]
        {
            Self
        }
    }

    #[doc(hidden)]
    #[inline]
    pub fn not_minimal_encoding() -> Self {
        #[cfg(feature = "detailed-errors")]
        {
            Self::NotMinimalEncoding
        }
        #[cfg(not(feature = "detailed-errors"))]
        {
            Self
        }
    }
}

#[cfg(feature = "detailed-errors")]
impl core::fmt::Display for ReadError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ReadError::SourceEmpty {
                requested,
                remaining,
            } => write!(
                f,
                "unexpected end of input (requested: {requested}, remaining: {remaining})"
            ),
            ReadError::SourceRemaining { remaining } => {
                write!(f, "input remaining after decoding: {remaining} bytes")
            }
            ReadError::InvalidHeaderByte { h } => write!(f, "invalid header byte: 0x{h:02x}"),
            ReadError::IntegerOutOfRange => write!(f, "decoded integer out of range"),
            ReadError::BadBool => write!(f, "bad bool value"),
            ReadError::BadDiscriminator(d) => write!(f, "unexpected discriminator value: {d}"),
            ReadError::MaxLengthExceeded => write!(f, "length exceeds array maximum"),
            ReadError::NonZeroPadding => write!(f, "non-zero XDR padding"),
            ReadError::BadUTF8 => write!(f, "bad UTF8"),
            ReadError::NotMinimalEncoding => write!(f, "not the minimal-size encoding"),
        }
    }
}

#[cfg(not(feature = "detailed-errors"))]
impl core::fmt::Display for ReadError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("XDR decoding failed")
    }
}

impl core::error::Error for ReadError {}
