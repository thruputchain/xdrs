use super::put::*;
use crate::serialize::writer::*;

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
        'a: 's,
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

impl<'a> Put for IntoBufWriter<'a> {
    fn put_byte(&mut self, b: u8) -> Result<()> {
        let p = self.reserve(1)?;
        p[0].write(b);
        Ok(())
    }

    fn put_bytes(&mut self, bytes: &[u8]) -> Result<()> {
        let len = bytes.len();
        let buf = self.reserve(len)?;

        // unstable feature maybe_uninit_write_slice
        buf.write_copy_of_slice(bytes);

        Ok(())
    }
}
