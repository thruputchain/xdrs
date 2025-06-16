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


// u6   HEX    BIN      i6   +1 = n
// 63   0x3f   11_1111  -1        X
// 62   0x3e   11_1110  -2       -1
// 61   0x3d   11_1101  -3       -2
//
//             10_0011 -61      -60
//             10_0010 -62      -61
//             10_0001 -63      -62
//             10_0000   0
//                        
//  1   0x01   0000_0001  
//  0   0x00

//  s = 2 -> 0xbf

// 00_0001
// 11_1110  invert
// 11_1111  add 1 overflowing

// -1
// 00_0001
// 11_1110
// 11_1111

// -2
// 00_0010
// 11_1101
// 11_1110

// -3
// 00_0011
// 11_1100
// 11_1101

// i32::min
// 0111_1111 ff ff ff
// 1000_0000 00 00 00
// 1000_0000 00 00 01
// 0xbf
enc_test_int! {
    [u32_zero, u32, 0, "00"],
    [u32_one, u32, 1, "01"],
    [u32_max, u32, u32::MAX, "bfffffffff"],
    [u32_db, u32, 4022250974, "bfdeadbeef"],
    [i32_zero, i32, 0, "00"],
    [i32_one, i32, 1, "01"],
    [i32_neg_one, i32, -1, "3e"],
    [i32_neg_two, i32, -2, "3d"],
    [i32_min, i32, i32::MIN, "bf01000080"],
    [i32_max, i32, i32::MAX, "bfffffffff"],
}
