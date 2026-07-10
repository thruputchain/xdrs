//! Tests const writers for `VecM`/`BytesM`/`StringM` with non-empty data.
//!
//! Non-empty containers can only be constructed in const contexts in
//! no-`alloc` builds, where they wrap `&'static [T]`. This file compiles to
//! nothing when the `alloc` feature is enabled; the empty-container const
//! cases (and runtime size checks) live in tests/const_writer.rs.
#![cfg(not(feature = "alloc"))]

use core::mem::MaybeUninit;
use xdrs::{BytesM, StringM, VecM};

/// Runtime-encode `val` with TXS into a fixed buffer, returning the bytes.
fn txs_runtime_encode<'a, T: xdrs::WriteXDR>(val: &T, buf: &'a mut [MaybeUninit<u8>]) -> &'a [u8] {
    let n = xdrs::txs::encode_into(val, buf).unwrap();
    // SAFETY: encode_into initialized the first `n` bytes.
    unsafe { xdrs::mu::assume_init_ref(&buf[..n]) }
}

/// Runtime-encode `val` with CXS into a fixed buffer, returning the bytes.
fn cxs_runtime_encode<'a, T: xdrs::WriteXDR>(val: &T, buf: &'a mut [MaybeUninit<u8>]) -> &'a [u8] {
    let n = xdrs::cxs::encode_into(val, buf).unwrap();
    // SAFETY: encode_into initialized the first `n` bytes.
    unsafe { xdrs::mu::assume_init_ref(&buf[..n]) }
}

#[derive(xdrs::ConstWriteXDR, xdrs::WriteXDR)]
struct Containers {
    nums: VecM<u32, 5>,
    raw: VecM<u8, 4>,
    bytes: BytesM<4>,
    name: StringM<8>,
    nested: VecM<VecM<u32, 2>, 3>,
    maybe: Option<BytesM<4>>,
}

const VAL: Containers = Containers {
    nums: VecM(&[1, 2, 70000]),
    raw: VecM(b"hi"),
    bytes: BytesM(&[0xAA, 0xBB, 0xCC]),
    name: StringM(b"xdr"),
    nested: VecM(&[VecM(&[7, 8]), VecM(&[9])]),
    maybe: Some(BytesM(&[0xDD])),
};

const VAL_TXS: [u8; VAL.const_txs_size()] = {
    let mut w = xdrs::txs::ConstWriter::<{ VAL.const_txs_size() }>::new();
    VAL.const_write_txs(&mut w);
    w.into_bytes()
};

const VAL_CXS: [u8; VAL.const_cxs_size()] = {
    let mut w = xdrs::cxs::ConstWriter::<{ VAL.const_cxs_size() }>::new();
    VAL.const_write_cxs(&mut w);
    w.into_bytes()
};

#[test]
fn container_nonempty_const_matches_runtime() {
    let mut buf = [MaybeUninit::uninit(); 256];
    assert_eq!(&VAL_TXS, txs_runtime_encode(&VAL, &mut buf));
    let mut buf = [MaybeUninit::uninit(); 256];
    assert_eq!(&VAL_CXS, cxs_runtime_encode(&VAL, &mut buf));
}

#[test]
fn container_nonempty_txs_expected_bytes() {
    #[rustfmt::skip]
    let expected: &[u8] = &[
        // nums: len 3, then 1, 2, 70000
        0, 0, 0, 3, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0x01, 0x11, 0x70,
        // raw: len 2, "hi", pad
        0, 0, 0, 2, 0x68, 0x69, 0, 0,
        // bytes: len 3, AA BB CC, pad
        0, 0, 0, 3, 0xAA, 0xBB, 0xCC, 0,
        // name: len 3, "xdr", pad
        0, 0, 0, 3, 0x78, 0x64, 0x72, 0,
        // nested: len 2, [len 2, 7, 8], [len 1, 9]
        0, 0, 0, 2,
        0, 0, 0, 2, 0, 0, 0, 7, 0, 0, 0, 8,
        0, 0, 0, 1, 0, 0, 0, 9,
        // maybe: present, then len 1, DD, pad
        0, 0, 0, 1,
        0, 0, 0, 1, 0xDD, 0, 0, 0,
    ];
    assert_eq!(&VAL_TXS, expected);
}

// A const-generic MAX bound flows through to the generated length guard.
#[derive(xdrs::ConstWriteXDR, xdrs::WriteXDR)]
struct GenericMax<const M: u32> {
    data: VecM<u32, M>,
    blob: BytesM<M>,
}

const GENERIC_VAL: GenericMax<4> = GenericMax {
    data: VecM(&[5, 6]),
    blob: BytesM(&[0xEE]),
};

const GENERIC_TXS: [u8; GENERIC_VAL.const_txs_size()] = {
    let mut w = xdrs::txs::ConstWriter::<{ GENERIC_VAL.const_txs_size() }>::new();
    GENERIC_VAL.const_write_txs(&mut w);
    w.into_bytes()
};

#[test]
fn generic_max_const_matches_runtime() {
    let mut buf = [MaybeUninit::uninit(); 64];
    assert_eq!(&GENERIC_TXS, txs_runtime_encode(&GENERIC_VAL, &mut buf));
}

#[derive(xdrs::ConstWriteXDR, xdrs::WriteXDR)]
#[repr(i32)]
#[allow(dead_code)]
enum ContainerEnum {
    Nums(VecM<u32, 5>) = 0,
    Bytes(BytesM<4>) = 1,
}

const ENUM_VAL: ContainerEnum = ContainerEnum::Nums(VecM(&[10, 20]));

const ENUM_CXS: [u8; ENUM_VAL.const_cxs_size()] = {
    let mut w = xdrs::cxs::ConstWriter::<{ ENUM_VAL.const_cxs_size() }>::new();
    ENUM_VAL.const_write_cxs(&mut w);
    w.into_bytes()
};

const ENUM_TXS: [u8; ENUM_VAL.const_txs_size()] = {
    let mut w = xdrs::txs::ConstWriter::<{ ENUM_VAL.const_txs_size() }>::new();
    ENUM_VAL.const_write_txs(&mut w);
    w.into_bytes()
};

#[test]
fn container_enum_const_matches_runtime() {
    let mut buf = [MaybeUninit::uninit(); 64];
    assert_eq!(&ENUM_TXS, txs_runtime_encode(&ENUM_VAL, &mut buf));
    let mut buf = [MaybeUninit::uninit(); 64];
    assert_eq!(&ENUM_CXS, cxs_runtime_encode(&ENUM_VAL, &mut buf));
}
