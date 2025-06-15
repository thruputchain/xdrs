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

    fn write_byte_array(&mut self, bytes: &[u8]) -> Result<Acc>;
    fn write_i32(&mut self, n: i32) -> Result<Acc>;
    fn write_u32(&mut self, n: u32) -> Result<Acc>;
}

// ------------ CXS format implementations ---------------------------------------

#[cfg(feature = "alloc")]
pub use buf_writer::*;

#[cfg(feature = "alloc")]
mod buf_writer {
    use super::*;

    pub struct BufWriter(Vec<u8>);

    impl BufWriter {
        pub fn new() -> Self {
            Self(vec![])
        }
    }

    impl Write for BufWriter {
        // type Acc = u32;

        fn write_byte_array(&mut self, bytes: &[u8]) -> Result<Acc> {
            assert!(bytes.len() < u32::MAX as usize);

            self.0.extend_from_slice(bytes);

            Ok(bytes.len() as u32)
        }

        fn write_i32(&mut self, n: i32) -> Result<Acc> {
            let int_bytes = n.to_le_bytes();
            self.write_byte_array(&int_bytes)
        }

        fn write_u32(&mut self, n: u32) -> Result<Acc> {
            let int_bytes = n.to_le_bytes();
            self.write_byte_array(&int_bytes)
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
        fn cursor<'s>(&'s mut self) -> &'s mut [Mu8]
        where
            'a: 's
        {
            &mut self.buf[self.written..]
        }

        fn reserve<'s>(&'s mut self, len: usize) -> Result<&'s mut [Mu8]>
        where
            'a: 's
        {
            let buf = self.cursor();

            if len > buf.len() {
                return Err(Error::SpaceExhausted);
            }

            Ok(&mut buf[..len])
        }
    }

    impl<'a> IntoBufWriter<'a> {
        pub fn new(buf: &'a mut [Mu8]) -> Self {
            Self { buf, written: 0 }
        }

        pub fn written(&self) -> usize {
            self.written
        }
    }

    impl<'a> Write for IntoBufWriter<'a> {
        fn write_byte_array(&mut self, bytes: &[u8]) -> Result<Acc> {
            let len = bytes.len();
            let buf = self.reserve(len)?;

            // unstable feature maybe_uninit_write_slice
            buf.write_copy_of_slice(bytes);

            self.written += len;

            Ok(len as u32)
        }

        fn write_i32(&mut self, n: i32) -> Result<Acc> {
            let int_bytes: [u8; 4] = n.to_le_bytes();
            self.write_byte_array(&int_bytes)
        }

        fn write_u32(&mut self, n: u32) -> Result<Acc> {
            let int_bytes: [u8; 4] = n.to_le_bytes();
            self.write_byte_array(&int_bytes)
        }
    }
} // mod into_buf_writer
