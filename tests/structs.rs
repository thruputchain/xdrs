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
    let expected_hex = "aabbcc07000000";
    let expected = &hex::decode(expected_hex).unwrap();

    check_encode(&val, expected);

    let val2: Foo = cxs::decode(expected).unwrap();
    assert_eq!(val2, val);
}

/*
fn check_encode_into_static<T>(value: &T, expected: &[u8])
where
    T: cxs::Deconstruct
{
    const BUFSZ: usize = 1000;
    assert!(expected.len() <= BUFSZ);

    // An "uninitialized" buffer that has actually been zeroed
    let mut buf: [MU<u8>; BUFSZ] = [const { MU::zeroed() }; BUFSZ];

    let written = encode_into(value, &mut buf);
    let buf = unsafe { MU::array_assume_init(buf) };
    let encoded = &buf[..written];

    assert_eq!(encoded, expected);
}

#[cfg(feature = "alloc")]
fn check_encode_into_alloc<T>(value: &T, expected: &[u8])
where T: cxs::Deconstruct
{
    let encoded = encode_into_alloc(value, expected.len());

    assert_eq!(&encoded, expected);
}

#[cfg(feature = "alloc")]
// Allocate a buffer of the expected size and encode
// the value into it
fn encode_into_alloc<T>(value: &T, expected_len: usize) -> Vec<u8>
where T: cxs::Deconstruct
{
    let mut encoded = Vec::with_capacity(expected_len);
    let written = cxs::encode_into(value, encoded.spare_capacity_mut());

    // SAFETY:
    // - the new len must be less than or equal to capacity().
    // - the elements at old_len..new_len must be initialized.
    assert!(written <= encoded.capacity());
    unsafe {
        encoded.set_len(written);
    }

    encoded
}
*/
