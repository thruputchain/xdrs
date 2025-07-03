#![feature(maybe_uninit_array_assume_init)]

mod common;
use common::*;

#[test]
fn struct_foo() {
    #[derive(Debug, PartialEq, Eq, xdrs::WriteXDR, xdrs::ReadXDR)]
    struct Foo {
        bytes: [u8; 3],
        blerg: i32,
    }

    // Given: a value and its expected encoding
    let val: Foo = Foo {
        bytes: [0xAA, 0xBB, 0xCC],
        blerg: 7,
    };
    let expected_hex = "aabbcc07";
    let expected = &hex::decode(expected_hex).unwrap();

    check_codec(&val, expected);
}

#[test]
fn struct_boo() {
    #[derive(Debug, PartialEq, Eq, xdrs::WriteXDR, xdrs::ReadXDR)]
    struct Boo<'a> {
        blerg: i32,
        bytes: &'a [u8; 9],
    }

    let bytes = [7u8; 9];
    let val = Boo {
        blerg: -5,
        bytes: &bytes,
    };
    let expected_hex = "3a070707070707070707";
    let expected = &hex::decode(expected_hex).unwrap();

    check_codec(&val, expected);
}

#[test]
fn newtype() {
    #[derive(Debug, PartialEq, Eq, xdrs::WriteXDR, xdrs::ReadXDR)]
    struct New<'a>(&'a [u8; 6]);

    let bytes = [0, 1, 2, 3, 4, 5];
    let val = New(&bytes);
    let expected = &hex!("000102030405");

    check_codec(&val, expected);
}

#[test]
fn as_type() {
    #[derive(Debug, PartialEq, Eq)]
    struct Blob(u32);

    impl core::ops::Deref for Blob {
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

    #[derive(Debug, PartialEq, Eq, xdrs::WriteXDR, xdrs::ReadXDR)]
    struct New(#[xdrs(as_type = "u32")] Blob);

    let n: u32 = 7;
    let val = New(Blob::from(n));
    let expected = &hex!("07");

    check_codec(&val, expected);
}
