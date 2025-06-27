use super::put::*;
use crate::serialize::writer::*;

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

// Implement the basic byte operations in order
// to get a Writer implementation for free
impl Put for BufWriter {
    fn put_byte(&mut self, b: u8) -> Result<()> {
        self.0.push(b);
        Ok(())
    }

    fn put_bytes(&mut self, bytes: &[u8]) -> Result<()> {
        if bytes.len() > u32::MAX as usize {
            return Err(Error::InvalidArrayLength);
        }

        self.0.extend_from_slice(bytes);

        Ok(())
    }
}
