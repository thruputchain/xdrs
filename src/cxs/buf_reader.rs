//! CXS buffer reader implementation.
//!
//! This module provides [`BufReader`], which implements [`Reader`] for reading
//! CXS-encoded XDR data from a byte slice.

#[cfg(not(feature = "permissive"))]
use super::limits::*;

use crate::serialize::reader::*;

/// Read CXS-encoded XDR data from a byte slice.
///
/// This reader implements the [`Reader`] trait for the CXS (Compact XDR
/// Serialization) format. It reads data from a byte slice and tracks the
/// current position.
pub struct BufReader<'r> {
    data: &'r [u8],
    pos: usize,
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
    fn get_input(&mut self, requested: u32) -> ReadResult<&'a [u8]> {
        let Some(extent) = self
            .pos
            .checked_add(requested as usize)
            .filter(|&e| e <= self.data.len())
        else {
            // requested <= u32::MAX, so if pos + requested > data.len()
            // then data.len() - pos <= u32::MAX
            let remaining: u32 = (self.data.len() - self.pos).try_into().unwrap();
            return Err(ReadError::SourceEmpty {
                requested,
                remaining,
            });
        };

        let buf = &self.data[self.pos..extent];

        self.pos = extent;

        Ok(buf)
    }

    #[inline]
    fn read_byte(&mut self) -> ReadResult<u8> {
        let byte = self.get_input(1)?;

        Ok(byte[0])
    }

    fn read_signed(&mut self) -> ReadResult<i64> {
        let h = self.read_byte()?;
        let s = (h & 0xc0) >> 6;
        let v = h & !0xc0;
        let n: i64;

        if v == 63 {
            // The number is encoded in the following bytes

            const N_BYTES_MAP: [usize; 4] = [1, 2, 4, 8];
            let n_bytes = N_BYTES_MAP[s as usize];

            let input = self.read_opaque_fixed(n_bytes as u32)?;

            let mut int_bytes = [0u8; 8];

            let int_dest = &mut int_bytes[0..n_bytes];
            int_dest.copy_from_slice(input);

            if input[n_bytes - 1] & 0x80 != 0 {
                // the high bit is set, so sign-extend remaining bytes
                for b in int_bytes.iter_mut().skip(n_bytes) {
                    *b = 0xff;
                }
            }

            n = i64::from_le_bytes(int_bytes);

            #[cfg(not(feature = "permissive"))]
            {
                // validate that the smallest possible encoding was used

                const N_BYTE_REPR_MINMAX: [(i64, i64); 4] = [
                    (IH_MAX as i64 + 1, IH_MIN as i64 - 1),
                    (i8::MAX as i64 + 1, i8::MIN as i64 - 1),
                    (i16::MAX as i64 + 1, i16::MIN as i64 - 1),
                    (i32::MAX as i64 + 1, i32::MIN as i64 - 1),
                ];
                let (repr_min, repr_max) = N_BYTE_REPR_MINMAX[s as usize];

                if n > 0 {
                    if n < repr_min {
                        return Err(ReadError::NotMinimalEncoding);
                    }
                } else if n > repr_max {
                    return Err(ReadError::NotMinimalEncoding);
                }
            }
        } else {
            // The number is the 6-bit, one's-complement signed integer in `v`

            if s != 0 {
                return Err(ReadError::InvalidHeaderByte { h });
            }

            if v & 0b10_0000 != 0 {
                // The sign-bit is set, so obtain the number by inverting and
                // negating:
                n = -(((!v) & 0b11_1111) as i64);
            } else {
                n = v as i64;
            }
        }

        Ok(n)
    }

    fn read_unsigned(&mut self) -> ReadResult<u64> {
        let h = self.read_byte()?;
        let s = (h & 0xc0) >> 6;
        let v = h & !0xc0;

        if v == 63 {
            // The number is encoded in the following bytes

            const N_BYTES_MAP: [usize; 4] = [1, 2, 4, 8];
            let n_bytes = N_BYTES_MAP[s as usize];

            let input = self.read_opaque_fixed(n_bytes as u32)?;

            let mut int_bytes = [0u8; 8];
            let int_dest = &mut int_bytes[0..n_bytes];

            int_dest.copy_from_slice(input);
            let uint = u64::from_le_bytes(int_bytes);

            #[cfg(not(feature = "permissive"))]
            {
                // validate that the smallest possible encoding was used

                const N_BYTE_REPR_MIN: [u64; 4] = [
                    63,
                    u8::MAX as u64 + 1,
                    u16::MAX as u64 + 1,
                    u32::MAX as u64 + 1,
                ];
                let repr_min = N_BYTE_REPR_MIN[s as usize];

                if uint < repr_min {
                    return Err(ReadError::NotMinimalEncoding);
                }
            }

            Ok(uint)
        } else {
            // The number is the 6-bit unsigned integer in `v`

            if s != 0 {
                return Err(ReadError::InvalidHeaderByte { h });
            }

            Ok(v as u64)
        }
    }
}

macro_rules! read_signed {
    { $( [$f:ident, $ty:ty] ),* $(,)? } => { $(

        #[inline]
        fn $f(&mut self) -> ReadResult<$ty> {
            let n = self.read_signed()?;
            let n = n.try_into().map_err(|_| ReadError::IntegerOutOfRange)?;
            Ok(n)
        }

    )* };
}

macro_rules! read_unsigned {
    { $( [$f:ident, $ty:ty] ),* $(,)? } => { $(

        #[inline]
        fn $f(&mut self) -> ReadResult<$ty> {
            let n = self.read_unsigned()?;
            let n = n.try_into().map_err(|_| ReadError::IntegerOutOfRange)?;
            Ok(n)
        }

    )* };
}

impl<'r> Reader<'r> for BufReader<'r> {
    read_signed! {
        [read_i64, i64],
        [read_i32, i32],
        [read_i16, i16],
    }

    read_unsigned! {
        [read_u64, u64],
        [read_u32, u32],
        [read_u16, u16],
    }

    fn read_opaque_fixed(&mut self, len: u32) -> ReadResult<&'r [u8]> {
        self.get_input(len)
    }

    fn read_opaque_variable(&mut self, max: u32) -> ReadResult<&'r [u8]> {
        let len = self.read_u32()?;

        if len > max {
            return Err(ReadError::MaxLengthExceeded);
        }

        self.read_opaque_fixed(len)
    }
}
