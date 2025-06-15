/*
macro_rules! enc_test {
    { $([$name:ident, $ty:ty, $val:expr, $expected_hex:expr]),* $(,)? } => {
        $(

        #[test]
        fn $name() {
            let expected_hex = $expected_hex;
            let expected = hex::decode(expected_hex).unwrap();
            let val: $ty = $val;

            let encoded = cxs::encode(&val);
            let encoded_hex = hex::encode(&encoded);
            assert_eq!(&encoded_hex, &expected_hex);

            let encoded = do_encode_into(&val, encoded.len());
            assert_eq!(encoded.as_slice(), expected.as_slice());

            let mut val2: $ty = Default::default();
            cxs::decode(&encoded, &mut val2);
            assert_eq!(val2, val);
        }

        )*
    };
}
*/

macro_rules! enc_test_int {
    { $([$name:ident, $ty:ty, $val:expr]),* $(,)? } => {
        $(

        #[test]
        fn $name() {
            let val: $ty = $val;
            let expected_arr = val.to_le_bytes();
            let expected = &expected_arr[..];
            let expected_hex = hex::encode(expected);

            let encoded = cxs::encode(&val);
            let encoded_hex = hex::encode(&encoded);
            assert_eq!(&encoded_hex, &expected_hex);

            let encoded = do_encode_into(&val, encoded.len());
            assert_eq!(encoded.as_slice(), expected);

            let mut val2: $ty = Default::default();
            cxs::decode(&encoded, &mut val2);
            assert_eq!(val2, val);
        }

        )*
    };
}

enc_test_int! {
    [i32_one, i32, 1],
    [i32_zero, i32, 0],
    [i32_neg_one, i32, -1],
    [i32_min, i32, i32::MIN],
    [i32_max, i32, i32::MAX],
    [u32_one, u32, 1],
    [u32_zero, u32, 0],
    [u32_max, u32, u32::MAX],
}


fn do_encode_into<T>(value: &T, encoded_len: usize) -> Vec<u8>
where T: cxs::Deconstruct
{
    let mut encoded = Vec::with_capacity(encoded_len);
    let written = cxs::encode_into(value, encoded.spare_capacity_mut());

    // SAFETY:
    // - the new len must be less than or equal to capacity().
    // - the elements at old_len..new_len must be initialized.
    assert!(written == encoded.capacity());
    unsafe {
        encoded.set_len(written);
    }
    encoded
}
