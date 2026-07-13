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
            // SAFETY: the cast is lossless in both cases that reach this branch:
            // - Non-overflow: pos + requested > data.len() implies
            //   data.len() - pos < requested <= u32::MAX.
            // - checked_add overflow (32-bit only): pos <= data.len() <= isize::MAX,
            //   so data.len() - pos <= isize::MAX < u32::MAX.
            let remaining: usize = self.data.len() - self.pos;
            return Err(ReadError::source_empty(requested as usize, remaining));
        };

        // SAFETY: we just verified extent <= self.data.len() and
        // self.pos <= extent (since requested >= 0)
        let buf = unsafe { self.data.get_unchecked(self.pos..extent) };

        self.pos = extent;

        Ok(buf)
    }

    #[inline]
    fn read_byte(&mut self) -> ReadResult<u8> {
        let byte = self.get_input(1)?;

        // SAFETY: get_input(1) returns exactly 1 byte.
        Ok(unsafe { *byte.get_unchecked(0) })
    }

    #[cfg_attr(
        any(
            target_family = "wasm",
            all(target_arch = "riscv32", target_os = "none"),
        ),
        inline(never)
    )]
    #[cfg_attr(
        not(any(
            target_family = "wasm",
            all(target_arch = "riscv32", target_os = "none"),
        )),
        inline(always)
    )]
    fn read_signed(&mut self) -> ReadResult<i64> {
        let h = self.read_byte()?;
        let s = (h & 0xc0) >> 6;
        let v = h & !0xc0;
        let n: i64;

        if v == 63 {
            // The number is encoded in the following bytes

            // SAFETY: get_input(N) returns exactly N initialized bytes
            // on success, so the pointer from as_ptr() is valid for an
            // N-byte read_unaligned.
            n = match s {
                0 => self.read_byte()? as i8 as i64,
                1 => i16::from_le(unsafe {
                    core::ptr::read_unaligned(self.get_input(2)?.as_ptr().cast::<i16>())
                }) as i64,
                2 => i32::from_le(unsafe {
                    core::ptr::read_unaligned(self.get_input(4)?.as_ptr().cast::<i32>())
                }) as i64,
                3 => i64::from_le(unsafe {
                    core::ptr::read_unaligned(self.get_input(8)?.as_ptr().cast::<i64>())
                }),
                _ => unreachable!(),
            };

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
                        return Err(ReadError::not_minimal_encoding());
                    }
                } else if n > repr_max {
                    return Err(ReadError::not_minimal_encoding());
                }
            }
        } else {
            // The number is the 6-bit, one's-complement signed integer in `v`

            if s != 0 {
                return Err(ReadError::invalid_header_byte(h));
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

    #[cfg_attr(
        any(
            target_family = "wasm",
            all(target_arch = "riscv32", target_os = "none"),
        ),
        inline(never)
    )]
    #[cfg_attr(
        not(any(
            target_family = "wasm",
            all(target_arch = "riscv32", target_os = "none"),
        )),
        inline(always)
    )]
    fn read_unsigned(&mut self) -> ReadResult<u64> {
        let h = self.read_byte()?;
        let s = (h & 0xc0) >> 6;
        let v = h & !0xc0;

        if v == 63 {
            // The number is encoded in the following bytes

            // SAFETY: get_input(N) returns exactly N initialized bytes
            // on success, so the pointer from as_ptr() is valid for an
            // N-byte read_unaligned.
            let uint = match s {
                0 => self.read_byte()? as u64,
                1 => u16::from_le(unsafe {
                    core::ptr::read_unaligned(self.get_input(2)?.as_ptr().cast::<u16>())
                }) as u64,
                2 => u32::from_le(unsafe {
                    core::ptr::read_unaligned(self.get_input(4)?.as_ptr().cast::<u32>())
                }) as u64,
                3 => u64::from_le(unsafe {
                    core::ptr::read_unaligned(self.get_input(8)?.as_ptr().cast::<u64>())
                }),
                _ => unreachable!(),
            };

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
                    return Err(ReadError::not_minimal_encoding());
                }
            }

            Ok(uint)
        } else {
            // The number is the 6-bit unsigned integer in `v`

            if s != 0 {
                return Err(ReadError::invalid_header_byte(h));
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
            let n = n
                .try_into()
                .map_err(|_| ReadError::integer_out_of_range())?;
            Ok(n)
        }

    )* };
}

macro_rules! read_unsigned {
    { $( [$f:ident, $ty:ty] ),* $(,)? } => { $(

        #[inline]
        fn $f(&mut self) -> ReadResult<$ty> {
            let n = self.read_unsigned()?;
            let n = n
                .try_into()
                .map_err(|_| ReadError::integer_out_of_range())?;
            Ok(n)
        }

    )* };
}

// SAFETY: read_opaque_fixed returns exactly `len` bytes (or an error).
unsafe impl<'r> Reader<'r> for BufReader<'r> {
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

    /// Reads a 32-bit IEEE 754 float from raw little-endian bytes.
    ///
    /// CXS encodes floats as fixed-size raw bytes (not variable-width
    /// compressed). Little-endian byte order follows CXS convention
    /// (not RFC 4506 big-endian).
    #[inline]
    fn read_f32(&mut self) -> ReadResult<f32> {
        let input = self.read_opaque_fixed(4)?;
        let mut bytes = [0u8; 4];
        // SAFETY: read_opaque_fixed(4) returns exactly 4 bytes.
        unsafe { core::ptr::copy_nonoverlapping(input.as_ptr(), bytes.as_mut_ptr(), 4) };
        Ok(f32::from_le_bytes(bytes))
    }

    /// Reads a 64-bit IEEE 754 double from raw little-endian bytes.
    ///
    /// CXS encodes floats as fixed-size raw bytes (not variable-width
    /// compressed). Little-endian byte order follows CXS convention
    /// (not RFC 4506 big-endian).
    #[inline]
    fn read_f64(&mut self) -> ReadResult<f64> {
        let input = self.read_opaque_fixed(8)?;
        let mut bytes = [0u8; 8];
        // SAFETY: read_opaque_fixed(8) returns exactly 8 bytes.
        unsafe { core::ptr::copy_nonoverlapping(input.as_ptr(), bytes.as_mut_ptr(), 8) };
        Ok(f64::from_le_bytes(bytes))
    }

    #[inline]
    fn read_opaque_fixed(&mut self, len: usize) -> ReadResult<&'r [u8]> {
        // SAFETY precondition: len must fit in u32 (all callers originate
        // from u32 values: read_u32, ValidU32, or small literals).
        debug_assert!(len <= u32::MAX as usize);
        self.get_input(len as u32)
    }

    #[inline(always)]
    fn read_opaque_variable(&mut self, max: u32) -> ReadResult<&'r [u8]> {
        let len = self.read_u32()?;

        if len > max {
            return Err(ReadError::max_length_exceeded());
        }

        self.read_opaque_fixed(len as usize)
    }
}
