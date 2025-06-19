#![feature(maybe_uninit_array_assume_init)]

mod common;
use common::*;

use cxs::*;

#[test]
fn byte_slice() {
    let arr: [u8; 3] = [0, 1, 2];
    let buf = Bytes::from(&arr[..]);
    let expected = &hex!("03000102");

    check_codec(&buf, expected);
}

#[test]
fn byte_array() {
    let arr: [u8; 3] = [0, 1, 2];
    let buf = ByteArray::from(&arr);
    let expected = &hex!("000102");

    check_codec(&buf, expected);
}

#[test]
fn owned_byte_array() {
    let arr: [u8; 3] = [0, 1, 2];
    let buf = OwnedByteArray::from(arr);
    let expected = &hex!("000102");

    check_encode(&buf, expected);
    check_decode(&buf, expected);
}
