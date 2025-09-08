//! Convenient interfaces to CXS serialization

use crate::*;
use crate::serialize_txs::buf_reader::*;
use crate::serialize_txs::into_buf_writer::*;
use core::mem::MaybeUninit as MU;

pub fn decode<'r, 'a: 'r, S, T>(buf: &'a [u8]) -> ReadResult<T>
where
    T: ReadXDR<'r, State = ()>,
    for<'a> (): From<&'a S>
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

pub fn encode_into<T>(value: &T, buf: &mut [MU<u8>]) -> WriteResult<usize>
where
    T: WriteXDR + ?Sized,
{
    let mut writer = IntoBufWriter::new(buf);

    value.write_xdr(&mut writer)?;

    Ok(writer.written())
}

#[cfg(feature = "alloc")]
mod utils_alloc {
    use crate::*;
    use crate::serialize_txs::buf_writer::*;
    use alloc::vec::Vec;

    pub fn encode<T>(value: &T) -> WriteResult<Vec<u8>>
    where
        T: WriteXDR + ?Sized,
    {
        let mut writer = BufWriter::new();

        value.write_xdr(&mut writer)?;

        Ok(writer.into())
    }
}

#[cfg(feature = "alloc")]
pub use utils_alloc::*;
