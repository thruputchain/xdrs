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

enc_test! {
    [i32_one, i32, 1, "01000000"],
    [i32_zero, i32, 0, "00000000"],
    [i32_neg_one, i32, -1, "ffffffff"],
}

fn do_encode_into<T>(value: &T, encoded_len: usize) -> Vec<u8>
where T: cxs::Deconstruct
{
    let mut encoded = Vec::with_capacity(encoded_len);
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
