//! TXS writer that writes into a pre-allocated buffer.
//!
//! This module provides [`IntoBufWriter`], which implements [`Writer`] for
//! writing TXS-encoded XDR data into a pre-allocated buffer of
//! `MaybeUninit<u8>`.

use super::put::*;
use crate::serialize::writer::*;
use core::mem::MaybeUninit;

type Mu8 = MaybeUninit<u8>;

/// A writer for TXS-encoded XDR data into a pre-allocated buffer.
///
/// This writer implements the [`Writer`] trait for the TXS (Traditional XDR
/// Serialization) format. It writes data into a provided buffer of
/// `MaybeUninit<u8>`, which is useful for `no_std` environments where
/// allocation is not available.
///
/// The buffer must be large enough to hold all the data being written, or
/// [`WriteError::SpaceExhausted`] will be returned.
///
pub struct IntoBufWriter<'a> {
    buf: &'a mut [Mu8],
    written: usize,
}

impl<'a> IntoBufWriter<'a> {
    /// Creates a new `IntoBufWriter` that writes into the given buffer.
    pub fn new(buf: &'a mut [Mu8]) -> Self {
        Self { buf, written: 0 }
    }

    /// Returns the number of bytes written so far.
    pub fn written(&self) -> usize {
        self.written
    }

    #[inline]
    fn reserve<'s>(&'s mut self, len: usize) -> WriteResult<&'s mut [Mu8]>
    where
        'a: 's,
    {
        let extent = self.written + len;
        if extent > self.buf.len() {
            return Err(WriteError::SpaceExhausted);
        }

        let start = self.written;
        self.written = extent;

        // SAFETY: we just verified extent <= self.buf.len(), and
        // start <= extent (since len >= 0 and start = old written).
        let reserved = unsafe { self.buf.get_unchecked_mut(start..extent) };

        Ok(reserved)
    }
}

impl<'a> Put for IntoBufWriter<'a> {
    #[inline]
    fn put_bytes(&mut self, bytes: &[u8]) -> WriteResult<()> {
        let len = bytes.len();
        let buf = self.reserve(len)?;

        // Cannot panic: reserve returns a slice of exactly `len` bytes,
        // and bytes.len() == len.
        buf.write_copy_of_slice(bytes);

        Ok(())
    }

    #[inline]
    fn put_opaque_variable(&mut self, bytes: &[u8]) -> WriteResult<()> {
        debug_assert!(bytes.len() <= u32::MAX as usize);

        static PAD: [u8; 3] = [0; 3];

        let len = bytes.len();
        let rem = if len.is_multiple_of(4) {
            0
        } else {
            4 - len % 4
        };
        let size = 4 + len + rem;
        debug_assert!(size.is_multiple_of(4));

        let buf = self.reserve(size)?;

        // Cannot panic: all sub-slices partition `buf` (of length
        // `size = 4 + len + rem`) exactly, so each pair of slices has
        // matching lengths.
        buf[0..4].write_copy_of_slice(&(len as u32).to_be_bytes());
        buf[4..4 + len].write_copy_of_slice(bytes);
        buf[4 + len..4 + len + rem].write_copy_of_slice(&PAD[0..rem]);

        Ok(())
    }

    #[inline]
    fn put_opaque_fixed(&mut self, bytes: &[u8]) -> WriteResult<()> {
        static PAD: [u8; 3] = [0; 3];
        debug_assert!(bytes.len() <= u32::MAX as usize);

        let len = bytes.len();
        let rem = if len.is_multiple_of(4) {
            0
        } else {
            4 - len % 4
        };
        let size = len + rem;
        debug_assert!(size.is_multiple_of(4));

        let buf = self.reserve(size)?;

        // Cannot panic: buf[0..len] and bytes both have length `len`;
        // buf[len..size] and PAD[0..rem] both have length `rem`.
        buf[0..len].write_copy_of_slice(bytes);
        buf[len..size].write_copy_of_slice(&PAD[0..rem]);

        Ok(())
    }
}
