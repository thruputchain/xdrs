//! TXS buffer reader implementation.
//!
//! This module provides [`BufReader`], which implements [`Reader`]
//! for reading TXS-encoded XDR data from a byte slice.

use crate::serialize::reader::*;

/// A reader for TXS-encoded XDR data from a byte slice.
///
/// This reader implements the [`Reader`] trait for the TXS (Traditional XDR
/// Serialization) format. It reads data from a byte slice and tracks the
/// current position.
pub struct BufReader<'r> {
    data: &'r [u8],
    pos: usize,
}

macro_rules! read_int {
    [ $($read:ident($nbytes:expr): $ty:ty),* $(,)? ] => { $(
        #[inline]
        fn $read(&mut self) -> ReadResult<$ty> {
            let input = self.get_input($nbytes)?;
            let mut bytes = [0u8; $nbytes];
            // SAFETY: get_input($nbytes) returns exactly $nbytes bytes.
            unsafe { core::ptr::copy_nonoverlapping(input.as_ptr(), bytes.as_mut_ptr(), $nbytes) };
            let n = <$ty>::from_be_bytes(bytes);
            Ok(n)
        }
    )* };
}

impl<'a> BufReader<'a> {
    /// Creates a new `BufReader` that reads from the given byte slice.
    ///
    /// The reader starts at position 0.
    pub fn new(data: &'a [u8]) -> Self {
        let pos = 0;
        Self { data, pos }
    }

    /// Returns the current read position in the buffer.
    pub fn position(&self) -> usize {
        self.pos
    }

    /// Returns the number of bytes remaining to be read.
    pub fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    /// Returns a slice of the remaining unread bytes.
    pub fn remaining_slice(&self) -> &'a [u8] {
        &self.data[self.pos..]
    }

    #[inline]
    fn get_input(&mut self, requested: usize) -> ReadResult<&'a [u8]> {
        let Some(extent) = self
            .pos
            .checked_add(requested)
            .filter(|&e| e <= self.data.len())
        else {
            // requested <= u32::MAX, so if pos + requested > data.len()
            // then data.len() - pos <= u32::MAX
            // SAFETY: the cast is lossless in both cases that reach this branch:
            // - Non-overflow: pos + requested > data.len() implies
            //   data.len() - pos < requested <= u32::MAX.
            // - checked_add overflow (32-bit only): pos <= data.len() <= isize::MAX,
            //   so data.len() - pos <= isize::MAX < u32::MAX.
            let remaining: usize = self.data.len() - self.pos;
            return Err(ReadError::SourceEmpty {
                requested,
                remaining,
            });
        };

        // SAFETY: we just verified extent <= self.data.len() and
        // self.pos <= extent (since requested >= 0)
        let buf = unsafe { self.data.get_unchecked(self.pos..extent) };

        self.pos = extent;

        Ok(buf)
    }
}

// SAFETY: read_opaque_fixed returns exactly `len` bytes (or an error).
unsafe impl<'r> Reader<'r> for BufReader<'r> {
    #[inline]
    fn read_opaque_fixed(&mut self, len: usize) -> ReadResult<&'r [u8]> {
        let rem = len % 4;
        if rem == 0 {
            return self.get_input(len);
        }

        let padlen = 4 - rem;

        // SAFETY: padlen <= 3 and get_input(len + padlen) returns exactly
        // len + padlen initialized bytes. The first `len` bytes are
        // payload, the trailing `padlen` bytes are padding.
        let combined = len.checked_add(padlen).ok_or(ReadError::SourceEmpty {
            // TODO(geoff): The actual requested amount if this overflow fails
            // is 2^32, i.e. one more than u32::MAX, on 32-bit targets
            // This check overflows for i.e. len = u32::MAX, padding=1 on 32-bit targets,
            // but on such targets we cannot have a buffer of size usize::MAX anyways.
            requested: usize::MAX,
            remaining: self.data.len() - self.pos,
        })?;
        let buf = self.get_input(combined)?;
        let payload = unsafe { buf.get_unchecked(..len as usize) };

        #[cfg(not(feature = "permissive"))]
        if unsafe { buf.get_unchecked(len as usize..) }
            .iter()
            .any(|&b| b != 0)
        {
            return Err(ReadError::NonZeroPadding);
        }

        Ok(payload)
    }

    #[inline]
    fn read_opaque_variable(&mut self, max: u32) -> ReadResult<&'r [u8]> {
        let len = self.read_u32()?;

        if len > max {
            return Err(ReadError::MaxLengthExceeded);
        }

        self.read_opaque_fixed(len as usize)
    }

    #[inline]
    fn read_i16(&mut self) -> ReadResult<i16> {
        // i16 is encoded as a 4-byte big-endian i32. The upper two bytes are
        // padding: they must be zero (or 0xFF for sign-extension) by default,
        // but in permissive mode non-zero padding is accepted and the value is
        // truncated to 16 bits.
        let n = self.read_i32()?;
        #[cfg(not(feature = "permissive"))]
        if n < i16::MIN as i32 || n > i16::MAX as i32 {
            return Err(ReadError::IntegerOutOfRange);
        }
        Ok(n as i16)
    }

    #[inline]
    fn read_u16(&mut self) -> ReadResult<u16> {
        // u16 is encoded as a 4-byte big-endian u32. The upper two bytes are
        // padding: they must be zero by default, but in permissive mode
        // non-zero padding is accepted and the value is truncated to 16 bits.
        let n = self.read_u32()?;
        #[cfg(not(feature = "permissive"))]
        if n < u16::MIN as u32 || n > u16::MAX as u32 {
            return Err(ReadError::IntegerOutOfRange);
        }
        Ok(n as u16)
    }

    read_int! [
        read_u64(8): u64,
        read_u32(4): u32,

        read_i64(8): i64,
        read_i32(4): i32,
    ];

    #[inline]
    fn read_f32(&mut self) -> ReadResult<f32> {
        let input = self.get_input(4)?;
        let mut bytes = [0u8; 4];
        // SAFETY: get_input(4) returns exactly 4 bytes.
        unsafe { core::ptr::copy_nonoverlapping(input.as_ptr(), bytes.as_mut_ptr(), 4) };
        Ok(f32::from_be_bytes(bytes))
    }

    #[inline]
    fn read_f64(&mut self) -> ReadResult<f64> {
        let input = self.get_input(8)?;
        let mut bytes = [0u8; 8];
        // SAFETY: get_input(8) returns exactly 8 bytes.
        unsafe { core::ptr::copy_nonoverlapping(input.as_ptr(), bytes.as_mut_ptr(), 8) };
        Ok(f64::from_be_bytes(bytes))
    }
}
