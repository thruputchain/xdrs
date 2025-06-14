use cxs::{encode, decode};

macro_rules! enc_test {
    { $([$name:ident, $ty:ty, $val:expr, $expected_hex:expr]),* } => {
        $(

        #[test]
        fn $name() {
            let expected_encoding = hex::decode($expected_hex).unwrap();
            let val: $ty = $val;

            let encoded = encode(&val);
            assert_eq!(encoded, expected_encoding);

            // Decode value from buffer and check equal to orig
            let mut val2: $ty = Default::default();
            decode(&encoded, &mut val2);
            assert_eq!(val2, val);
        }

        )*
    };
}

enc_test! {
    [i32, i32, 1, "01000000"]
}
