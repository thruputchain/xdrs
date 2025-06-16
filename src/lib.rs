#![no_std]
#![feature(maybe_uninit_write_slice)]
#![allow(ambiguous_glob_reexports)]

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "alloc")]
use alloc::vec::Vec;

pub mod codec;
pub mod xdr;

pub use codec::read::*;
pub use codec::read::Result as ReadResult;
pub use codec::write::*;
pub use codec::write::Result as WriteResult;
pub use xdr::construct::*;
pub use xdr::deconstruct::*;

// Utilities

use core::mem::MaybeUninit as MU;

pub fn encode_into<T: Deconstruct>(value: &T, buf: &mut [MU<u8>]) -> usize {
    let mut writer = IntoBufWriter::new(buf);
    let _acc = value.deconstruct(&mut writer).unwrap();
    writer.written()
}

#[cfg(feature = "alloc")]
pub fn encode<T: Deconstruct>(value: &T) -> Vec<u8> {
    let mut writer = BufWriter::new();
    let _acc = value.deconstruct(&mut writer).unwrap();
    writer.into()
}

pub fn decode<'c, T: Construct<'c>> (
    buf: &'c [u8],
) -> ReadResult<T> {
    let mut reader = BufReader::new(buf);
    let mut value: MU<T> = MU::uninit();
    construct(&mut value, &mut reader)?;

    // SAFETY: if it doesn't panic, construct() has initialized the value
    let value = unsafe { value.assume_init() };

    Ok(value)
}
