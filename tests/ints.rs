#![feature(maybe_uninit_array_assume_init)]

mod common;
use common::*;

macro_rules! enc_test_int {
    { $([$name:ident, $ty:ty, $val:expr, $expected_hex:expr]),* $(,)? } => {
        $(

        #[test]
        fn $name() {
            let val: $ty = $val;
            let expected = &hex::decode($expected_hex).unwrap();

            check_codec(&val, expected);
        }

        )*
    };
}

enc_test_int! {
    [u32_zero, u32, 0, "00"],
    [u32_one, u32, 1, "01"],
    [u32_max, u32, u32::MAX, "bfffffffff"],
    [u32_db, u32, 4022250974, "bfdeadbeef"],
    [i32_zero, i32, 0, "00"],
    [i32_one, i32, 1, "01"],
    [i32_neg_one, i32, -1, "3e"],
    [i32_neg_two, i32, -2, "3d"],
    [i32_neg31, i32, -31, "20"],
    [i32_neg32, i32, -32, "3fe0"],
    [i32_31, i32, 31, "1f"],
    [i32_32, i32, 32, "3f20"],
    [i32_max, i32, i32::MAX, "bfffffff7f"],
    [i32_min, i32, i32::MIN, "bf00000080"],
}
