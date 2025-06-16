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
        requested: u32,
        remaining: u32,
    },

    #[error("invalid header byte: {h:02x}")]
    InvalidHeaderByte {
        h: u8
    },

    #[error("bad U32 encoding")]
    BadU32,
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

    fn reserve_extent(&self, requested: u32) -> Result<usize> {
        let extent = self.pos + requested as usize;
        if extent > self.data.len() {
            let remaining = (self.data.len() - self.pos).try_into().unwrap();
            Err(Error::SourceEmpty { requested, remaining })
        } else {
            Ok(extent)
        }
    }

    fn read_byte(&mut self) -> Result<u8> {
        let _ = self.reserve_extent(1)?;
        let byte = self.data[self.pos];

        self.pos += 1;

        Ok(byte)
    }

    fn read_signed(&mut self) -> Result<i64> {
        let h = self.read_byte()?;
        let s = (h & 0xc0) >> 6;
        let v = h & !0xc0;

        if v == 63 {
            if s > 3 {
                return Err(Error::InvalidHeaderByte { h })
            }

            let n_bytes: usize = 2usize.pow(s as u32);
            let input = self.read_byte_array(n_bytes as u32)?;

            let mut int_bytes = [0u8; 8];
            let int_dest = &mut int_bytes[0..n_bytes];
            int_dest.copy_from_slice(input);

            let int = i64::from_le_bytes(int_bytes);

            Ok(int)
        } else {
            if s != 0 {
                return Err(Error::InvalidHeaderByte { h })
            }

            let n: i64;
            if v & 0x20 != 0 {
                // the sign-bit (for i6) is set, so
                // sign-extend to 8 bits and add one,
                // per the encoding
                n = (v | 0xc0) as i8 as i64 + 1;
            } else {
                n = v as i64;
            }

            Ok(n)
        }
    }

    fn read_unsigned(&mut self) -> Result<u64> {
        let h = self.read_byte()?;
        let s = (h & 0xc0) >> 6;
        let v = h & !0xc0;

        if v == 63 {
            if s > 3 {
                return Err(Error::InvalidHeaderByte { h })
            }

            let n_bytes: usize = 2usize.pow(s as u32);
            let input = self.read_byte_array(n_bytes as u32)?;

            let mut int_bytes = [0u8; 8];
            let int_dest = &mut int_bytes[0..n_bytes];

            int_dest.copy_from_slice(input);
            let uint = u64::from_le_bytes(int_bytes);

            Ok(uint)
        } else {
            if s != 0 {
                return Err(Error::InvalidHeaderByte { h });
            }

            Ok(v as u64)
        }
    }
}

impl<'r> Read<'r> for BufReader<'r> {
    fn read_byte_array(&mut self, len: u32) -> Result<&'r [u8]> {
        let extent = self.reserve_extent(len)?;
        let shared = &self.data[self.pos .. extent];

        self.pos += len as usize;

        Ok(shared)
    }

    fn read_bytes(&mut self) -> Result<&'r [u8]> {
        let len = self.read_u32()?;
        self.read_byte_array(len)
    }

    fn read_i32(&mut self) -> Result<i32> {
        let n = self.read_signed()?;
        let n = n.try_into().map_err(|_| Error::BadU32)?;
        Ok(n)
    }

    fn read_u32(&mut self) -> Result<u32> {
        let n = self.read_unsigned()?;
        let n = n.try_into().map_err(|_| Error::BadU32)?;
        Ok(n)
    }
}
