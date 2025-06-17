#![feature(maybe_uninit_array_assume_init)]

use cxs::*;

mod common;
use common::*;

use core::mem::MaybeUninit as MU;

#[derive(Debug, Clone, PartialEq)]
struct Foo {
    bytes: [u8; 3],
    blerg: i32,
}

impl<'p> Construct<'p> for Foo {
    fn construct(foo: &mut MU<Foo>, r: &mut dyn Read<'p>) -> ReadResult<()> {
        let foo_ptr = foo.as_mut_ptr();

        let bytes = r.read_byte_array(3)?.try_into().unwrap();
        unsafe {
            let foo_bytes = &raw mut (*foo_ptr).bytes;
            foo_bytes.write(bytes);
        }

        let blerg = r.read_i32()?;
        unsafe {
            let foo_blerg = &raw mut (*foo_ptr).blerg;
            foo_blerg.write(blerg);
        }

        Ok(())
    }
}

impl Deconstruct for Foo {
    fn deconstruct(&self, w: &mut dyn Write) -> WriteResult<Acc> {
        let mut acc = 0;

        acc += w.write_byte_array(&self.bytes)?;
        acc += w.write_i32(self.blerg)?;

        Ok(acc)
    }
}

#[test]
fn struct_foo() {
    // Given: a value and its expected encoding
    let val: Foo = Foo { blerg: 7, bytes: [0xAA, 0xBB, 0xCC] };
    let expected_hex = "aabbcc07";
    let expected = &hex::decode(expected_hex).unwrap();

    check_encode(&val, expected);

    let val2: Foo = cxs::decode(expected).unwrap();
    assert_eq!(val2, val);
}
