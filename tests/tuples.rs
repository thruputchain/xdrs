#![feature(maybe_uninit_array_assume_init)]
#![feature(maybe_uninit_slice)]

mod common;
use common::*;

#[test]
fn unit() {
    let x = ();

    CXS::check_codec(&x, &hex!(""));
    TXS::check_codec(&x, &hex!(""));
}

#[test]
fn tuples() {
    let n: u32 = 7;
    let arr: [u8; 3] = [0, 1, 2];

    let tup = (&arr, n);
    CXS::check_codec(&tup, &hex!("00010207"));
    TXS::check_codec(&tup, &hex!("0001020000000007"));

    let tup = (&arr, n, &arr);
    CXS::check_codec(&tup, &hex!("00010207000102"));
    TXS::check_codec(&tup, &hex!("000102000000000700010200"));

    let tup = (&arr, n, (&arr, n));
    CXS::check_codec(&tup, &hex!("0001020700010207"));
    TXS::check_codec(&tup, &hex!("00010200000000070001020000000007"));
}
