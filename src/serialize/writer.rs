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
