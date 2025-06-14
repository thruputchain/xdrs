//! Provide XDR values

use thiserror::Error;

pub trait Provider<'p> {
    fn provide_byte_array(&mut self, len: usize) -> Result<&'p [u8]>;
    fn provide_i32(&mut self) -> Result<i32>;
}

pub type Result<T, E = Error> = core::result::Result<T, E>;

#[derive(Copy, Clone, Debug, Error)]
pub enum Error {
    #[error("provider source is empty")]
    SourceEmpty,
}

// -------------------------------------------------------------

pub struct BufProvider<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> BufProvider<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        let pos = 0;
        Self { data, pos }
    }
}

impl<'p> Provider<'p> for BufProvider<'p> {
    fn provide_byte_array(&mut self, len: usize) -> Result<&'p [u8]>
    {
        if len > self.data.len() - self.pos {
            return Err(Error::SourceEmpty);
        }

        let shared = &self.data[self.pos .. len];
        self.pos += len;
        Ok(shared)
    }

    fn provide_i32(&mut self) -> Result<i32> {
        let int_bytes = self.provide_byte_array(4)?.try_into().unwrap();
        Ok(i32::from_le_bytes(int_bytes))
    }
}
