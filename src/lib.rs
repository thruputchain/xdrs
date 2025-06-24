#![no_std]
#![feature(maybe_uninit_array_assume_init)]
#![feature(maybe_uninit_write_slice)]
#![allow(ambiguous_glob_reexports)]

#[cfg(feature = "alloc")]
extern crate alloc;

pub mod bytes;

// Traits for translating Rust values to and from XDR
pub mod write_xdr;
pub mod read_xdr;

// The abstract interface for serialization formats
pub mod codec;

// An implementation of CXS serialization
pub mod cxs;

pub use ::xdr_derive::*;

pub mod prelude {
    pub use crate::bytes::*;
    pub use crate::codec::reader::*;
    pub use crate::codec::reader::Result as ReadResult;
    pub use crate::codec::reader::Error as ReadError;
    pub use crate::codec::writer::*;
    pub use crate::codec::writer::Result as WriteResult;
    pub use crate::read_xdr::*;
    pub use crate::write_xdr::*;
}
pub use prelude::*;

mod cxs_utils {
    use core::mem::MaybeUninit as MU;
    use crate::prelude::*;
    use crate::cxs::into_buf_writer::*;
    use crate::cxs::buf_reader::*;

    pub fn decode<'r, 'a: 'r, T>(buf: &'a [u8]) -> ReadResult<T>
    where
        T: ReadXDR<'r>
    {
        let mut reader = BufReader::new(buf);

        let value = read_xdr(&mut reader)?;

        debug_assert!(reader.position() <= buf.len());

        let remaining = buf.len().checked_sub(reader.position()).unwrap();
        if remaining > 0 {
            let remaining = remaining as u32;
            return Err(ReadError::SourceRemaining { remaining })
        }

        Ok(value)
    }

    pub fn encode_into<T: WriteXDR>(value: &T, buf: &mut [MU<u8>]) -> usize {
        let mut writer = IntoBufWriter::new(buf);
        let _acc = value.write_xdr(&mut writer).unwrap();
        writer.written()
    }

}
pub use cxs_utils::*;

#[cfg(feature = "alloc")]
mod cxs_utils_alloc {
    use alloc::vec::Vec;
    use crate::prelude::*;
    use crate::cxs::buf_writer::*;

    #[cfg(feature = "alloc")]
    pub fn encode<T: WriteXDR>(value: &T) -> Vec<u8> {
        let mut writer = BufWriter::new();
        let _acc = value.write_xdr(&mut writer).unwrap();
        writer.into()
    }

    #[cfg(feature = "alloc")]
    pub fn encode_into_vec<T: WriteXDR>(value: &T, vec: Vec<u8>) -> Vec<u8> {
        let mut writer = BufWriter::from(vec);
        let _acc = value.write_xdr(&mut writer).unwrap();
        writer.into()
    }
}

#[cfg(feature = "alloc")]
pub use cxs_utils_alloc::*;
