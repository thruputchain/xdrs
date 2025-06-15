use cxs::*;

use core::mem::MaybeUninit as MU;

#[derive(Debug, Clone, PartialEq)]
struct Foo {
    bytes: [u8; 3],
    blerg: i32,
}

impl<'p> Construct<'p> for Foo {
    fn construct(foo: &mut MU<Foo>, r: &mut dyn Read<'p>) -> ReadResult<()> {
        let foo_ptr = foo.as_mut_ptr();

        unsafe {
            let foo_bytes = &raw mut (*foo_ptr).bytes;
            let foo_blerg = &raw mut (*foo_ptr).blerg;

            let bytes = r.read_byte_array(3)?.try_into().unwrap();
            foo_bytes.write(bytes);

            let blerg = r.read_i32()?;
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
    let val: Foo = Foo { blerg: 7, bytes: [0xAA, 0xBB, 0xCC] };
    let expected_hex = "aabbcc07000000";
    let expected = hex::decode(expected_hex).unwrap();

    let min_space;
    #[cfg(feature = "alloc")]
    {
        let encoded = cxs::encode(&val);
        let encoded_hex = hex::encode(&encoded);
        assert_eq!(&encoded_hex, &expected_hex);
        min_space = encoded.len();
    }
    #[cfg(not(feature = "alloc"))]
    {
        min_space = 1000;
    }

    let encoded = do_encode_into(&val, min_space);
    assert_eq!(&encoded, &expected);

    let val2: Foo = cxs::decode(&encoded);
    assert_eq!(val2, val);
}

fn do_encode_into<T>(value: &T, min_space: usize) -> Vec<u8>
where T: cxs::Deconstruct
{
    let mut encoded = Vec::with_capacity(min_space);
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
