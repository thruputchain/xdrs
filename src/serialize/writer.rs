//! Writing XDR values to a serialization format.
//!
//! This module defines the [`Writer`] trait, which provides methods for
//! writing [XDR][RFC-4506] primitive types (as defined in RFC 4506)
//! to some underlying serialization format.
//! Implementations of this trait can be used with [`WriteXDR`](crate::WriteXDR)
//! to serialize complex types.
//!
//! [RFC-4506]: https://datatracker.ietf.org/doc/html/rfc4506

/// Errors that can occur when writing XDR data.
#[derive(Copy, Clone, Debug)]
pub enum WriteError {
    /// The writer has run out of space.
    SpaceExhausted,

    /// An array length is too large to be represented in XDR.
    InvalidArrayLength,
}

impl core::fmt::Display for WriteError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            WriteError::SpaceExhausted => write!(f, "space exhausted"),
            WriteError::InvalidArrayLength => write!(f, "invalid array length (too large)"),
        }
    }
}

impl core::error::Error for WriteError {}

/// A result type for writer operations.
pub type WriteResult<T = ()> = core::result::Result<T, WriteError>;

/// Something that can write XDR values to a serialization format.
///
/// This trait provides methods for writing XDR primitive types to an
/// underlying serialization format. The type parameter `F` represents
/// the format being written (e.g., `CXS` or `TXS`).
///
/// Implementations of this trait are used by [`WriteXDR`](crate::WriteXDR)
/// to serialize complex types.
///
pub trait Writer<F> {
    // Callers must ensure bytes.len() <= u32::MAX; only debug-asserted.
    #[doc(hidden)]
    fn write_opaque_fixed(&mut self, bytes: &[u8]) -> WriteResult<()>;

    /// Writes a byte sequence of variable length.
    fn write_opaque_variable(&mut self, bytes: &[u8]) -> WriteResult<()>;

    /// Writes a byte sequence of variable length that is held as consecutive
    /// chunks.
    ///
    /// The encoding is that of [`write_opaque_variable`](Self::write_opaque_variable)
    /// on the chunks' concatenation. This crate's formats write each chunk in
    /// place. The default implementation copies the chunks into one buffer
    /// first, so a writer defined elsewhere encodes correctly without
    /// overriding it.
    #[cfg(feature = "alloc")]
    fn write_opaque_variable_chunks<B: AsRef<[u8]>>(&mut self, chunks: &[B]) -> WriteResult<()>
    where
        Self: Sized,
    {
        self.write_opaque_variable(&concat_chunks(chunks))
    }

    /// Writes a signed 64-bit integer.
    fn write_i64(&mut self, n: i64) -> WriteResult<()>;

    /// Writes a signed 32-bit integer.
    fn write_i32(&mut self, n: i32) -> WriteResult<()>;

    /// Writes a signed 16-bit integer.
    fn write_i16(&mut self, n: i16) -> WriteResult<()>;

    /// Writes an unsigned 64-bit integer.
    fn write_u64(&mut self, n: u64) -> WriteResult<()>;

    /// Writes an unsigned 32-bit integer.
    fn write_u32(&mut self, n: u32) -> WriteResult<()>;

    /// Writes an unsigned 16-bit integer.
    fn write_u16(&mut self, n: u16) -> WriteResult<()>;

    /// Writes a boolean value.
    fn write_bool(&mut self, b: bool) -> WriteResult<()>;

    /// Writes a 32-bit IEEE 754 floating-point value.
    fn write_f32(&mut self, n: f32) -> WriteResult<()>;

    /// Writes a 64-bit IEEE 754 floating-point value.
    fn write_f64(&mut self, n: f64) -> WriteResult<()>;
}

/// The total length of `chunks`, or `None` if it does not fit in an XDR
/// length.
#[cfg(feature = "alloc")]
pub(crate) fn opaque_chunks_len<B: AsRef<[u8]>>(chunks: &[B]) -> Option<u32> {
    chunks.iter().try_fold(0u32, |total, chunk| {
        u32::try_from(chunk.as_ref().len())
            .ok()
            .and_then(|len| total.checked_add(len))
    })
}

/// The number of zero bytes that pad `len` bytes of opaque data to a 4-byte
/// boundary in TXS.
#[cfg(feature = "alloc")]
pub(crate) const fn opaque_padding_len(len: usize) -> usize {
    (4 - len % 4) % 4
}

/// The chunks copied into one buffer, for writers without an in-place path.
#[cfg(feature = "alloc")]
pub(crate) fn concat_chunks<B: AsRef<[u8]>>(chunks: &[B]) -> alloc::vec::Vec<u8> {
    let len = chunks.iter().map(|chunk| chunk.as_ref().len()).sum();
    let mut bytes = alloc::vec::Vec::with_capacity(len);
    for chunk in chunks {
        bytes.extend_from_slice(chunk.as_ref());
    }
    bytes
}
