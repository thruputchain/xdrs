//! TXS writer that writes into a pre-allocated buffer.
//!
//! This module provides [`IntoBufWriter`], which implements [`Writer`] for
//! writing TXS-encoded XDR data into a pre-allocated buffer of
//! `MaybeUninit<u8>`.

use super::put::*;
use crate::mu;
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
/// [`Error::SpaceExhausted`] will be returned.
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

    fn reserve<'s>(&'s mut self, len: usize) -> Result<&'s mut [Mu8]>
    where
        'a: 's,
    {
        let extent = self.written + len;
        if extent > self.buf.len() {
            return Err(Error::SpaceExhausted);
        }

        let start = self.written;
        self.written += len;

        let reserved = &mut self.buf[start..extent];

        Ok(reserved)
    }
}

impl<'a> Put for IntoBufWriter<'a> {
    fn put_bytes(&mut self, bytes: &[u8]) -> Result<()> {
        let len = bytes.len();
        let buf = self.reserve(len)?;

        mu::write_copy_of_slice(buf, bytes);

        Ok(())
    }
}
