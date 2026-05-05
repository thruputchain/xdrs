//! TXS (Traditional XDR Serialization) implementation.
//!
//! TXS is the standard [XDR][RFC-4506] serialization format as defined in [RFC 4506][RFC-4506].
//! This module provides implementations of [`Reader`](crate::serialize::reader::Reader) and
//! [`Writer`](crate::serialize::writer::Writer) for the traditional XDR format.
//!
//! # TXS Format Details
//!
//! TXS follows the standard XDR encoding rules:
//!
//! - **Integers**: All integer types (signed and unsigned, 16-bit, 32-bit, and 64-bit)
//!   are encoded in big-endian format using their full size (2, 4, or 8 bytes).
//!
//! - **Booleans**: Encoded as 32-bit signed integers (`0` for `false`, `1` for `true`).
//!
//! - **Opaque Data**: Fixed-length opaque data is padded to 4-byte boundaries.
//!   Variable-length opaque data is prefixed with a 32-bit unsigned integer length,
//!   followed by the data padded to 4-byte boundaries. By default, decoders
//!   require padding bytes to be zero; with feature `permissive`, non-zero
//!   padding is accepted. The same applies to the upper two bytes of a 4-byte
//!   wire integer decoded as `i16` or `u16`: they are treated as padding and
//!   must be zero by default, but are allowed to be non-zero in permissive mode
//!   (the value is truncated to 16 bits).
//!
//! - **Arrays**: Variable-length arrays are prefixed with a 32-bit unsigned integer
//!   length, followed by the array elements.
//!
//! - **Unions**: Discriminated unions use a 32-bit signed integer discriminant,
//!   followed by the union data.
//!
//! This format is compatible with the standard XDR specification and can interoperate
//! with other XDR implementations.
//!
//! For a more compact alternative, see the [`cxs`](crate::cxs) module.
//!
//! [RFC-4506]: https://datatracker.ietf.org/doc/html/rfc4506

mod utils;
pub use utils::*;

pub mod buf_reader;
pub mod into_buf_writer;

#[cfg(feature = "alloc")]
pub mod buf_writer;

mod put;

pub mod const_writer;
pub use const_writer::ConstWriter;
