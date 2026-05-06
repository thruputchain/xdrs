mod common;
use common::*;

macro_rules! enc_test_int {
    { $fmt:ident $([$name:ident, $ty:ty, $val:expr, $expected_hex:expr]),* $(,)? } => { $(

        #[test]
        fn $name() {
            let val: $ty = $val;
            let expected = &hex::decode($expected_hex).unwrap();

            $fmt::check_codec(&val, expected);
        }

    )* };
}

mod txs {
    use super::*;

    enc_test_int! { TXS
        [u16_zero, u16, 0, "00000000"],
        [u16_one, u16, 1, "00000001"],
        [u16_max, u16, u16::MAX, "0000ffff"],

        [i16_min, i16, i16::MIN, "ffff8000"],
        [i16_one, i16, 1, "00000001"],
        [i16_zero, i16, 0, "00000000"],
        [i16_max, i16, i16::MAX, "00007fff"],

        [u32_zero, u32, 0, "00000000"],
        [u32_one, u32, 1, "00000001"],
        [u32_max, u32, u32::MAX, "ffffffff"],

        [i32_min, i32, i32::MIN, "80000000"],
        [i32_max, i32, i32::MAX, "7fffffff"],
        [i32_neg_one, i32, -1, "ffffffff"],
        [i32_zero, i32, 0, "00000000"],
        [i32_one, i32, 1, "00000001"],

        [u64_max, u64, u64::MAX, "ffffffffffffffff"],
        [u64_db, u64, 0xdead1234beef5678, "dead1234beef5678"],
        [u64_min, u64, u64::MIN, "0000000000000000"],

        [i64_min, i64, i64::MIN, "8000000000000000"],
        [i64_zero, i64, 0, "0000000000000000"],
        [i64_max, i64, i64::MAX, "7fffffffffffffff"],
    }

    #[cfg(feature = "int128")]
    enc_test_int! { TXS
        [u128_min, u128, u128::MIN, "00000000000000000000000000000000"],
        [u128_mid, u128, 0x03dead1234beef5678, "0000000000000003dead1234beef5678"],
        [u128_max, u128, u128::MAX, "ffffffffffffffffffffffffffffffff"],
        [i128_min, i128, i128::MIN, "80000000000000000000000000000000"],
        [i128_pos, i128, 0x03dead1234beef5678, "0000000000000003dead1234beef5678"],
        [i128_max, i128, i128::MAX, "7fffffffffffffffffffffffffffffff"],
    }

    #[test]
    fn f32_one() {
        let val: f32 = 1.0;
        let expected = &hex::decode("3f800000").unwrap();
        TXS::check_encode(&val, expected);
        let decoded: f32 = TXS::decode(expected).unwrap();
        assert_eq!(decoded.to_bits(), val.to_bits());
    }

    #[test]
    fn f64_one() {
        let val: f64 = 1.0;
        let expected = &hex::decode("3ff0000000000000").unwrap();
        TXS::check_encode(&val, expected);
        let decoded: f64 = TXS::decode(expected).unwrap();
        assert_eq!(decoded.to_bits(), val.to_bits());
    }

    #[test]
    fn f32_zero() {
        let val: f32 = 0.0;
        let expected = &hex::decode("00000000").unwrap();
        TXS::check_encode(&val, expected);
        let decoded: f32 = TXS::decode(expected).unwrap();
        assert_eq!(decoded.to_bits(), val.to_bits());
    }

    #[test]
    fn f32_neg_zero() {
        let val: f32 = -0.0;
        let expected = &hex::decode("80000000").unwrap();
        TXS::check_encode(&val, expected);
        let decoded: f32 = TXS::decode(expected).unwrap();
        assert_eq!(decoded.to_bits(), val.to_bits());
    }

    #[test]
    fn f32_infinity() {
        let val: f32 = f32::INFINITY;
        let expected = &hex::decode("7f800000").unwrap();
        TXS::check_encode(&val, expected);
        let decoded: f32 = TXS::decode(expected).unwrap();
        assert_eq!(decoded.to_bits(), val.to_bits());
    }

    #[test]
    fn f32_neg_infinity() {
        let val: f32 = f32::NEG_INFINITY;
        let expected = &hex::decode("ff800000").unwrap();
        TXS::check_encode(&val, expected);
        let decoded: f32 = TXS::decode(expected).unwrap();
        assert_eq!(decoded.to_bits(), val.to_bits());
    }

    #[test]
    fn f64_zero() {
        let val: f64 = 0.0;
        let expected = &hex::decode("0000000000000000").unwrap();
        TXS::check_encode(&val, expected);
        let decoded: f64 = TXS::decode(expected).unwrap();
        assert_eq!(decoded.to_bits(), val.to_bits());
    }

    #[test]
    fn f64_neg_zero() {
        let val: f64 = -0.0;
        let expected = &hex::decode("8000000000000000").unwrap();
        TXS::check_encode(&val, expected);
        let decoded: f64 = TXS::decode(expected).unwrap();
        assert_eq!(decoded.to_bits(), val.to_bits());
    }

    #[test]
    fn f64_infinity() {
        let val: f64 = f64::INFINITY;
        let expected = &hex::decode("7ff0000000000000").unwrap();
        TXS::check_encode(&val, expected);
        let decoded: f64 = TXS::decode(expected).unwrap();
        assert_eq!(decoded.to_bits(), val.to_bits());
    }

    #[test]
    fn f64_neg_infinity() {
        let val: f64 = f64::NEG_INFINITY;
        let expected = &hex::decode("fff0000000000000").unwrap();
        TXS::check_encode(&val, expected);
        let decoded: f64 = TXS::decode(expected).unwrap();
        assert_eq!(decoded.to_bits(), val.to_bits());
    }
}

mod cxs {
    use super::*;

    // Tests for non-minimal encoding handling
    mod non_minimal {
        use xdrs::cxs;
        use xdrs::serialize::reader::ReadError;

        // Helper to decode and check for NotMinimalEncoding error
        #[cfg_attr(feature = "permissive", allow(dead_code))]
        fn expect_non_minimal_error<T: for<'r> xdrs::ReadXDR<'r> + std::fmt::Debug>(
            bytes: &[u8],
            type_name: &str,
        ) {
            let result: Result<T, _> = cxs::decode(bytes);
            match result {
                Err(ReadError::NotMinimalEncoding) => {} // expected
                Err(e) => panic!("{type_name}: expected NotMinimalEncoding error, got: {e:?}"),
                Ok(v) => panic!("{type_name}: expected NotMinimalEncoding error, got value: {v:?}"),
            }
        }

        // Helper to decode and verify successful decoding with expected value
        #[cfg_attr(not(feature = "permissive"), allow(dead_code))]
        fn expect_decode_ok<T>(bytes: &[u8], expected: T, type_name: &str)
        where
            T: for<'r> xdrs::ReadXDR<'r> + std::fmt::Debug + PartialEq,
        {
            let result: Result<T, _> = cxs::decode(bytes);
            match result {
                Ok(v) => assert_eq!(v, expected, "{type_name}: decoded value mismatch"),
                Err(e) => panic!(
                    "{type_name}: expected successful decode of {expected:?}, got error: {e:?}"
                ),
            }
        }

        // =====================================================================
        // Tests that run WITHOUT the permissive feature (strict mode)
        // These verify that non-minimal encodings are REJECTED
        // =====================================================================
        #[cfg(not(feature = "permissive"))]
        mod strict {
            use super::*;

            // --- Unsigned non-minimal encodings ---

            #[test]
            fn u32_zero_with_1byte_ext_rejected() {
                // Value 0 should be encoded as just 0x00 (fits in header)
                // Non-minimal: 0x3f 0x00 (header says 1-byte extension, byte is 0)
                let bytes = &[0x3f, 0x00];
                expect_non_minimal_error::<u32>(bytes, "u32");
            }

            #[test]
            fn u32_62_with_1byte_ext_rejected() {
                // Value 62 should be encoded as just 0x3e (fits in header)
                // Non-minimal: 0x3f 0x3e (header says 1-byte extension)
                let bytes = &[0x3f, 0x3e];
                expect_non_minimal_error::<u32>(bytes, "u32");
            }

            #[test]
            fn u32_255_with_2byte_ext_rejected() {
                // Value 255 fits in 1-byte extension: 0x3f 0xff
                // Non-minimal: 0x7f 0xff 0x00 (2-byte extension, little-endian)
                let bytes = &[0x7f, 0xff, 0x00];
                expect_non_minimal_error::<u32>(bytes, "u32");
            }

            #[test]
            fn u32_100_with_4byte_ext_rejected() {
                // Value 100 fits in 1-byte extension: 0x3f 0x64
                // Non-minimal: 0xbf 0x64 0x00 0x00 0x00 (4-byte extension)
                let bytes = &[0xbf, 0x64, 0x00, 0x00, 0x00];
                expect_non_minimal_error::<u32>(bytes, "u32");
            }

            #[test]
            fn u64_100_with_8byte_ext_rejected() {
                // Value 100 fits in 1-byte extension
                // Non-minimal: 0xff + 8 bytes (8-byte extension)
                let bytes = &[0xff, 0x64, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
                expect_non_minimal_error::<u64>(bytes, "u64");
            }

            #[test]
            fn u16_zero_with_1byte_ext_rejected() {
                let bytes = &[0x3f, 0x00];
                expect_non_minimal_error::<u16>(bytes, "u16");
            }

            // --- Signed non-minimal encodings ---

            #[test]
            fn i32_zero_with_1byte_ext_rejected() {
                // Value 0 should be encoded as just 0x00 (fits in header)
                // Non-minimal: 0x3f 0x00 (1-byte extension)
                let bytes = &[0x3f, 0x00];
                expect_non_minimal_error::<i32>(bytes, "i32");
            }

            #[test]
            fn i32_31_with_1byte_ext_rejected() {
                // Value 31 should be encoded as 0x1f (fits in header, IH_MAX)
                // Non-minimal: 0x3f 0x1f (1-byte extension)
                let bytes = &[0x3f, 0x1f];
                expect_non_minimal_error::<i32>(bytes, "i32");
            }

            #[test]
            fn i32_neg31_with_1byte_ext_rejected() {
                // Value -31 should be encoded in header (IH_MIN)
                // Non-minimal: 0x3f 0xe1 (1-byte extension, -31 as signed byte)
                let bytes = &[0x3f, 0xe1];
                expect_non_minimal_error::<i32>(bytes, "i32");
            }

            #[test]
            fn i32_127_with_2byte_ext_rejected() {
                // Value 127 fits in 1-byte extension: 0x3f 0x7f
                // Non-minimal: 0x7f 0x7f 0x00 (2-byte extension, little-endian)
                let bytes = &[0x7f, 0x7f, 0x00];
                expect_non_minimal_error::<i32>(bytes, "i32");
            }

            #[test]
            fn i32_neg128_with_2byte_ext_rejected() {
                // Value -128 fits in 1-byte extension: 0x3f 0x80
                // Non-minimal: 0x7f 0x80 0xff (2-byte extension, little-endian sign-extended)
                let bytes = &[0x7f, 0x80, 0xff];
                expect_non_minimal_error::<i32>(bytes, "i32");
            }

            #[test]
            fn i64_zero_with_8byte_ext_rejected() {
                // Value 0 fits in header
                // Non-minimal: 0xff + 8 zero bytes
                let bytes = &[0xff, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
                expect_non_minimal_error::<i64>(bytes, "i64");
            }

            #[test]
            fn i16_neg1_with_1byte_ext_rejected() {
                // Value -1 should be encoded as 0x3e (fits in header)
                // Non-minimal: 0x3f 0xff (1-byte extension, -1 as signed byte)
                let bytes = &[0x3f, 0xff];
                expect_non_minimal_error::<i16>(bytes, "i16");
            }
        }

        // =====================================================================
        // Tests that run WITH the permissive feature
        // These verify that non-minimal encodings are ACCEPTED and decode correctly
        // =====================================================================
        #[cfg(feature = "permissive")]
        mod permissive {
            use super::*;

            // --- Unsigned non-minimal encodings ---

            #[test]
            fn u32_zero_with_1byte_ext_accepted() {
                let bytes = &[0x3f, 0x00];
                expect_decode_ok::<u32>(bytes, 0, "u32");
            }

            #[test]
            fn u32_62_with_1byte_ext_accepted() {
                let bytes = &[0x3f, 0x3e];
                expect_decode_ok::<u32>(bytes, 62, "u32");
            }

            #[test]
            fn u32_255_with_2byte_ext_accepted() {
                let bytes = &[0x7f, 0xff, 0x00];
                expect_decode_ok::<u32>(bytes, 255, "u32");
            }

            #[test]
            fn u32_100_with_4byte_ext_accepted() {
                let bytes = &[0xbf, 0x64, 0x00, 0x00, 0x00];
                expect_decode_ok::<u32>(bytes, 100, "u32");
            }

            #[test]
            fn u64_100_with_8byte_ext_accepted() {
                let bytes = &[0xff, 0x64, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
                expect_decode_ok::<u64>(bytes, 100, "u64");
            }

            #[test]
            fn u16_zero_with_1byte_ext_accepted() {
                let bytes = &[0x3f, 0x00];
                expect_decode_ok::<u16>(bytes, 0, "u16");
            }

            // --- Signed non-minimal encodings ---

            #[test]
            fn i32_zero_with_1byte_ext_accepted() {
                let bytes = &[0x3f, 0x00];
                expect_decode_ok::<i32>(bytes, 0, "i32");
            }

            #[test]
            fn i32_31_with_1byte_ext_accepted() {
                let bytes = &[0x3f, 0x1f];
                expect_decode_ok::<i32>(bytes, 31, "i32");
            }

            #[test]
            fn i32_neg31_with_1byte_ext_accepted() {
                let bytes = &[0x3f, 0xe1];
                expect_decode_ok::<i32>(bytes, -31, "i32");
            }

            #[test]
            fn i32_127_with_2byte_ext_accepted() {
                let bytes = &[0x7f, 0x7f, 0x00];
                expect_decode_ok::<i32>(bytes, 127, "i32");
            }

            #[test]
            fn i32_neg128_with_2byte_ext_accepted() {
                let bytes = &[0x7f, 0x80, 0xff];
                expect_decode_ok::<i32>(bytes, -128, "i32");
            }

            #[test]
            fn i64_zero_with_8byte_ext_accepted() {
                let bytes = &[0xff, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
                expect_decode_ok::<i64>(bytes, 0, "i64");
            }

            #[test]
            fn i16_neg1_with_1byte_ext_accepted() {
                let bytes = &[0x3f, 0xff];
                expect_decode_ok::<i16>(bytes, -1, "i16");
            }
        }
    }

    enc_test_int! { CXS
        [u16_zero, u16, 0, "00"],
        [u16_max, u16, u16::MAX, "7fffff"],

        [i16_neg32, i16, -32, "3fe0"],
        [i16_neg31, i16, -31, "20"],
        [i16_neg_one, i16, -1, "3e"],
        [i16_one, i16, 1, "01"],
        [i16_zero, i16, 0, "00"],
        [i16_31, i16, 31, "1f"],
        [i16_32, i16, 32, "3f20"],

        [u32_zero, u32, 0, "00"],
        [u32_one, u32, 1, "01"],
        [u32_max, u32, u32::MAX, "bfffffffff"],
        [u32_db, u32, 4022250974, "bfdeadbeef"],

        [i32_min, i32, i32::MIN, "bf00000080"],
        [i32_neg32, i32, -32, "3fe0"],
        [i32_neg31, i32, -31, "20"],
        [i32_neg_two, i32, -2, "3d"],
        [i32_neg_one, i32, -1, "3e"],
        [i32_zero, i32, 0, "00"],
        [i32_one, i32, 1, "01"],
        [i32_31, i32, 31, "1f"],
        [i32_32, i32, 32, "3f20"],
        [i32_db, i32, 15711917, "bfadbeef00"],
        [i32_max, i32, i32::MAX, "bfffffff7f"],

        [u64_max, u64, u64::MAX, "ffffffffffffffffff"],
        [u64_db, u64, 0xdead1234beef5678, "ff7856efbe3412adde"],
        [u64_min, u64, u64::MIN, "00"],

        [i64_min, i64, i64::MIN, "ff0000000000000080"],
        [i64_zero, i64, 0, "00"],
        [i64_max, i64, i64::MAX, "ffffffffffffffff7f"],
    }

    #[cfg(feature = "int128")]
    enc_test_int! { CXS
        [u128_min, u128, u128::MIN, "0000"],
        [u128_mid, u128, 0x03dead1234beef5678, "03ff7856efbe3412adde"],
        [u128_max, u128, u128::MAX, "ffffffffffffffffffffffffffffffffffff"],
        [i128_min, i128, i128::MIN, "ff000000000000008000"],
        [i128_pos, i128, 0x03dead1234beef5678, "03ff7856efbe3412adde"],
        [i128_max, i128, i128::MAX, "ffffffffffffffff7fffffffffffffffffff"],
    }

    #[test]
    fn f32_one() {
        let val: f32 = 1.0;
        let expected = &hex::decode("0000803f").unwrap();
        CXS::check_encode(&val, expected);
        let decoded: f32 = CXS::decode(expected).unwrap();
        assert_eq!(decoded.to_bits(), val.to_bits());
    }

    #[test]
    fn f64_one() {
        let val: f64 = 1.0;
        let expected = &hex::decode("000000000000f03f").unwrap();
        CXS::check_encode(&val, expected);
        let decoded: f64 = CXS::decode(expected).unwrap();
        assert_eq!(decoded.to_bits(), val.to_bits());
    }

    #[test]
    fn f32_zero() {
        let val: f32 = 0.0;
        let expected = &hex::decode("00000000").unwrap();
        CXS::check_encode(&val, expected);
        let decoded: f32 = CXS::decode(expected).unwrap();
        assert_eq!(decoded.to_bits(), val.to_bits());
    }

    #[test]
    fn f32_neg_zero() {
        let val: f32 = -0.0;
        let expected = &hex::decode("00000080").unwrap();
        CXS::check_encode(&val, expected);
        let decoded: f32 = CXS::decode(expected).unwrap();
        assert_eq!(decoded.to_bits(), val.to_bits());
    }

    #[test]
    fn f32_infinity() {
        let val: f32 = f32::INFINITY;
        let expected = &hex::decode("0000807f").unwrap();
        CXS::check_encode(&val, expected);
        let decoded: f32 = CXS::decode(expected).unwrap();
        assert_eq!(decoded.to_bits(), val.to_bits());
    }

    #[test]
    fn f64_zero() {
        let val: f64 = 0.0;
        let expected = &hex::decode("0000000000000000").unwrap();
        CXS::check_encode(&val, expected);
        let decoded: f64 = CXS::decode(expected).unwrap();
        assert_eq!(decoded.to_bits(), val.to_bits());
    }

    #[test]
    fn f64_neg_zero() {
        let val: f64 = -0.0;
        let expected = &hex::decode("0000000000000080").unwrap();
        CXS::check_encode(&val, expected);
        let decoded: f64 = CXS::decode(expected).unwrap();
        assert_eq!(decoded.to_bits(), val.to_bits());
    }

    #[test]
    fn f64_infinity() {
        let val: f64 = f64::INFINITY;
        let expected = &hex::decode("000000000000f07f").unwrap();
        CXS::check_encode(&val, expected);
        let decoded: f64 = CXS::decode(expected).unwrap();
        assert_eq!(decoded.to_bits(), val.to_bits());
    }
}
