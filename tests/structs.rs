#![feature(maybe_uninit_array_assume_init)]

use ::cxs::*;
// use ::cxs::codec::writer::Result as WriteResult;
use ::cxs::codec::reader::Result as ReadResult;
use ::xdr_derive::*;

mod common;
use common::*;

#[test]
fn struct_foo() {
    #[derive(Debug, PartialEq, WriteXDR, ReadXDR)]
    struct Foo {
        bytes: OwnedByteArray<3>,
        blerg: i32,
    }

    /*
    use core::mem::MaybeUninit as MU;
    use core::mem::transmute;

    impl<'r> ReadXDR<'r> for Foo {
        fn read_xdr<R: Reader<'r>>(value: &mut MU<Self>, r: &mut R) -> ReadResult<()> {
            let struct_ptr = value.as_mut_ptr();

            let visit_field = |index: u32, r: &mut R| -> ReadResult<()> {
                match index {

                    0 => {
                        let field_mu: &mut MU<OwnedByteArray<3>> = unsafe {
                            transmute(&raw mut (*struct_ptr).bytes)
                        };

                        read_xdr(field_mu, r)
                    },

                    1 => {
                        let field_mu: &mut MU<i32> = unsafe {
                            transmute(&raw mut (*struct_ptr).blerg)
                        };

                        read_xdr(field_mu, r)
                    }

                    _ => panic!("bad field index")
                }
            };

            r.read_struct(2, visit_field)
        }
    }
    */

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
