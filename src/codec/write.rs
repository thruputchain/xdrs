//! Receive XDR values

use thiserror::Error;

#[derive(Copy, Clone, Debug, Error)]
pub enum Error {}

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
