#![feature(maybe_uninit_array_assume_init)]

use ::cxs::*;
// use ::cxs::codec::writer::Result as WriteResult;
use ::xdr_derive::*;

mod common;
use common::*;

#[derive(Debug, PartialEq, WriteXDR, ReadXDR)]
struct Foo {
    bytes: OwnedByteArray<3>,
    blerg: i32,
}

#[derive(Debug, PartialEq, WriteXDR, ReadXDR)]
struct Boo<'a> {
    blerg: i32,
    bytes: ByteArray<'a, 9>,
}

#[test]
fn struct_foo() {
    // Given: a value and its expected encoding
    let val: Foo = Foo {
        bytes: OwnedByteArray::from([0xAA, 0xBB, 0xCC]),
        blerg: 7
    };
    let expected_hex = "aabbcc07";
    let expected = &hex::decode(expected_hex).unwrap();

    check_encode(&val, expected);

    let val2: Foo = decode(expected).unwrap();
    assert_eq!(val2, val);
}

#[test]
fn struct_boo() {
    let bytes = [7u8; 9];
    let val = Boo {
        blerg: -5,
        bytes: ByteArray::from(&bytes),
    };
    let expected_hex = "3a070707070707070707";
    let expected = &hex::decode(expected_hex).unwrap();

    check_encode(&val, expected);

    let val2: Boo = decode(expected).unwrap();
    assert_eq!(val2, val);
}
