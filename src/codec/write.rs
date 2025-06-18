//! Write XDR values in some format

use thiserror::Error;

#[derive(Copy, Clone, Debug, Error)]
pub enum Error {
    #[error("space exhausted")]
    SpaceExhausted
}

pub type Result<T, E = Error> = core::result::Result<T, E>;

pub type Acc = u32;

pub trait Write {
    // type Acc;

    fn write_byte(&mut self, b: u8) -> Result<Acc>;

    /// Write a byte array of length known to a decoder
    fn write_byte_array(&mut self, bytes: &[u8]) -> Result<Acc>;

    fn write_i32(&mut self, n: i32) -> Result<Acc> {
        assert_eq!(i16::MAX as u32 as i32, i16::MAX as i32);

        let nu: u32 = n.unsigned_abs();
        if nu < 32 {
            let h;
            if n < 0 {
                // A small negative number: this can be represented as 1's complement in the
                // header byte; i.e., inverted with the sign bit set
                h = (!(nu as u8) & 0x1f) | 0b0010_0000;
            } else {
                h = n as u8;
            }
            self.write_byte(h)
        } else {
            // Otherwise we'll use the header byte plus 1 to 4 bytes
            let v = 63;
            if n > i16::MAX as i32 || n < i16::MIN as i32{
                // use 4-byte repr
                let s = 2 << 6;
                let h = v | s; 
                self.write_byte(h)?;
                let buf = n.to_le_bytes();
                self.write_byte_array(&buf)
            } else if n > i8::MAX as i32 || n < i8::MIN as i32 {
                // use 2-byte repr
                let s = 1 << 6;
                let h = v | s; 
                self.write_byte(h)?;
                let buf = (n as i16).to_le_bytes();
                self.write_byte_array(&buf)
            } else {
                // use 1-byte repr
                let s = 0;
                let h = v | s;
                self.write_byte(h)?;
                self.write_byte(n as u8)
            }
        }
    }

    fn write_u32(&mut self, n: u32) -> Result<Acc> {
        if n <= 32 {
            // This can be represented in the header byte
            self.write_byte(n as u8)
        } else {
            let mut h: u8 = 63;
            match n {
                i if i <= u8::MAX as u32 => {
                    self.write_byte(h)?;
                    self.write_byte(n as u8)
                },
                i if i <= u16::MAX as u32 => {
                    h |= 1 << 6;
                    self.write_byte(h)?;
                    let buf = (n as u16).to_le_bytes();
                    self.write_byte_array(&buf)
                },
                i if i <= u32::MAX as u32 => {
                    h |= 2 << 6;
                    self.write_byte(h)?;
                    let buf = (n as u32).to_le_bytes();
                    self.write_byte_array(&buf)
                },
                _ => unreachable!()
            }
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
            assert!(bytes.len() < u32::MAX as usize);

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
