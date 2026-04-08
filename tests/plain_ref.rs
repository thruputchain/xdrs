// tests/plain_ref.rs
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

const LEAF_A: Leaf = Leaf { code: 7 };
const LEAF_B: Leaf = Leaf { code: 42 };

// --- Plain &T in a struct ---

#[derive(Debug, PartialEq, Eq, xdrs::ConstWriteXDR, xdrs::WriteXDR)]
struct BorrowedLeaf<'a> {
    tag: u32,
    inner: &'a Leaf,
}

const BORROWED_LEAF: BorrowedLeaf<'static> = BorrowedLeaf {
    tag: 1,
    inner: &LEAF_A,
};

const BORROWED_LEAF_CXS: [u8; BORROWED_LEAF.const_cxs_size()] = {
    let mut w = xdrs::cxs::ConstWriter::<{ BORROWED_LEAF.const_cxs_size() }>::new();
    BORROWED_LEAF.const_write_cxs(&mut w);
    w.into_bytes()
};

const BORROWED_LEAF_TXS: [u8; BORROWED_LEAF.const_txs_size()] = {
    let mut w = xdrs::txs::ConstWriter::<{ BORROWED_LEAF.const_txs_size() }>::new();
    BORROWED_LEAF.const_write_txs(&mut w);
    w.into_bytes()
};

#[test]
fn struct_with_plain_ref_const_matches_runtime() {
    assert_eq!(
        &BORROWED_LEAF_CXS,
        cxs_runtime_encode(&BORROWED_LEAF).as_slice()
    );
    assert_eq!(
        &BORROWED_LEAF_TXS,
        txs_runtime_encode(&BORROWED_LEAF).as_slice()
    );
}

// --- Enum with &T variant fields (mirrors sdk_rs ContractTypeView) ---

#[derive(Debug, PartialEq, Eq, xdrs::ConstWriteXDR, xdrs::WriteXDR)]
#[repr(i32)]
enum TypeRef<'a> {
    None = 0,
    Single(&'a Leaf) = 1,
    Pair(&'a Leaf, &'a Leaf) = 2,
}

const TYPE_REF_SINGLE: TypeRef<'static> = TypeRef::Single(&LEAF_A);
const TYPE_REF_PAIR: TypeRef<'static> = TypeRef::Pair(&LEAF_A, &LEAF_B);

const TYPE_REF_SINGLE_CXS: [u8; TYPE_REF_SINGLE.const_cxs_size()] = {
    let mut w = xdrs::cxs::ConstWriter::<{ TYPE_REF_SINGLE.const_cxs_size() }>::new();
    TYPE_REF_SINGLE.const_write_cxs(&mut w);
    w.into_bytes()
};

const TYPE_REF_SINGLE_TXS: [u8; TYPE_REF_SINGLE.const_txs_size()] = {
    let mut w = xdrs::txs::ConstWriter::<{ TYPE_REF_SINGLE.const_txs_size() }>::new();
    TYPE_REF_SINGLE.const_write_txs(&mut w);
    w.into_bytes()
};

const TYPE_REF_PAIR_CXS: [u8; TYPE_REF_PAIR.const_cxs_size()] = {
    let mut w = xdrs::cxs::ConstWriter::<{ TYPE_REF_PAIR.const_cxs_size() }>::new();
    TYPE_REF_PAIR.const_write_cxs(&mut w);
    w.into_bytes()
};

const TYPE_REF_PAIR_TXS: [u8; TYPE_REF_PAIR.const_txs_size()] = {
    let mut w = xdrs::txs::ConstWriter::<{ TYPE_REF_PAIR.const_txs_size() }>::new();
    TYPE_REF_PAIR.const_write_txs(&mut w);
    w.into_bytes()
};

#[test]
fn enum_with_ref_variants_const_matches_runtime() {
    assert_eq!(
        &TYPE_REF_SINGLE_CXS,
        cxs_runtime_encode(&TYPE_REF_SINGLE).as_slice()
    );
    assert_eq!(
        &TYPE_REF_SINGLE_TXS,
        txs_runtime_encode(&TYPE_REF_SINGLE).as_slice()
    );
    assert_eq!(
        &TYPE_REF_PAIR_CXS,
        cxs_runtime_encode(&TYPE_REF_PAIR).as_slice()
    );
    assert_eq!(
        &TYPE_REF_PAIR_TXS,
        txs_runtime_encode(&TYPE_REF_PAIR).as_slice()
    );
}

// --- Option<&T> (the sdk_rs pattern) ---

#[derive(Debug, PartialEq, Eq, xdrs::ConstWriteXDR, xdrs::WriteXDR)]
struct OptionRef<'a> {
    maybe: Option<&'a Leaf>,
}

const OPT_REF_SOME: OptionRef<'static> = OptionRef {
    maybe: Some(&LEAF_A),
};
const OPT_REF_NONE: OptionRef<'static> = OptionRef { maybe: None };

const OPT_REF_SOME_CXS: [u8; OPT_REF_SOME.const_cxs_size()] = {
    let mut w = xdrs::cxs::ConstWriter::<{ OPT_REF_SOME.const_cxs_size() }>::new();
    OPT_REF_SOME.const_write_cxs(&mut w);
    w.into_bytes()
};

const OPT_REF_SOME_TXS: [u8; OPT_REF_SOME.const_txs_size()] = {
    let mut w = xdrs::txs::ConstWriter::<{ OPT_REF_SOME.const_txs_size() }>::new();
    OPT_REF_SOME.const_write_txs(&mut w);
    w.into_bytes()
};

const OPT_REF_NONE_CXS: [u8; OPT_REF_NONE.const_cxs_size()] = {
    let mut w = xdrs::cxs::ConstWriter::<{ OPT_REF_NONE.const_cxs_size() }>::new();
    OPT_REF_NONE.const_write_cxs(&mut w);
    w.into_bytes()
};

const OPT_REF_NONE_TXS: [u8; OPT_REF_NONE.const_txs_size()] = {
    let mut w = xdrs::txs::ConstWriter::<{ OPT_REF_NONE.const_txs_size() }>::new();
    OPT_REF_NONE.const_write_txs(&mut w);
    w.into_bytes()
};

#[test]
fn option_of_ref_const_matches_runtime() {
    assert_eq!(
        &OPT_REF_SOME_CXS,
        cxs_runtime_encode(&OPT_REF_SOME).as_slice()
    );
    assert_eq!(
        &OPT_REF_SOME_TXS,
        txs_runtime_encode(&OPT_REF_SOME).as_slice()
    );
    assert_eq!(
        &OPT_REF_NONE_CXS,
        cxs_runtime_encode(&OPT_REF_NONE).as_slice()
    );
    assert_eq!(
        &OPT_REF_NONE_TXS,
        txs_runtime_encode(&OPT_REF_NONE).as_slice()
    );
}
