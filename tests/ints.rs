macro_rules! enc_test_int {
    { $([$name:ident, $ty:ty, $val:expr]),* $(,)? } => {
        $(

        #[test]
        fn $name() {
            let val: $ty = $val;
            let expected_arr = val.to_le_bytes();
            let expected = &expected_arr[..];

            #[cfg(feature = "alloc")]
            {
                let expected_hex = hex::encode(expected);
                let encoded = cxs::encode(&val);
                let encoded_hex = hex::encode(&encoded);
                assert_eq!(&encoded_hex, &expected_hex);
            }

            let encoded = do_encode_into(&val, 1000);
            assert_eq!(encoded.as_slice(), expected);

            let val2: $ty = cxs::decode(&encoded);
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
