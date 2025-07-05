#![feature(maybe_uninit_array_assume_init)]
#![feature(maybe_uninit_slice)]

mod common;
use common::*;

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "alloc")]
use alloc::{vec, vec::Vec};

#[test]
fn byte_slice() {
    let arr: [u8; 3] = [0, 1, 2];
    let buf = &arr[..];
    let expected = &hex!("03000102");
    check_codec(&buf, expected);
}

#[test]
fn byte_array() {
    let arr: [u8; 3] = [0, 1, 2];
    let buf: &[u8; 3] = &arr;
    let expected = &hex!("000102");
    check_codec(&buf, expected);
}

#[test]
fn owned_byte_array() {
    let arr: [u8; 3] = [0, 1, 2];
    let expected = &hex!("000102");
    check_codec(&arr, expected);
}

#[test]
fn optional_data() {
    let n: i32 = 5;
    let mut v = Some(n);
    let expected = &hex!("0105");
    check_codec(&v, expected);

    v = None;
    let expected = &hex!("00");
    check_codec(&v, expected);
}

#[test]
fn int_array() {
    let v: [i32; 3] = [-1, 0, 1];
    let expected = &hex!("3e0001");
    check_codec(&v, expected);
}

#[cfg(false)]
#[test]
fn array_too_big() {
    // should produce compiler error
    let _v: [bool; 8_000_000_000] = ::xdrs::serialize_cxs::decode(b"foo").unwrap();
}

#[cfg(feature = "alloc")]
#[test]
fn int_vec() {
    let v: Vec<i32> = vec![-1, 0, 1];
    let expected = &hex!("033e0001");
    check_codec(&v, expected);
}
