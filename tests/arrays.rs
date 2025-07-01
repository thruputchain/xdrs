#![feature(maybe_uninit_array_assume_init)]

mod common;
use common::*;

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

    check_encode(&arr, expected);
    check_decode(&arr, expected);
}

#[test]
fn optional_data() {
    let n: i32 = 5;
    let mut v = Some(n);
    let expected = &hex!("0105");

    check_encode(&v, expected);
    check_decode(&v, expected);

    v = None;
    let expected = &hex!("00");

    check_encode(&v, expected);
    check_decode(&v, expected);
}

#[test]
fn int_array() {
    let v: [i32; 3] = [-1, 0, 1];
    let expected = &hex!("3e0001");

    check_encode(&v, expected);
    check_decode(&v, expected);
}
