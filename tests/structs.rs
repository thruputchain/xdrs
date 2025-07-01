#![feature(maybe_uninit_array_assume_init)]

use ::xdrs::serialize_cxs::*;
use ::xdrs::*;
use core::ops::Deref;

mod common;
use common::*;

#[test]
fn struct_foo() {
    #[derive(Debug, PartialEq, WriteXDR, ReadXDR)]
    struct Foo {
        bytes: OwnedByteArray<3>,
        blerg: i32,
    }

    // Given: a value and its expected encoding
    let val: Foo = Foo {
        bytes: OwnedByteArray::from([0xAA, 0xBB, 0xCC]),
        blerg: 7,
    };
    let expected_hex = "aabbcc07";
    let expected = &hex::decode(expected_hex).unwrap();

    check_encode(&val, expected);

    let val2: Foo = decode(expected).unwrap();
    assert_eq!(val2, val);
}

#[test]
fn struct_boo() {
    #[derive(Debug, PartialEq, WriteXDR, ReadXDR)]
    struct Boo<'a> {
        blerg: i32,
        bytes: ByteArray<'a, 9>,
    }

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

#[test]
fn newtype() {
    #[derive(Debug, PartialEq, Eq, WriteXDR, ReadXDR)]
    struct New<'a>(ByteArray<'a, 6>);

    let bytes = [0, 1, 2, 3, 4, 5];
    let val = New(ByteArray::from(&bytes));
    let expected = &hex!("000102030405");

    check_encode(&val, expected);
    check_decode(&val, expected);
}

#[test]
fn as_type() {
    #[derive(Debug, PartialEq, Eq)]
    struct Blob(u32);

    impl Deref for Blob {
        type Target = u32;

        fn deref(&self) -> &u32 {
            &self.0
        }
    }

    impl From<u32> for Blob {
        fn from(n: u32) -> Self {
            Self(n)
        }
    }

    /*
    impl From<Blob> for u32 {
        fn from(b: Blob) -> u32 {
            b.0
        }
    }

    impl AsRef<u32> for Blob {
        fn as_ref(&self) -> &u32 {
            &self.0
        }
    }
    */

    #[derive(Debug, PartialEq, Eq, WriteXDR, ReadXDR)]
    struct New(#[xdrs(as_type = "u32")] Blob);

    let n: u32 = 7;
    let val = New(Blob::from(n));
    let expected = &hex!("07");

    check_encode(&val, expected);
    check_decode(&val, expected);
}
