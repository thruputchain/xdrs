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
    CXS::check_codec(&buf, expected);

    let expected = &hex!("0000000300010200");
    TXS::check_codec(&buf, expected);

    // another padding check
    let arr: [u8; 7] = [0, 1, 2, 3, 4, 5, 6];
    let buf = &arr[..];
    TXS::check_codec(&buf, &hex!("000000070001020304050600"));
}

#[test]
fn byte_array() {
    let arr: [u8; 3] = [0, 1, 2];
    let buf: &[u8; 3] = &arr;

    let expected = &hex!("000102");
    CXS::check_codec(&buf, expected);

    let expected = &hex!("00010200");
    TXS::check_codec(&buf, expected);
}

#[test]
#[cfg(feature = "alloc")]
fn byte_vec() {
    let vec: Vec<u8> = vec![0, 1, 2];

    let expected = &hex!("03000102");
    CXS::check_codec(&vec, expected);

    let expected = &hex!("0000000300010200");
    TXS::check_codec(&vec, expected);
}

#[test]
fn optional_data() {
    let n: i32 = 5;
    let mut v = Some(n);

    let expected = &hex!("0105");
    CXS::check_codec(&v, expected);

    let expected = &hex!("0000000100000005");
    TXS::check_codec(&v, expected);

    v = None;

    let expected = &hex!("00");
    CXS::check_codec(&v, expected);

    let expected = &hex!("00000000");
    TXS::check_codec(&v, expected);
}

#[test]
fn int_array() {
    let v: [i32; 3] = [-1, 0, 1];

    let expected = &hex!("3e0001");
    CXS::check_codec(&v, expected);

    let expected = &hex!("ffffffff0000000000000001");
    TXS::check_codec(&v, expected);
}

#[test]
fn unpadded_array() {
    let v: [u8; 32] = [0xff; 32];

    let expected = &hex!("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff");
    CXS::check_codec(&v, expected);
    TXS::check_codec(&v, expected);
}

#[cfg(feature = "alloc")]
#[test]
fn int_vec() {
    let v: Vec<i32> = vec![-1, 0, 1];

    let expected = &hex!("033e0001");
    CXS::check_codec(&v, expected);

    let expected = &hex!("00000003ffffffff0000000000000001");
    TXS::check_codec(&v, expected);
}

#[cfg(feature = "alloc")]
#[test]
fn empty_vec() {
    let v: Vec<i32> = vec![];

    CXS::check_codec(&v, &hex!("00"));
    TXS::check_codec(&v, &hex!("00000000"));
}

#[cfg(feature = "alloc")]
#[test]
fn string() {
    let s: String = "String!".to_owned();

    CXS::check_codec(&s, &hex!("07537472696e6721"));
    TXS::check_codec(&s, &hex!("00000007537472696e672100"));
}

#[cfg(feature = "alloc")]
#[allow(dead_code)]
fn array_too_big() {
    // should produce compiler error
    let _v: [bool; 8_000_000_000] = ::xdrs::serialize_cxs::decode(b"foo").unwrap();
}
