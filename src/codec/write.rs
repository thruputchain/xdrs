//! Write XDR values in some format

use thiserror::Error;

#[derive(Copy, Clone, Debug, Error)]
pub enum Error {
    #[error("space exhausted")]
    SpaceExhausted,

    #[error("invalid array length (too large)")]
    InvalidArrayLength,
}

pub type Result<T, E = Error> = core::result::Result<T, E>;

pub type Acc = u32;

const V_MULTIBYTE: u8 = 63;
const S_8BYTE: u8 = 3 << 6;
const S_4BYTE: u8 = 2 << 6;
const S_2BYTE: u8 = 1 << 6;
const S_1BYTE: u8 = 0 << 6;
const S_HEADER: u8 = 0 << 6;
const U6_MAX: u8 = 62;
const I6_MAX: i8 = 31;
const I6_MIN: i8 = -31;

pub trait Write {
    // type Acc;

    fn write_byte(&mut self, b: u8) -> Result<Acc>;

    /// Write a byte array of specified length
    fn write_byte_array(&mut self, bytes: &[u8]) -> Result<Acc>;

    /// Write a byte sequence of unspecified length
    fn write_bytes(&mut self, bytes: &[u8]) -> Result<Acc> {
        if bytes.len() > u32::MAX as usize {
            return Err(Error::InvalidArrayLength);
        }

        self.write_u32(bytes.len() as u32)?;
        self.write_byte_array(bytes)
    }

    fn write_i64(&mut self, n: i64) -> Result<Acc> {
        if n > i32::MAX as i64 || n < i32::MIN as i64 {
            // Use the 8-byte representation
            let s = S_8BYTE;
            let v = V_MULTIBYTE;
            let h = v | s;
            self.write_byte(h)?;

            let buf = n.to_le_bytes();
            self.write_byte_array(&buf)
        } else {
            self.write_i32(n as i32)
        }
    }

    fn write_i32(&mut self, n: i32) -> Result<Acc> {
        if n > i16::MAX as i32 || n < i16::MIN as i32 {
            // use 4-byte repr
            let s = S_4BYTE;
            let v = V_MULTIBYTE;
            let h = v | s;
            self.write_byte(h)?;
            let buf = n.to_le_bytes();
            self.write_byte_array(&buf)
        } else {
            self.write_i16(n as i16)
        }
    }

    fn write_i16(&mut self, n: i16) -> Result<Acc> {
        if n > i8::MAX as i16 || n < i8::MIN as i16 {
            // use 2-byte repr
            let s = S_2BYTE;
            let v = V_MULTIBYTE;
            let h = v | s;
            self.write_byte(h)?;
            let buf = (n as i16).to_le_bytes();
            self.write_byte_array(&buf)
        } else {
            self.write_i8(n as i8)
        }
    }

    fn write_i8(&mut self, n: i8) -> Result<Acc> {
        if n > I6_MAX || n < I6_MIN {
            // Use the 1-byte repr
            let s = S_1BYTE;
            let v = V_MULTIBYTE;
            let h = v | s;
            self.write_byte(h)?;
            self.write_byte(n as u8)
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
            self.write_byte(h)
        }
    }

    fn write_u64(&mut self, n: u64) -> Result<Acc> {
        if n > u32::MAX as u64 {
            // Use 8-byte representation
            let s = S_8BYTE;
            let v = V_MULTIBYTE;
            let h = v | s;
            self.write_byte(h)?;

            let buf = n.to_le_bytes();
            self.write_byte_array(&buf)
        } else {
            self.write_u32(n as u32)
        }
    }

    fn write_u32(&mut self, n: u32) -> Result<Acc> {
        if n > u16::MAX as u32 {
            // Use 4-byte representation
            let s = S_4BYTE;
            let v = V_MULTIBYTE;
            let h = v | s;
            self.write_byte(h)?;

            let buf = n.to_le_bytes();
            self.write_byte_array(&buf)
        } else {
            self.write_u16(n as u16)
        }
    }

    fn write_u16(&mut self, n: u16) -> Result<Acc> {
        if n > u8::MAX as u16 {
            // Use 2-byte representation
            let s = S_2BYTE;
            let v = V_MULTIBYTE;
            let h = v | s;
            self.write_byte(h)?;

            let buf = n.to_le_bytes();
            self.write_byte_array(&buf)
        } else {
            self.write_u8(n as u8)
        }
    }

    fn write_u8(&mut self, n: u8) -> Result<Acc> {
        if n > U6_MAX {
            // Use 1-byte representation
            let s = S_1BYTE;
            let v = V_MULTIBYTE;
            let h = v | s;
            self.write_byte(h)?;

            let buf = n.to_le_bytes();
            self.write_byte_array(&buf)
        } else {
            // This can be represented in the header byte
            self.write_byte(n)
        }
    }
}

// ------------ CXS format implementations ---------------------------------------

#[cfg(feature = "alloc")]
pub use buf_writer::*;

#[cfg(feature = "alloc")]
mod buf_writer {
    use super::*;
    use alloc::{vec, vec::Vec};

    pub struct BufWriter(Vec<u8>);

    impl BufWriter {
        pub fn new() -> Self {
            Self(vec![])
        }
    }

    impl From<Vec<u8>> for BufWriter {
        fn from(vec: Vec<u8>) -> Self {
            Self(vec)
        }
    }

    impl Write for BufWriter {
        // type Acc = u32;

        fn write_byte(&mut self, b: u8) -> Result<Acc> {
            self.0.push(b);
            Ok(1)
        }

        fn write_byte_array(&mut self, bytes: &[u8]) -> Result<Acc> {
            if bytes.len() > u32::MAX as usize {
                return Err(Error::InvalidArrayLength);
            }

            self.0.extend_from_slice(bytes);

            Ok(bytes.len() as u32)
        }
    }

    impl AsRef<[u8]> for BufWriter {
        fn as_ref(&self) -> &[u8] {
            self.0.as_ref()
        }
    }

    impl Into<Vec<u8>> for BufWriter {
        fn into(self) -> Vec<u8> {
            self.0
        }
    }
}

// ---------------------------------------------------------

pub use into_buf_writer::*;

mod into_buf_writer {
    use super::*;

    use core::mem::MaybeUninit;
    type Mu8 = MaybeUninit<u8>;

    pub struct IntoBufWriter<'a> {
        buf: &'a mut [Mu8],
        written: usize,
    }

    impl<'a> IntoBufWriter<'a> {
        pub fn new(buf: &'a mut [Mu8]) -> Self {
            Self { buf, written: 0 }
        }

        pub fn written(&self) -> usize {
            self.written
        }

        fn reserve<'s>(&'s mut self, len: usize) -> Result<&'s mut [Mu8]>
        where
            'a: 's
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

    impl<'a> Write for IntoBufWriter<'a> {
        fn write_byte(&mut self, b: u8) -> Result<Acc> {
            let p = self.reserve(1)?;
            p[0].write(b);
            Ok(1)
        }

        fn write_byte_array(&mut self, bytes: &[u8]) -> Result<Acc> {
            let len = bytes.len();
            let buf = self.reserve(len)?;

            // unstable feature maybe_uninit_write_slice
            buf.write_copy_of_slice(bytes);

            Ok(len as u32)
        }
    }
} // mod into_buf_writer
