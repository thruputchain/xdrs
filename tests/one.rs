macro_rules! enc_test {
    { $([$name:ident, $ty:ty, $val:expr, $expected_hex:expr]),* $(,)? } => {
        $(

        #[test]
        fn $name() {
            let expected_hex = $expected_hex;
            let val: $ty = $val;

            let encoded = cxs::encode(&val);
            let encoded_hex = hex::encode(&encoded);
            assert_eq!(&encoded_hex, &expected_hex);

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
