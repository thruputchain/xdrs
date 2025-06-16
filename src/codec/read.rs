//! Read XDR values from some format
//!
//! Primitives:
//! - i32
//! - byte_array (size known by decoder)

use thiserror::Error;

/// Objects that can source XDR values
pub trait Read<'r> {
    /// Read an array of known length
    /// borrowing the contents from the reader
    fn read_byte_array(&mut self, len: u32) -> Result<&'r [u8]>;

    /// Read a byte-sequence of unknown length,
    /// borrowing the contents from the reader
    fn read_bytes(&mut self) -> Result<&'r [u8]>;

    fn read_i32(&mut self) -> Result<i32>;

    fn read_u32(&mut self) -> Result<u32>;
}

pub type Result<T, E = Error> = core::result::Result<T, E>;

#[derive(Copy, Clone, Debug, Error)]
pub enum Error {
    #[error("source is empty (req: {requested}, rem: {remaining}")]
    SourceEmpty {
        requested: usize,
        remaining: usize,
    }
}

// ---------- CXS format implementation -----------------------------------------------

pub struct BufReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> BufReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        let pos = 0;
        Self { data, pos }
    }
}

impl<'r> Read<'r> for BufReader<'r> {
    fn read_byte_array(&mut self, len: u32) -> Result<&'r [u8]>
    {
        let extent = self.pos + len as usize;
        if extent > self.data.len() {
            let requested = len as usize;
            let remaining = self.data.len() - self.pos;
            return Err(Error::SourceEmpty { requested, remaining });
        }

        let shared = &self.data[self.pos .. extent];
        self.pos += len as usize;

        Ok(shared)
    }

    fn read_bytes(&mut self) -> Result<&'r [u8]> {
        let len = self.read_u32()?;
        self.read_byte_array(len)
    }

    fn read_i32(&mut self) -> Result<i32> {
        let int_bytes = self.read_byte_array(4)?.try_into().unwrap();
        Ok(i32::from_le_bytes(int_bytes))
    }

    fn read_u32(&mut self) -> Result<u32> {
        let int_bytes = self.read_byte_array(4)?.try_into().unwrap();
        Ok(u32::from_le_bytes(int_bytes))
    }
}
