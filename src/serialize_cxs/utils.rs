//! Convenient interfaces to CXS serialization

use crate::prelude::*;
use crate::serialize_cxs::buf_reader::*;
use crate::serialize_cxs::into_buf_writer::*;
use core::mem::MaybeUninit as MU;

pub fn decode<'r, 'a: 'r, T>(buf: &'a [u8]) -> ReadResult<T>
where
    T: ReadXDR<'r>,
{
    let mut reader = BufReader::new(buf);

    let value = read_xdr(&mut reader)?;

    debug_assert!(reader.position() <= buf.len());

    let remaining = buf.len().checked_sub(reader.position()).unwrap();
    if remaining > 0 {
        let remaining = remaining as u32;
        return Err(ReadError::SourceRemaining { remaining });
    }

    Ok(value)
}

pub fn encode_into<T: WriteXDR>(value: &T, buf: &mut [MU<u8>]) -> WriteResult<usize> {
    let mut writer = IntoBufWriter::new(buf);

    value.write_xdr(&mut writer)?;

    Ok(writer.written())
}

pub struct UninitBuf<const N: usize> {
    buf: [MU<u8>; N],
    valid_len: usize,
}

impl<const N: usize> UninitBuf<N> {
    /// Create an uninitialized buffer with byte-length `N`
    pub const fn new() -> Self {
       Self {
           buf: [MU::uninit(); N],
           valid_len: 0,
       }
    }

    /// Initialize some prefix of the buffer, reporting
    /// how many bytes were initialized
    pub fn init<F>(&mut self, f: F) -> WriteResult<()>
    where
        F: Fn(&mut [MU<u8>]) -> WriteResult<usize>
    {
        self.valid_len = f(&mut self.buf)?;
        Ok(())
    }


    /// Get a reference to the now-allegedly-initialized portion of the buffer
    ///
    /// # Safety
    ///
    /// The function most-recently passed to init, if any,
    /// must have initialized the buffer prefix it claimed to.
    pub unsafe fn assume_init(&self) -> &[u8] {
        unsafe {
            self.buf[..self.valid_len].assume_init_ref()
        }
    }
}

impl<const N: usize> Default for UninitBuf<N> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "alloc")]
mod utils_alloc {
    use crate::prelude::*;
    use crate::serialize_cxs::buf_writer::*;
    use alloc::vec::Vec;

    #[cfg(feature = "alloc")]
    pub fn encode<T: WriteXDR>(value: &T) -> WriteResult<Vec<u8>> {
        let mut writer = BufWriter::new();

        value.write_xdr(&mut writer)?;

        Ok(writer.into())
    }
}

#[cfg(feature = "alloc")]
pub use utils_alloc::*;
