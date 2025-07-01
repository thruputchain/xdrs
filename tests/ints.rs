#![feature(maybe_uninit_array_assume_init)]

mod common;
use common::*;

macro_rules! enc_test_int {
    { $([$name:ident, $ty:ty, $val:expr, $expected_hex:expr]),* $(,)? } => { $(

        #[test]
        fn $name() {
            let val: $ty = $val;
            let expected = &hex::decode($expected_hex).unwrap();

            check_codec(&val, expected);
        }

    )* };
}

enc_test_int! {
    [i8_min, i8, i8::MIN, "3f80"],
    [i8_neg_33, i8, -33, "3fdf"],
    [i8_neg_32, i8, -32, "3fe0"],
    [i8_neg_31, i8, -31, "20"],
    [i8_neg_two, i8, -2, "3d"],
    [i8_neg_one, i8, -1, "3e"],
    [i8_zero, i8, 0, "00"],
    [i8_one, i8, 1, "01"],
    [i8_two, i8, 2, "02"],
    [i8_31, i8, 31, "1f"],
    [i8_32, i8, 32, "3f20"],
    [i8_33, i8, 33, "3f21"],
    [i8_max, i8, i8::MAX, "3f7f"],

    [u16_min, u16, u16::MAX, "7fffff"],
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
    [u64_min, u64, u64::MIN, "00"],

    [i64_min, i64, i64::MIN, "ff0000000000000080"],
    [i64_zero, i64, 0, "00"],
    [i64_max, i64, i64::MAX, "ffffffffffffffff7f"],
}
