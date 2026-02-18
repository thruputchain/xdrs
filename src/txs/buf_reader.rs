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
        fn $read(&mut self) -> ReadResult<$ty> {
            let bytes: [u8; $nbytes] = self.get_input($nbytes)?.try_into().unwrap();
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

    fn get_input(&mut self, requested: u32) -> ReadResult<&'a [u8]> {
        let extent = self.pos + requested as usize;
        if extent > self.data.len() {
            // no panic because remaining < requested <= u32::MAX
            let remaining = (self.data.len() - self.pos).try_into().unwrap();
            return Err(ReadError::SourceEmpty {
                requested,
                remaining,
            });
        }

        let buf = &self.data[self.pos..extent];

        self.pos = extent;

        Ok(buf)
    }

    /// Skips the specified number of bytes in the input.
    ///
    /// This is used internally for padding. The number of bytes to skip
    /// must be at most 3 (for 4-byte alignment padding).
    pub fn skip_input(&mut self, skip_bytes: u32) -> ReadResult<()> {
        debug_assert!(skip_bytes <= 3);

        let _ = self.get_input(skip_bytes)?;
        Ok(())
    }
}

impl<'r> Reader<'r> for BufReader<'r> {
    fn read_opaque_fixed(&mut self, len: u32) -> ReadResult<&'r [u8]> {
        let buf = self.get_input(len)?;

        let rem = len % 4;
        if rem > 0 {
            let padlen = 4 - rem;
            self.skip_input(padlen)?;
        }

        Ok(buf)
    }

    fn read_opaque_variable(&mut self, max: u32) -> ReadResult<&'r [u8]> {
        let len = self.read_u32()?;

        if len > max {
            return Err(ReadError::MaxLengthExceeded);
        }

        self.read_opaque_fixed(len)
    }

    read_int! [
        read_u64(8): u64,
        read_u32(4): u32,
        read_u16(2): u16,

        read_i64(8): i64,
        read_i32(4): i32,
        read_i16(2): i16,
    ];
}
