//! Convenient interfaces to CXS serialization

use crate::*;
use crate::serialize_cxs::buf_reader::*;
use crate::serialize_cxs::into_buf_writer::*;
use core::mem::MaybeUninit as MU;

#[cfg(feature = "alloc")]
use crate::serialize_cxs::state_reader::*;

pub fn decode<'a, 'r, T>(buf: &'a [u8]) -> ReadResult<T>
where
    'a: 'r,
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

#[allow(unused_variables)]
pub fn decode_state<'r, 'a, T>(buf: &'a [u8]) -> ReadResult<T>
where
    'a: 'r,
    T: ReadXDRState<'r>,
{
    #[cfg(not(feature = "alloc"))]
    panic!("decode_state() currently requires feature \"alloc\"");

    #[cfg(feature = "alloc")]
    {
        let mut reader = StateReader::new(BufReader::new(buf));

        decode_state_reader(&mut reader)
    }
}

pub fn decode_state_reader<'r, R, T>(reader: &mut R) -> ReadResult<T>
where
    T: ReadXDRState<'r>,
    R: ReaderState<'r>,
{
    let value = read_xdr_state(reader)?;

    if reader.has_more_input() {
        return Err(ReadError::TrailingData);
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
