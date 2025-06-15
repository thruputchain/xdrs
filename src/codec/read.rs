//! Read XDR values from some format
//!
//! Primitives:
//! - i32
//! - byte_array (size known by decoder)

use thiserror::Error;

pub trait Read<'p> {
    fn read_byte_array(&mut self, len: usize) -> Result<&'p [u8]>;
    fn read_i32(&mut self) -> Result<i32>;
    fn read_u32(&mut self) -> Result<u32>;
}

pub type Result<T, E = Error> = core::result::Result<T, E>;

#[derive(Copy, Clone, Debug, Error)]
pub enum Error {
    #[error("provider source is empty")]
    SourceEmpty,
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
    fn read_byte_array(&mut self, len: usize) -> Result<&'r [u8]>
    {
        if len > self.data.len() - self.pos {
            return Err(Error::SourceEmpty);
        }

        let shared = &self.data[self.pos .. len];
        self.pos += len;
        Ok(shared)
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
