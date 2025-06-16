#![feature(maybe_uninit_array_assume_init)]

mod common;
use common::*;

#[test]
fn arrays() {
    let arr = [0u8, 1, 2];
    let buf: &[u8] = &arr;
    let expected_hex = "03000102";
    let expected = &hex::decode(expected_hex).unwrap();

    // check_encode(&buf, expected);
    check_decode(&buf, expected);
}
