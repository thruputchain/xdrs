//! Provide a generic CXS implementation for any type
//! that implements some basic byte operations

use crate::xdr::writer::*;
use crate::xdr::writer::Result;
use crate::write_xdr::*;

const V_MULTIBYTE: u8 = 63;
const S_8BYTE: u8 = 3 << 6;
const S_4BYTE: u8 = 2 << 6;
const S_2BYTE: u8 = 1 << 6;
const S_1BYTE: u8 = 0 << 6;
const S_HEADER: u8 = 0 << 6;
const U6_MAX: u8 = 62;
const I6_MAX: i8 = 31;
const I6_MIN: i8 = -31;

/// A trait that facilitates writing serializers by implementing
/// only two methods.
pub trait Put {
    // Required:

    fn put_byte(&mut self, b: u8) -> Result<()>;

    /// Write a byte array of specified length
    fn put_byte_array(&mut self, bytes: &[u8]) -> Result<()>;

    // Provided:

    fn put_bool(&mut self, v: bool) -> Result<()> {
        self.put_i32(if v { 1 } else { 0 })
    }

    fn put_i64(&mut self, n: i64) -> Result<()> {
        if n > i32::MAX as i64 || n < i32::MIN as i64 {
            // Use the 8-byte representation
            let s = S_8BYTE;
            let v = V_MULTIBYTE;
            let h = v | s;
            self.put_byte(h)?;

            let buf = n.to_le_bytes();
            self.put_byte_array(&buf)
        } else {
            self.put_i32(n as i32)
        }
    }

    fn put_i32(&mut self, n: i32) -> Result<()> {
        if n > i16::MAX as i32 || n < i16::MIN as i32 {
            // use 4-byte repr
            let s = S_4BYTE;
            let v = V_MULTIBYTE;
            let h = v | s;
            self.put_byte(h)?;
            let buf = n.to_le_bytes();
            self.put_byte_array(&buf)
        } else {
            self.put_i16(n as i16)
        }
    }

    fn put_i16(&mut self, n: i16) -> Result<()> {
        if n > i8::MAX as i16 || n < i8::MIN as i16 {
            // use 2-byte repr
            let s = S_2BYTE;
            let v = V_MULTIBYTE;
            let h = v | s;
            self.put_byte(h)?;
            let buf = (n as i16).to_le_bytes();
            self.put_byte_array(&buf)
        } else {
            self.put_i8(n as i8)
        }
    }

    fn put_i8(&mut self, n: i8) -> Result<()> {
        if n > I6_MAX || n < I6_MIN {
            // Use the 1-byte repr
            let s = S_1BYTE;
            let v = V_MULTIBYTE;
            let h = v | s;
            self.put_byte(h)?;
            self.put_byte(n as u8)
        } else {
            // This small, signed number will be represented in the header-byte with 6-bit,
            // one's-complement encoding
            let v;
            let nu: u8 = n.unsigned_abs();
            if n >= 0 {
                v = nu;
            } else {
                // invert and set the high bit
                v = (!nu & 0b01_1111) | 0b0010_0000;
            }

            let s = S_HEADER;
            let h = v | s;
            self.put_byte(h)
        }
    }

    fn put_u64(&mut self, n: u64) -> Result<()> {
        if n > u32::MAX as u64 {
            // Use 8-byte representation
            let s = S_8BYTE;
            let v = V_MULTIBYTE;
            let h = v | s;
            self.put_byte(h)?;

            let buf = n.to_le_bytes();
            self.put_byte_array(&buf)
        } else {
            self.put_u32(n as u32)
        }
    }

    fn put_u32(&mut self, n: u32) -> Result<()> {
        if n > u16::MAX as u32 {
            // Use 4-byte representation
            let s = S_4BYTE;
            let v = V_MULTIBYTE;
            let h = v | s;
            self.put_byte(h)?;

            let buf = n.to_le_bytes();
            self.put_byte_array(&buf)
        } else {
            self.put_u16(n as u16)
        }
    }

    fn put_u16(&mut self, n: u16) -> Result<()> {
        if n > u8::MAX as u16 {
            // Use 2-byte representation
            let s = S_2BYTE;
            let v = V_MULTIBYTE;
            let h = v | s;
            self.put_byte(h)?;

            let buf = n.to_le_bytes();
            self.put_byte_array(&buf)
        } else {
            self.put_u8(n as u8)
        }
    }

    fn put_u8(&mut self, n: u8) -> Result<()> {
        if n > U6_MAX {
            // Use 1-byte representation
            let s = S_1BYTE;
            let v = V_MULTIBYTE;
            let h = v | s;
            self.put_byte(h)?;

            let buf = n.to_le_bytes();
            self.put_byte_array(&buf)
        } else {
            // This can be represented in the header byte
            self.put_byte(n)
        }
    }

    fn put_xdr<T: WriteXDR>(&mut self, value: &T) -> Result<()>
        where Self: Sized
    {
        value.write_xdr(self)
    }
}


impl<'a, P> Writer for &'a mut P
where
    P: Put
{
    type SW = &'a mut P;

    fn write_struct(self) -> Result<Self::SW> {
        Ok(self)
    }

    fn write_byte_array(self, bytes: &[u8]) -> Result<()> {
        self.put_byte_array(bytes)
    }

    fn write_bytes(self, bytes: &[u8]) -> Result<()> {
        if bytes.len() > u32::MAX as usize {
            return Err(Error::InvalidArrayLength);
        }

        self.put_u32(bytes.len() as u32)?;
        self.put_byte_array(bytes)
    }

    fn write_opt<T: WriteXDR>(self, v: &Option<T>) -> Result <()> {
        if let Some(v) = v.as_ref() {
            self.put_bool(true)?;
            self.put_xdr(v)
        } else {
            self.put_bool(false)
        }
    }

    fn write_array<T, const N: usize>(self, array: &[T; N]) -> Result<()>
        where T: WriteXDR
    {
        for elt in array {
            self.put_xdr(elt)?;
        }
        Ok(())
    }

    #[inline]
    fn write_i64(self, n: i64) -> Result<()> {
        self.put_i64(n)
    }
    #[inline]
    fn write_i32(self, n: i32) -> Result<()> {
        self.put_i32(n)
    }
    #[inline]
    fn write_i16(self, n: i16) -> Result<()> {
        self.put_i16(n)
    }
    #[inline]
    fn write_i8(self, n: i8) -> Result<()> {
        self.put_i8(n)
    }
    #[inline]
    fn write_u64(self, n: u64) -> Result<()> {
        self.put_u64(n)
    }
    #[inline]
    fn write_u32(self, n: u32) -> Result<()> {
        self.put_u32(n)
    }
    #[inline]
    fn write_u16(self, n: u16) -> Result<()> {
        self.put_u16(n)
    }
    #[inline]
    fn write_u8(self, n: u8) -> Result<()> {
        self.put_u8(n)
    }
    #[inline]
    fn write_bool(self, b: bool) -> Result<()> {
        self.put_bool(b)
    }
}

impl<'a, P> StructWriter for &'a mut P
where P: Put
{
    fn write_field<T: WriteXDR>(&mut self, value: &T) -> Result<()> {
        value.write_xdr(&mut **self)
    }
}
