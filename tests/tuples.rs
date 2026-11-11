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

    let n: u16 = 7;
    let tup = (n, n, n, n, n, n);
    CXS::check_codec(&tup, &hex!("070707070707"));
    TXS::check_codec(
        &tup,
        &hex!("000000070000000700000007000000070000000700000007"),
    );
}

#[test]
fn tuple_unchecked_writes_txs() {
    use core::mem::MaybeUninit;
    use xdrs::WriteXDR;

    let mut buf = [MaybeUninit::uninit(); 8];
    let mut w = xdrs::txs::unchecked_buf_writer::UncheckedBufWriter::new(&mut buf);
    // SAFETY: two u32s occupy exactly the 8-byte buffer.
    unsafe { (1u32, 2u32).write_xdr_unchecked(&mut w) };
    assert_eq!(w.written(), 8);
    // SAFETY: the first 8 bytes were just initialized.
    let bytes = unsafe { buf[..8].assume_init_ref() };
    assert_eq!(bytes, [0, 0, 0, 1, 0, 0, 0, 2]);
}
