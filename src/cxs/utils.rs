//! Convenient interfaces to CXS serialization.
//!
//! This module provides high-level functions for encoding and decoding XDR
//! data using the CXS (Compact XDR Serialization) format.

use crate::cxs::buf_reader::*;
use crate::cxs::into_buf_writer::*;
use crate::*;
use core::mem::MaybeUninit as MU;

/// Decodes CXS-encoded XDR data from a byte slice.
///
/// This function reads the entire buffer and deserializes a value of type `T`.
/// It returns an error if there are any remaining bytes after decoding.
///
/// # Example
///
/// ```rust
/// use xdrs::cxs;
///
/// let data = [0u8]; // valid CXS encoding for the unsigned integer 0
/// let value: u32 = cxs::decode(&data)?;
/// # Ok::<(), xdrs::ReadError>(())
/// ```
pub fn decode<'r, 'a: 'r, T>(buf: &'a [u8]) -> ReadResult<T>
where
    T: ReadXDR<'r>,
{
    let mut reader = BufReader::new(buf);

    let value = read_xdr(&mut reader)?;

    debug_assert!(reader.position() <= buf.len());

    let remaining = buf.len().checked_sub(reader.position()).unwrap();
    if remaining > 0 {
        let remaining = remaining as u32;
        return Err(ReadError::SourceRemaining { remaining });
    }

    Ok(value)
}

/// Encodes a value as CXS-encoded XDR data into a pre-allocated buffer.
///
/// This function serializes the value into the provided buffer and returns the
/// number of bytes written. Returns `Err(WriteError::SpaceExhausted)` if the
/// buffer is not large enough to hold the serialized data.
///
/// # Example
///
/// ```rust
/// use xdrs::cxs;
/// use core::mem::MaybeUninit;
///
/// let value: u32 = 42;
///
/// let mut buf = [MaybeUninit::uninit(); 16];
///
/// let written = cxs::encode_into(&value, &mut buf)?;
/// # Ok::<(), xdrs::WriteError>(())
/// ```
pub fn encode_into<T: WriteXDR + ?Sized>(value: &T, buf: &mut [MU<u8>]) -> WriteResult<usize> {
    let mut writer = IntoBufWriter::new(buf);

    value.write_xdr(&mut writer)?;

    Ok(writer.written())
}

#[cfg(feature = "alloc")]
mod utils_alloc {
    use crate::cxs::buf_writer::*;
    use crate::*;
    use alloc::vec::Vec;

    /// Encodes a value as CXS-encoded XDR data, returning an owned vector.
    ///
    /// This function serializes the value and returns a `Vec<u8>` containing
    /// the serialized data.
    ///
    /// *Requires feature `alloc`.*
    ///
    /// # Example
    ///
    /// ```rust
    /// use xdrs::cxs;
    ///
    /// let value: u32 = 42;
    ///
    /// let encoded = cxs::encode(&value)?;
    /// # Ok::<(), xdrs::WriteError>(())
    /// ```
    #[cfg(feature = "alloc")]
    #[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
    pub fn encode<T: WriteXDR + ?Sized>(value: &T) -> WriteResult<Vec<u8>> {
        let mut writer = BufWriter::new();

        value.write_xdr(&mut writer)?;

        Ok(writer.into())
    }
}

#[cfg(feature = "alloc")]
pub use utils_alloc::*;
