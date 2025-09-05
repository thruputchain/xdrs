//! Convenient interfaces to CXS serialization

use crate::*;
use crate::serialize_cxs::buf_reader::*;
use crate::serialize_cxs::into_buf_writer::*;
use core::mem::MaybeUninit as MU;

pub fn decode_structured<'r, 'a: 'r, S, T>(buf: &'a [u8], structure: S) -> ReadResult<T>
where
    S: Structured,
    T: ReadXDR<'r, S>,
{
    let mut reader: BufReader<'r, S> = BufReader::new_structured(buf, structure);

    let value = read_xdr(&mut reader)?;

    debug_assert!(reader.position() <= buf.len());

    let remaining = buf.len().checked_sub(reader.position()).unwrap();
    if remaining > 0 {
        let remaining = remaining as u32;
        return Err(ReadError::SourceRemaining { remaining });
    }

    Ok(value)
}

pub fn decode<'r, 'a: 'r, T>(buf: &'a [u8]) -> ReadResult<T>
where
    T: ReadXDR<'r>,
{
    let mut reader: BufReader<'r, ()> = BufReader::new(buf);

    let value = read_xdr(&mut reader)?;

    debug_assert!(reader.position() <= buf.len());

    let remaining = buf.len().checked_sub(reader.position()).unwrap();
    if remaining > 0 {
        let remaining = remaining as u32;
        return Err(ReadError::SourceRemaining { remaining });
    }

    Ok(value)
}

pub fn encode_into<T: WriteXDR + ?Sized>(value: &T, buf: &mut [MU<u8>]) -> WriteResult<usize> {
    let mut writer = IntoBufWriter::new(buf);

    value.write_xdr(&mut writer)?;

    Ok(writer.written())
}

#[cfg(feature = "alloc")]
mod utils_alloc {
    use crate::*;
    use crate::serialize_cxs::buf_writer::*;
    use alloc::vec::Vec;

    #[cfg(feature = "alloc")]
    pub fn encode<T: WriteXDR + ?Sized>(value: &T) -> WriteResult<Vec<u8>> {
        let mut writer = BufWriter::new();

        value.write_xdr(&mut writer)?;

        Ok(writer.into())
    }
}

#[cfg(feature = "alloc")]
pub use utils_alloc::*;
