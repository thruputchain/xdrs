mod common;
use common::*;

fn cxs_runtime_encode<T: xdrs::WriteXDR>(val: &T) -> Vec<u8> {
    xdrs::cxs::encode(val).unwrap()
}

fn txs_runtime_encode<T: xdrs::WriteXDR>(val: &T) -> Vec<u8> {
    xdrs::txs::encode(val).unwrap()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, xdrs::ConstWriteXDR, xdrs::WriteXDR)]
struct Leaf {
    code: u32,
}

#[derive(Debug, PartialEq, Eq, xdrs::ConstWriteXDR, xdrs::WriteXDR)]
struct BorrowedLeafSlice<'a> {
    items: &'a [Leaf],
}

const LEAF_ITEMS: [Leaf; 2] = [Leaf { code: 7 }, Leaf { code: 42 }];
const BORROWED: BorrowedLeafSlice<'static> = BorrowedLeafSlice { items: &LEAF_ITEMS };

const BORROWED_CXS: [u8; BORROWED.const_cxs_size()] = {
    let mut buf = [0u8; BORROWED.const_cxs_size()];
    let mut w = xdrs::cxs::Cursor::new(&mut buf);
    BORROWED.const_write_cxs(&mut w);
    assert!(w.pos() == BORROWED.const_cxs_size());
    buf
};

const BORROWED_TXS: [u8; BORROWED.const_txs_size()] = {
    let mut buf = [0u8; BORROWED.const_txs_size()];
    let mut w = xdrs::txs::Cursor::new(&mut buf);
    BORROWED.const_write_txs(&mut w);
    assert!(w.pos() == BORROWED.const_txs_size());
    buf
};

#[test]
fn derived_borrowed_leaf_slice_encodes() {
    let val = BorrowedLeafSlice { items: &LEAF_ITEMS };

    CXS::check_encode(&val, &hex!("02072a"));
    TXS::check_encode(&val, &hex!("00000002000000070000002a"));
}

#[test]
fn borrowed_leaf_slice_const_matches_runtime() {
    assert_eq!(&BORROWED_CXS, cxs_runtime_encode(&BORROWED).as_slice());
    assert_eq!(&BORROWED_TXS, txs_runtime_encode(&BORROWED).as_slice());
}
