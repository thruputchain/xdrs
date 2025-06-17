#![allow(dead_code)]

use cxs::*;
use core::mem::MaybeUninit as MU;
use core::fmt::Debug;

pub fn check_codec<'r, T>(val: &T, expected: &'r [u8])
where
    T: Debug + Eq + cxs::Construct<'r> + cxs::Deconstruct
{
    check_encode(val, expected);
    check_decode(val, expected);
}

pub fn check_decode<'r, T>(val: &T, expected: &'r [u8])
where
    T: Debug + Eq + cxs::Construct<'r>
{
    let val2: T = decode(expected).unwrap();

    assert_eq!(&val2, val);
}

pub fn check_encode<T>(val: &T, expected: &[u8])
where
    T: cxs::Deconstruct
{
    // Check the allocating encoder
    #[cfg(feature = "alloc")]
    check_encode_alloc(val, expected);

    // Check the static encoder
    check_encode_into_static(val, expected);
}

pub fn check_encode_into_static<T>(value: &T, expected: &[u8])
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

    check_eq(encoded, expected);
}

#[cfg(feature = "alloc")]
pub fn check_encode_alloc<T>(value: &T, expected: &[u8])
where
    T: cxs::Deconstruct
{
    let encoded = encode(value);

    check_eq(&encoded, expected);
}

#[cfg(feature = "alloc")]
pub fn check_encode_into_alloc<T>(value: &T, expected: &[u8])
where T: cxs::Deconstruct
{
    let encoded = encode_into_alloc(value, expected.len());

    check_eq(&encoded, expected);
}

#[cfg(feature = "alloc")]
// Allocate a buffer of the expected size and encode
// the value into it
pub fn encode_into_alloc<T>(value: &T, expected_len: usize) -> Vec<u8>
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

// panic with readable output
pub fn check_eq(encoded: &[u8], expected: &[u8]) {
    // assert!(a.len() == b.len());

    let a_hex = hex::encode(encoded);
    let b_hex = hex::encode(expected);

    assert_eq!(a_hex, b_hex);
}

/*
pub fn check_roundtrip<'r, T>(value: &T)
where
    T: Debug + Eq + cxs::Deconstruct + cxs::Construct<'r>
{
    let vec = vec![];
    let vec = encode_into_vec(value, vec);

    let value2: T = decode(&vec).unwrap();

    assert_eq!(&value2, value);
}
*/
