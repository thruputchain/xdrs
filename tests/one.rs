use cxs::codec::provider::*;
use cxs::codec::receiver::*;
use cxs::xdr::construct::Construct;
use cxs::xdr::deconstruct::Deconstruct;

macro_rules! enc_test {
    { $([$name:ident, $ty:ty, $val:expr, $expected_hex:expr]),* } => {
        $(

        #[test]
        fn $name() {
            let expected_encoding = hex::decode($expected_hex).unwrap();
            let val = $val;

            // Encode value and check encoding
            let mut receiver = BufReceiver::new();
            let _acc = val.deconstruct(&mut receiver).unwrap();
            let encoded: &[u8] = receiver.as_ref();
            assert_eq!(encoded, expected_encoding);

            // Decode value from buffer and check equal to orig
            let mut input = BufProvider::new(&expected_encoding);
            let mut x: $ty = Default::default();
            x.construct(&mut input).unwrap();
            assert_eq!(x, $val);

        }

        )*
    };
}

enc_test! {
    [i32, i32, 1, "01000000"]
}
