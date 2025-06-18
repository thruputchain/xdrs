#![no_std]
#![feature(maybe_uninit_write_slice)]
#![allow(ambiguous_glob_reexports)]

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "alloc")]
use alloc::vec::Vec;

pub mod bytes;
pub mod codec;
pub mod xdr;

pub use bytes::*;
pub use codec::read::*;
pub use codec::read::Result as ReadResult;
pub use codec::write::*;
pub use codec::write::Result as WriteResult;
pub use xdr::construct::*;
pub use xdr::deconstruct::*;

// Utilities

use core::mem::MaybeUninit as MU;

pub fn decode<'r, 'a: 'r, T>(buf: &'a [u8]) -> ReadResult<T>
where
    T: Construct<'r>
{
    let mut reader = BufReader::new(buf);
    let mut value: MU<T> = MU::uninit();
    construct(&mut value, &mut reader)?;

    // SAFETY: if it doesn't panic, construct() has initialized the value
    let value = unsafe { value.assume_init() };

    Ok(value)
}

pub fn encode_into<T: Deconstruct>(value: &T, buf: &mut [MU<u8>]) -> usize {
    let mut writer = IntoBufWriter::new(buf);
    let _acc = value.deconstruct(&mut writer).unwrap();
    writer.written()
}

pub fn encode_into_with<T>(value: &T, buf: &mut [MU<u8>], deconstruct: DeconstructFn<T>) -> usize {
    let mut writer = IntoBufWriter::new(buf);
    let _acc = deconstruct(value, &mut writer).unwrap();
    writer.written()
}

pub fn encode_bytes_into(value: &[u8], buf: &mut [MU<u8>]) -> usize {
    encode_into_with(&value, buf, deconstruct_byte_slice)
}

pub fn encode_byte_array_into(value: &[u8], buf: &mut [MU<u8>]) -> usize {
    encode_into_with(&value, buf, deconstruct_byte_array)
}

#[cfg(feature = "alloc")]
pub fn encode<T: Deconstruct>(value: &T) -> Vec<u8> {
    let mut writer = BufWriter::new();
    let _acc = value.deconstruct(&mut writer).unwrap();
    writer.into()
}

#[cfg(feature = "alloc")]
pub fn encode_into_vec<T: Deconstruct>(value: &T, vec: Vec<u8>) -> Vec<u8> {
    let mut writer = BufWriter::from(vec);
    let _acc = value.deconstruct(&mut writer).unwrap();
    writer.into()
}
