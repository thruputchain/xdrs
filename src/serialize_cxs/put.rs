//! Provide a generic CXS implementation for any type
//! that implements some basic byte operations

#![allow(clippy::upper_case_acronyms)]

use crate::serialize::writer::Result;
use crate::serialize::writer::*;

const V_MULTIBYTE: u8 = 63;
const S_8BYTE: u8 = 3 << 6;
const S_4BYTE: u8 = 2 << 6;
const S_2BYTE: u8 = 1 << 6;
const S_1BYTE: u8 = 0 << 6;
const S_HEADER: u8 = 0 << 6;
const U6_MAX: u8 = 62;
const I6_MAX: i8 = 31;
const I6_MIN: i8 = -31;

/// Implement these basic byte operations in order
/// to get a `Writer<CXS>` implementation for free
pub(crate) trait Put {
    // Required:
    fn put_byte(&mut self, b: u8) -> Result<()>;
    fn put_bytes(&mut self, bytes: &[u8]) -> Result<()>;
}

pub struct CXS;

impl<P> Writer<CXS> for P
where
    P: Put,
{
    fn write_opaque_fixed(&mut self, bytes: &[u8]) -> Result<()> {
        self.put_bytes(bytes)
    }

    fn write_opaque_variable(&mut self, bytes: &[u8]) -> Result<()> {
        if bytes.len() > u32::MAX as usize {
            return Err(Error::InvalidArrayLength);
        }

        self.write_u32(bytes.len() as u32)?;
        self.put_bytes(bytes)
    }

    fn write_bool(&mut self, v: bool) -> Result<()> {
        self.write_i32(if v { 1 } else { 0 })
    }

    fn write_i64(&mut self, n: i64) -> Result<()> {
        if n > i32::MAX as i64 || n < i32::MIN as i64 {
            // Use the 8-byte representation
            let s = S_8BYTE;
            let v = V_MULTIBYTE;
            let h = v | s;
            self.put_byte(h)?;

            let buf = n.to_le_bytes();
            self.put_bytes(&buf)
        } else {
            self.write_i32(n as i32)
        }
    }

    fn write_i32(&mut self, n: i32) -> Result<()> {
        if n > i16::MAX as i32 || n < i16::MIN as i32 {
            // use 4-byte repr
            let s = S_4BYTE;
            let v = V_MULTIBYTE;
            let h = v | s;
            self.put_byte(h)?;

            let buf = n.to_le_bytes();
            self.put_bytes(&buf)
        } else {
            self.write_i16(n as i16)
        }
    }

    fn write_i16(&mut self, n: i16) -> Result<()> {
        if n > i8::MAX as i16 || n < i8::MIN as i16 {
            // use 2-byte repr
            let s = S_2BYTE;
            let v = V_MULTIBYTE;
            let h = v | s;
            self.put_byte(h)?;

            let buf = n.to_le_bytes();
            self.put_bytes(&buf)
        } else {
            self.write_i8(n as i8)
        }
    }

    fn write_i8(&mut self, n: i8) -> Result<()> {
        if !(I6_MIN..=I6_MAX).contains(&n) {
            // Use the 1-byte repr
            let s = S_1BYTE;
            let v = V_MULTIBYTE;
            let h = v | s;
            self.put_byte(h)?;

            self.put_byte(n as u8)
        } else {
            // This small, signed number will be represented in the header-byte with 6-bit,
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
            self.put_byte(h)
        }
    }

    fn write_u64(&mut self, n: u64) -> Result<()> {
        if n > u32::MAX as u64 {
            // Use 8-byte representation
            let s = S_8BYTE;
            let v = V_MULTIBYTE;
            let h = v | s;
            self.put_byte(h)?;

            let buf = n.to_le_bytes();
            self.put_bytes(&buf)
        } else {
            self.write_u32(n as u32)
        }
    }

    fn write_u32(&mut self, n: u32) -> Result<()> {
        if n > u16::MAX as u32 {
            // Use 4-byte representation
            let s = S_4BYTE;
            let v = V_MULTIBYTE;
            let h = v | s;
            self.put_byte(h)?;

            let buf = n.to_le_bytes();
            self.put_bytes(&buf)
        } else {
            self.write_u16(n as u16)
        }
    }

    fn write_u16(&mut self, n: u16) -> Result<()> {
        if n > u8::MAX as u16 {
            // Use 2-byte representation
            let s = S_2BYTE;
            let v = V_MULTIBYTE;
            let h = v | s;
            self.put_byte(h)?;

            let buf = n.to_le_bytes();
            self.put_bytes(&buf)
        } else {
            self.write_u8(n as u8)
        }
    }

    fn write_u8(&mut self, n: u8) -> Result<()> {
        if n > U6_MAX {
            // Use 1-byte representation
            let s = S_1BYTE;
            let v = V_MULTIBYTE;
            let h = v | s;
            self.put_byte(h)?;

            let buf = n.to_le_bytes();
            self.put_bytes(&buf)
        } else {
            // This can be represented in the header byte
            self.put_byte(n)
        }
    }
}
