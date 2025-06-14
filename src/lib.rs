pub mod codec;
pub mod xdr;

use xdr::deconstruct::*;
use xdr::construct::*;
use codec::read::*;
use codec::write::*;

// TODO: non-allocating ones

pub fn encode<T: Deconstruct>(value: &T) -> Vec<u8> {
    let mut receiver = BufWriter::new();
    let _acc = value.deconstruct(&mut receiver).unwrap();
    receiver.into()
}

pub fn decode<'c, T: Construct<'c>>(
    buf: &'c [u8],
    value: &'c mut T,
) {
    let mut reader = BufReader::new(buf);
    value.construct(&mut reader).unwrap();
}
