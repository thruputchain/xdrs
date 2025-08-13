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
        [u16_zero, u16, 0, "0000"],
        [u16_one, u16, 1, "0001"],
        [u16_max, u16, u16::MAX, "ffff"],

        [i16_min, i16, i16::MIN, "8000"],
        [i16_one, i16, 1, "0001"],
        [i16_zero, i16, 0, "0000"],
        [i16_max, i16, i16::MAX, "7fff"],

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

    #[cfg(all(feature = "extras", not(feature = "int128_xdr")))]
    enc_test_int! { TXS
        [u128_min, u128, u128::MIN, "00000000"],
        [u128_d, u128, 0x07dead, "00000003adde0700"],
        [u128_mid, u128, 0xadde07, "0000000307dead00"],
        [u128_max, u128, u128::MAX, "00000010ffffffffffffffffffffffffffffffff"],

        [i128_min, i128, i128::MIN, "0000001000000000000000000000000000000080"],
        [i128_pos, i128, 0xadde07, "0000000307dead00"],
        [i128_max, i128, i128::MAX, "00000010ffffffffffffffffffffffffffffff7f"],
    }

    #[cfg(all(feature = "extras", feature = "int128_xdr"))]
    enc_test_int! { TXS
        [u128_min, u128, u128::MIN, "00000000000000000000000000000000"],
        [u128_mid, u128, 0x03dead1234beef5678, "0000000000000003dead1234beef5678"],
        [u128_max, u128, u128::MAX, "ffffffffffffffffffffffffffffffff"],
        [i128_min, i128, i128::MIN, "80000000000000000000000000000000"],
        [i128_pos, i128, 0x03dead1234beef5678, "0000000000000003dead1234beef5678"],
        [i128_max, i128, i128::MAX, "7fffffffffffffffffffffffffffffff"],
    }
}

mod cxs {
    use super::*;

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

    #[cfg(all(feature = "extras", not(feature = "int128_xdr")))]
    enc_test_int! { CXS
        [u128_min, u128, u128::MIN, "00"],
        [u128_d, u128, 0x07dead, "03adde07"],
        [u128_mid, u128, 0xadde07, "0307dead"],
        [u128_max, u128, u128::MAX, "10ffffffffffffffffffffffffffffffff"],
        [i128_min, i128, i128::MIN, "1000000000000000000000000000000080"],
        [i128_pos, i128, 0xadde07, "0307dead"],
        [i128_max, i128, i128::MAX, "10ffffffffffffffffffffffffffffff7f"],
    }

    #[cfg(all(feature = "extras", feature = "int128_xdr"))]
    enc_test_int! { CXS
        [u128_min, u128, u128::MIN, "0000"],
        [u128_mid, u128, 0x03dead1234beef5678, "03ff7856efbe3412adde"],
        [u128_max, u128, u128::MAX, "ffffffffffffffffffffffffffffffffffff"],
        [i128_min, i128, i128::MIN, "ff000000000000008000"],
        [i128_pos, i128, 0x03dead1234beef5678, "03ff7856efbe3412adde"],
        [i128_max, i128, i128::MAX, "ffffffffffffffff7fffffffffffffffffff"],
    }
}
