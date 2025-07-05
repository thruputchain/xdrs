#![allow(dead_code)]

use ::xdrs::*;

use core::fmt::Debug;
use core::mem::MaybeUninit as MU;

#[macro_export]
macro_rules! hex {
    ($hstr:expr) => {
        ::hex::decode($hstr).unwrap()
    };
}

pub fn check_codec<'r, T>(val: &T, expected: &'r [u8])
where
    T: Debug + Eq + ReadXDR<'r> + WriteXDR,
{
    check_encode(val, expected);
    check_decode(val, expected);
}

pub fn check_decode<'r, T>(val: &T, expected: &'r [u8])
where
    T: Debug + Eq + ReadXDR<'r>,
{
    let val2: T = serialize_cxs::decode(expected).unwrap();
    assert_eq!(&val2, val);

    // let val2: T = serialize_txs::decode(expected).unwrap();
    // assert_eq!(&val2, val);
}

pub fn check_encode<T>(val: &T, expected: &[u8])
where
    T: WriteXDR,
{
    // Check the allocating encoder
    #[cfg(feature = "alloc")]
    check_encode_alloc(val, expected);

    // Check the static encoder
    check_encode_into_static(val, expected, serialize_cxs::encode_into);
    // check_encode_into_static(val, expected, serialize_txs::encode_into);
}

pub fn check_encode_into_static<T, E>(value: &T, expected: &[u8], encode_into: E)
where
    T: WriteXDR,
    E: Fn(&T, &mut [MU<u8>]) -> WriteResult<usize>,
{
    const BUFSZ: usize = 1000;
    assert!(expected.len() <= BUFSZ);

    // An "uninitialized" buffer that has actually been zeroed
    let mut buf: [MU<u8>; BUFSZ] = [const { MU::zeroed() }; BUFSZ];

    let written = encode_into(value, &mut buf).unwrap();

    let encoded = unsafe {
        (&buf[..written]).assume_init_ref()
    };

    check_eq(encoded, expected);
}

#[cfg(feature = "alloc")]
pub fn check_encode_alloc<T>(value: &T, expected: &[u8])
where
    T: WriteXDR,
{
    let encoded = serialize_cxs::encode(value).unwrap();
    check_eq(&encoded, expected);

    // let encoded = serialize_txs::encode(value);
    // check_eq(&encoded, expected);
}

#[cfg(feature = "alloc")]
pub fn check_encode_into_alloc<T>(value: &T, expected: &[u8])
where
    T: WriteXDR,
{
    let encoded = encode_into_alloc(value, expected.len(), serialize_cxs::encode_into);
    check_eq(&encoded, expected);

    // let encoded = encode_into_alloc(value, expected.len(), serialize_txs::encode_into);
    // check_eq(&encoded, expected);
}

#[cfg(feature = "alloc")]
// Allocate a buffer of the expected size and encode
// the value into it
pub fn encode_into_alloc<T, E>(value: &T, expected_len: usize, encode_into: E) -> Vec<u8>
where
    T: WriteXDR,
    E: Fn(&T, &mut [MU<u8>]) -> WriteResult<usize>,
{
    let mut encoded = Vec::with_capacity(expected_len);
    let written = encode_into(value, encoded.spare_capacity_mut()).unwrap();

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
