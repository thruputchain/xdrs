#![feature(maybe_uninit_array_assume_init)]
#![feature(maybe_uninit_slice)]

mod common;
use common::*;

#[test]
fn tuples() {
    let n: u32 = 7;
    let arr: [u8; 3] = [0, 1, 2];

    let tup = (&arr, n);
    let expected = &hex!("00010207");
    check_codec(&tup, expected);

    let tup = (&arr, n, &arr);
    let expected = &hex!("00010207000102");
    check_codec(&tup, expected);

    let tup = (&arr, n, (&arr, n));
    let expected = &hex!("0001020700010207");
    check_codec(&tup, expected);
}
