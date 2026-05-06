//! Provide a generic CXS implementation for any type
//! that implements some basic byte operations

#![allow(clippy::upper_case_acronyms)]

use super::limits::*;
use crate::serialize::writer::*;

const V_MULTIBYTE: u8 = 63;
const S_8BYTE: u8 = 3 << 6;
const S_4BYTE: u8 = 2 << 6;
const S_2BYTE: u8 = 1 << 6;
const S_1BYTE: u8 = 0 << 6;
const S_HEADER: u8 = 0 << 6;

/// Implement these basic byte operations in order
/// to get a `Writer<CXS>` implementation for free
pub(crate) trait Put {
    // Required:
    fn put_byte(&mut self, b: u8) -> WriteResult<()>;
    fn put_bytes(&mut self, bytes: &[u8]) -> WriteResult<()>;
}

pub struct CXS;

impl<P> Writer<CXS> for P
where
    P: Put,
{
    #[inline]
    fn write_opaque_fixed(&mut self, bytes: &[u8]) -> WriteResult<()> {
        debug_assert!(bytes.len() <= u32::MAX as usize);
        self.put_bytes(bytes)
    }

    // inline here slows down benchmarks
    fn write_opaque_variable(&mut self, bytes: &[u8]) -> WriteResult<()> {
        if bytes.len() > u32::MAX as usize {
            return Err(WriteError::InvalidArrayLength);
        }

        self.write_u32(bytes.len() as u32)?;
        self.put_bytes(bytes)
    }

    #[inline]
    fn write_bool(&mut self, v: bool) -> WriteResult<()> {
        self.write_i32(if v { 1 } else { 0 })
    }

    #[inline]
    fn write_i64(&mut self, n: i64) -> WriteResult<()> {
        if n > i32::MAX as i64 || n < i32::MIN as i64 {
            // Use the 8-byte representation
            let h = V_MULTIBYTE | S_8BYTE;
            let b = n.to_le_bytes();
            let combined = [h, b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]];
            self.put_bytes(&combined)
        } else {
            self.write_i32(n as i32)
        }
    }

    #[inline]
    fn write_i32(&mut self, n: i32) -> WriteResult<()> {
        if n > i16::MAX as i32 || n < i16::MIN as i32 {
            // use 4-byte repr
            let h = V_MULTIBYTE | S_4BYTE;
            let b = n.to_le_bytes();
            let combined = [h, b[0], b[1], b[2], b[3]];
            self.put_bytes(&combined)
        } else {
            self.write_i16(n as i16)
        }
    }

    #[inline]
    fn write_i16(&mut self, n: i16) -> WriteResult<()> {
        if n > i8::MAX as i16 || n < i8::MIN as i16 {
            // use 2-byte repr
            let h = V_MULTIBYTE | S_2BYTE;
            let b = n.to_le_bytes();
            let combined = [h, b[0], b[1]];
            self.put_bytes(&combined)
        } else {
            write_i8(self, n as i8)
        }
    }

    #[inline]
    fn write_u64(&mut self, n: u64) -> WriteResult<()> {
        if n > u32::MAX as u64 {
            // Use 8-byte representation
            let h = V_MULTIBYTE | S_8BYTE;
            let b = n.to_le_bytes();
            let combined = [h, b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]];
            self.put_bytes(&combined)
        } else {
            self.write_u32(n as u32)
        }
    }

    #[inline]
    fn write_u32(&mut self, n: u32) -> WriteResult<()> {
        if n > u16::MAX as u32 {
            // Use 4-byte representation
            let h = V_MULTIBYTE | S_4BYTE;
            let b = n.to_le_bytes();
            let combined = [h, b[0], b[1], b[2], b[3]];
            self.put_bytes(&combined)
        } else {
            self.write_u16(n as u16)
        }
    }

    #[inline]
    fn write_u16(&mut self, n: u16) -> WriteResult<()> {
        if n > u8::MAX as u16 {
            // Use 2-byte representation
            let h = V_MULTIBYTE | S_2BYTE;
            let b = n.to_le_bytes();
            let combined = [h, b[0], b[1]];
            self.put_bytes(&combined)
        } else {
            write_u8(self, n as u8)
        }
    }
}

#[inline]
fn write_u8<P>(p: &mut P, n: u8) -> WriteResult<()>
where
    P: Put,
{
    if n > UH_MAX {
        // Use 1-additional-byte representation
        let h = V_MULTIBYTE | S_1BYTE;
        let combined = [h, n];
        p.put_bytes(&combined)
    } else {
        // Use header-byte-only representation
        p.put_byte(n)
    }
}

#[inline]
fn write_i8<P>(p: &mut P, n: i8) -> WriteResult<()>
where
    P: Put,
{
    if !(IH_MIN..=IH_MAX).contains(&n) {
        // Use the 1-byte repr
        let h = V_MULTIBYTE | S_1BYTE;
        let combined = [h, n as u8];
        p.put_bytes(&combined)
    } else {
        // Represent this small, signed number in the header-byte with 6-bit,
        // one's-complement encoding
        let nu: u8 = n.unsigned_abs();
        let v = if n >= 0 {
            nu
        } else {
            // invert and set the high bit
            (!nu & 0b01_1111) | 0b0010_0000
        };

        let s = S_HEADER;
        let h = v | s;
        p.put_byte(h)
    }
}
