//! Receive XDR values

use core::mem::MaybeUninit;
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

    fn write_i32(&mut self, n: i32) -> Result<Acc>;
}

// ---------------------------------------------------------

pub struct BufWriter(Vec<u8>);

impl BufWriter {
    pub fn new() -> Self {
        Self(vec![])
    }
}

impl Write for BufWriter {
    // type Acc = u32;

    fn write_i32(&mut self, n: i32) -> Result<Acc> {
        let int_bytes = n.to_le_bytes();
        self.0.extend_from_slice(&int_bytes);

        Ok(4)
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

// ---------------------------------------------------------

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
    fn write_i32(&mut self, n: i32) -> Result<Acc> {
        let buf = self.cursor();

        if buf.len() < 4 {
            return Err(Error::SpaceExhausted)
        }

        let int_bytes: &mut [Mu8; 4] = buf.try_into().map_err(|_e|
            Error::SpaceExhausted)?;

        for (i, b) in n.to_le_bytes().into_iter().enumerate() {
            int_bytes[i].write(b);
        }

        self.written += 4;

        Ok(4)
    }
}

/*
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
*/
