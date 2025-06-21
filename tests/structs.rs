#![feature(maybe_uninit_array_assume_init)]

use ::cxs::*;
use ::cxs::codec::writer::Result as WriteResult;

mod common;
use common::*;

use core::mem::MaybeUninit as MU;
use core::mem::transmute;

#[derive(Debug, PartialEq)]
struct Foo {
    bytes: OwnedByteArray<3>,
    blerg: i32,
}

// derive this
impl<'p> ReadXDR<'p> for Foo {
    fn read_xdr(foo: &mut MU<Foo>, r: &mut dyn Reader<'p>) -> ReadResult<()> {
        let foo_ptr = foo.as_mut_ptr();

        let foo_bytes: &mut MU<OwnedByteArray<3>> = unsafe {
            transmute(&raw mut (*foo_ptr).bytes)
        };
        read_xdr(foo_bytes, r)?;

        let foo_blerg: &mut MU<i32> = unsafe {
            transmute(&raw mut (*foo_ptr).blerg)
        };
        read_xdr(foo_blerg, r)?;

        Ok(())
    }
}

// derive this
impl WriteXDR for Foo {
    fn write_xdr<W: Writer>(&self, w: W) -> WriteResult {
        let mut s = w.write_struct()?;

        s.write_field(&self.bytes)?;
        s.write_field(&self.blerg)?;

        Ok(())
    }
}

#[test]
fn struct_foo() {
    // Given: a value and its expected encoding
    let val: Foo = Foo { bytes: OwnedByteArray::from([0xAA, 0xBB, 0xCC]), blerg: 7 };
    let expected_hex = "aabbcc07";
    let expected = &hex::decode(expected_hex).unwrap();

    check_encode(&val, expected);

    let val2: Foo = decode(expected).unwrap();
    assert_eq!(val2, val);
}
