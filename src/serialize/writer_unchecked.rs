//! Writing XDR values to a serialization format without runtime checks.
//!
//! This module defines the [`WriterUnchecked`] trait, the infallible, unsafe
//! counterpart to [`Writer`](crate::serialize::writer::Writer). Its methods
//! perform no runtime bounds checking: the caller must guarantee up front
//! that the writer's underlying buffer is large enough for everything
//! written, typically by sizing it with the worst-case bounds computed by
//! `#[derive(ConstWriteXDR)]` (`const_max_cxs_size` / `const_max_txs_size`).
//!
//! Preconditions are checked with `debug_assert!` only, so misuse is caught
//! in debug builds at zero release-mode cost. This is intended for
//! code-size-sensitive targets (e.g. wasm smart contracts) where the
//! `WriteResult` plumbing and per-write bounds checks of
//! [`Writer`](crate::serialize::writer::Writer) are unwanted overhead.

/// Something that can write XDR values to a serialization format, without
/// runtime checks.
///
/// This is the infallible, unsafe counterpart to
/// [`Writer`](crate::serialize::writer::Writer): the same primitive-writing
/// methods, but no method can fail and no method checks that space remains
/// in the underlying buffer.
///
/// # Safety
///
/// Every method shares one contract: the writer's underlying buffer must
/// have enough space remaining for the encoded form of the argument. The
/// opaque methods additionally require `bytes.len() <= u32::MAX`.
/// Violations are `debug_assert!`ed; in release builds they are undefined
/// behavior.
pub trait WriterUnchecked<F> {
    /// Writes a byte sequence of fixed length.
    ///
    /// # Safety
    ///
    /// The underlying buffer must have space for the encoded form, and
    /// `bytes.len()` must not exceed `u32::MAX`.
    #[doc(hidden)]
    unsafe fn write_opaque_fixed(&mut self, bytes: &[u8]);

    /// Writes a byte sequence of variable length.
    ///
    /// # Safety
    ///
    /// The underlying buffer must have space for the encoded form, and
    /// `bytes.len()` must not exceed `u32::MAX`.
    unsafe fn write_opaque_variable(&mut self, bytes: &[u8]);

    /// Writes a signed 64-bit integer.
    ///
    /// # Safety
    ///
    /// The underlying buffer must have space for the encoded form.
    unsafe fn write_i64(&mut self, n: i64);

    /// Writes a signed 32-bit integer.
    ///
    /// # Safety
    ///
    /// The underlying buffer must have space for the encoded form.
    unsafe fn write_i32(&mut self, n: i32);

    /// Writes a signed 16-bit integer.
    ///
    /// # Safety
    ///
    /// The underlying buffer must have space for the encoded form.
    unsafe fn write_i16(&mut self, n: i16);

    /// Writes an unsigned 64-bit integer.
    ///
    /// # Safety
    ///
    /// The underlying buffer must have space for the encoded form.
    unsafe fn write_u64(&mut self, n: u64);

    /// Writes an unsigned 32-bit integer.
    ///
    /// # Safety
    ///
    /// The underlying buffer must have space for the encoded form.
    unsafe fn write_u32(&mut self, n: u32);

    /// Writes an unsigned 16-bit integer.
    ///
    /// # Safety
    ///
    /// The underlying buffer must have space for the encoded form.
    unsafe fn write_u16(&mut self, n: u16);

    /// Writes a boolean value.
    ///
    /// # Safety
    ///
    /// The underlying buffer must have space for the encoded form.
    unsafe fn write_bool(&mut self, b: bool);

    /// Writes a 32-bit IEEE 754 floating-point value.
    ///
    /// # Safety
    ///
    /// The underlying buffer must have space for the encoded form.
    unsafe fn write_f32(&mut self, n: f32);

    /// Writes a 64-bit IEEE 754 floating-point value.
    ///
    /// # Safety
    ///
    /// The underlying buffer must have space for the encoded form.
    unsafe fn write_f64(&mut self, n: f64);
}
