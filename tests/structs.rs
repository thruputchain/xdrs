#![feature(maybe_uninit_array_assume_init)]

use cxs::*;

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
impl<'p> Construct<'p> for Foo {
    fn construct(foo: &mut MU<Foo>, r: &mut dyn Read<'p>) -> ReadResult<()> {
        let foo_ptr = foo.as_mut_ptr();

        let foo_bytes: &mut MU<OwnedByteArray<3>> = unsafe {
            transmute(&raw mut (*foo_ptr).bytes)
        };
        construct(foo_bytes, r)?;

        let foo_blerg: &mut MU<i32> = unsafe {
            transmute(&raw mut (*foo_ptr).blerg)
        };
        construct(foo_blerg, r)?;

        Ok(())
    }
}

// derive this
impl Deconstruct for Foo {
    fn deconstruct(&self, w: &mut dyn Write) -> WriteResult<Acc> {
        let mut acc = 0;

        acc += deconstruct(&self.bytes, w)?;
        acc += deconstruct(&self.blerg, w)?;

        Ok(acc)
    }
}

#[test]
fn struct_foo() {
    // Given: a value and its expected encoding
    let val: Foo = Foo { bytes: OwnedByteArray::from([0xAA, 0xBB, 0xCC]), blerg: 7 };
    let expected_hex = "aabbcc07";
    let expected = &hex::decode(expected_hex).unwrap();

    check_encode(&val, expected);

    let val2: Foo = cxs::decode(expected).unwrap();
    assert_eq!(val2, val);
}
