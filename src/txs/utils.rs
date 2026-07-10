//! Convenient interfaces to TXS serialization.
//!
//! This module provides high-level functions for encoding and decoding XDR data
//! using the TXS (Traditional XDR Serialization) format.

use crate::txs::buf_reader::*;
use crate::txs::into_buf_writer::*;
use crate::txs::unchecked_buf_writer::*;
use crate::*;
use core::mem::MaybeUninit as MU;

/// Decodes TXS-encoded XDR data from a byte slice.
///
/// This function reads the entire buffer and deserializes a value of type `T`.
/// It returns an error if there are any remaining bytes after decoding.
///
/// # Example
///
/// ```rust
/// use xdrs::txs;
///
/// let data = [0u8; 4]; // valid TXS encoding for the unsigned integer 0
/// let value: u32 = txs::decode(&data)?;
/// # Ok::<(), xdrs::ReadError>(())
/// ```
pub fn decode<'r, 'a: 'r, T>(buf: &'a [u8]) -> ReadResult<T>
where
    T: ReadXDR<'r>,
{
    let mut reader = BufReader::new(buf);

    let value = read_xdr(&mut reader)?;

    debug_assert!(reader.position() <= buf.len());

    let remaining = buf.len() - reader.position();
    if remaining > 0 {
        let remaining = remaining as u64;
        return Err(ReadError::SourceRemaining { remaining });
    }

    Ok(value)
}

/// Encodes a value as TXS-encoded XDR data into a pre-allocated buffer.
///
/// This function serializes the value into the provided buffer and returns the
/// number of bytes written. Returns `Err(WriteError::SpaceExhausted)` if the
/// buffer is not large enough to hold the serialized data.
///
/// # Example
///
/// ```rust
/// use xdrs::txs;
/// use core::mem::MaybeUninit;
///
/// let value: u32 = 42;
///
/// let mut buf = [MaybeUninit::uninit(); 16];
///
/// let written = txs::encode_into(&value, &mut buf)?;
/// # Ok::<(), xdrs::WriteError>(())
/// ```
pub fn encode_into<T>(value: &T, buf: &mut [MU<u8>]) -> WriteResult<usize>
where
    T: WriteXDR + ?Sized,
{
    let mut writer = IntoBufWriter::new(buf);

    value.write_xdr(&mut writer)?;

    Ok(writer.written())
}

/// Encodes a value as TXS-encoded XDR data into a pre-allocated buffer,
/// without runtime checks.
///
/// This is the infallible, unsafe counterpart to [`encode_into`]: no bounds
/// checking is performed and no error can be returned, minimizing code size
/// on constrained targets. Returns the number of bytes written.
///
/// # Safety
///
/// - `buf.len()` must be at least the encoded size of `value`. With
///   `#[derive(ConstWriteXDR)]`, `const_max_txs_size()` (worst case over
///   all values) or `const_txs_size()` (exact for this value) provide
///   suitable sizes.
/// - `value` must respect all declared XDR bounds (e.g. a `VecM<T, MAX>`
///   must not be longer than `MAX`), since a violation would make the
///   encoding exceed any precomputed size bound.
///
/// Violations are caught by `debug_assert!` in debug builds; in release
/// builds they are undefined behavior.
///
/// # Example
///
/// ```rust
/// use xdrs::txs;
/// use core::mem::MaybeUninit;
///
/// let value: u32 = 42;
///
/// let mut buf = [MaybeUninit::uninit(); 4];
///
/// // SAFETY: a u32 always encodes to exactly 4 TXS bytes.
/// let written = unsafe { txs::encode_into_unchecked(&value, &mut buf) };
/// assert_eq!(written, 4);
/// ```
pub unsafe fn encode_into_unchecked<T>(value: &T, buf: &mut [MU<u8>]) -> usize
where
    T: WriteXDR + ?Sized,
{
    let mut writer = UncheckedBufWriter::new(buf);

    // SAFETY: the caller guarantees buf can hold the encoded form of value
    // and that value respects its declared bounds.
    unsafe { value.write_xdr_unchecked(&mut writer) };

    writer.written()
}

#[cfg(feature = "alloc")]
mod utils_alloc {
    use crate::txs::buf_writer::*;
    use crate::*;
    use alloc::vec::Vec;

    /// Encodes a value as TXS-encoded XDR data, returning an owned vector.
    ///
    /// This function serializes the value and returns a `Vec<u8>` containing
    /// the serialized data.
    ///
    /// *Requires feature `alloc`.*
    ///
    /// # Example
    ///
    /// ```rust
    /// use xdrs::txs;
    ///
    /// let value: u32 = 42;
    /// let encoded = txs::encode(&value)?;
    /// # Ok::<(), xdrs::WriteError>(())
    /// ```
    #[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
    pub fn encode<T>(value: &T) -> WriteResult<Vec<u8>>
    where
        T: WriteXDR + ?Sized,
    {
        let mut writer = BufWriter::new();

        value.write_xdr(&mut writer)?;

        Ok(writer.into())
    }
}

#[cfg(feature = "alloc")]
pub use utils_alloc::*;
