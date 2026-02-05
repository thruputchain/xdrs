//! Writing XDR values to a serialization format.
//!
//! This module defines the [`Writer`] trait, which provides methods for
//! writing [XDR][RFC-4506] primitive types (as defined in RFC 4506)
//! to some underlying serialization format.
//! Implementations of this trait can be used with [`WriteXDR`](crate::WriteXDR)
//! to serialize complex types.
//!
//! [RFC-4506]: https://datatracker.ietf.org/doc/html/rfc4506

use thiserror::Error;

/// Errors that can occur when writing XDR data.
#[derive(Copy, Clone, Debug, Error)]
pub enum WriteError {
    /// The writer has run out of space.
    #[error("space exhausted")]
    SpaceExhausted,

    /// An array length is too large to be represented in XDR.
    #[error("invalid array length (too large)")]
    InvalidArrayLength,
}

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
    /// Writes a byte array of fixed length.
    fn write_opaque_fixed(&mut self, bytes: &[u8]) -> Result<(), WriteError>;

    /// Writes a byte sequence of variable length.
    fn write_opaque_variable(&mut self, bytes: &[u8]) -> Result<(), WriteError>;

    /// Writes a signed 64-bit integer.
    fn write_i64(&mut self, n: i64) -> Result<(), WriteError>;

    /// Writes a signed 32-bit integer.
    fn write_i32(&mut self, n: i32) -> Result<(), WriteError>;

    /// Writes a signed 16-bit integer.
    fn write_i16(&mut self, n: i16) -> Result<(), WriteError>;

    /// Writes an unsigned 64-bit integer.
    fn write_u64(&mut self, n: u64) -> Result<(), WriteError>;

    /// Writes an unsigned 32-bit integer.
    fn write_u32(&mut self, n: u32) -> Result<(), WriteError>;

    /// Writes an unsigned 16-bit integer.
    fn write_u16(&mut self, n: u16) -> Result<(), WriteError>;

    /// Writes a boolean value.
    fn write_bool(&mut self, b: bool) -> Result<(), WriteError>;
}
