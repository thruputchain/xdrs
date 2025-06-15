pub mod codec;
pub mod xdr;

use codec::read::*;
use codec::write::*;

pub use xdr::deconstruct::Deconstruct;
pub use xdr::construct::Construct;

// Utilities

use core::mem::MaybeUninit;

pub fn encode_into<T: Deconstruct>(value: &T, buf: &mut [MaybeUninit<u8>]) -> usize {
    let mut writer = IntoBufWriter::new(buf);
    let _acc = value.deconstruct(&mut writer).unwrap();
    writer.written()
}


// Allocating

pub fn encode<T: Deconstruct>(value: &T) -> Vec<u8> {
    let mut writer = BufWriter::new();
    let _acc = value.deconstruct(&mut writer).unwrap();
    writer.into()
}

pub fn decode<'c, T: Construct<'c>>(
    buf: &'c [u8],
    value: &'c mut T,
) {
    let mut reader = BufReader::new(buf);
    value.construct(&mut reader).unwrap();
}
