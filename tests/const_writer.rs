//! Tests that const writers produce the same output as runtime encoders.
#![allow(dead_code, unused_imports)]

/// Helper: runtime-encode a value with CXS, return the bytes.
fn cxs_runtime_encode<T: xdrs::WriteXDR>(val: &T) -> Vec<u8> {
    xdrs::cxs::encode(val).unwrap()
}

/// Helper: runtime-encode a value with TXS, return the bytes.
fn txs_runtime_encode<T: xdrs::WriteXDR>(val: &T) -> Vec<u8> {
    xdrs::txs::encode(val).unwrap()
}

// -- TXS cross-validation --

const TXS_I32_NEG1: [u8; 4] = {
    let mut w = xdrs::txs::ConstWriter::<4>::new();
    w.write_i32(-1);
    w.into_bytes()
};

const TXS_U64_DEADBEEF: [u8; 8] = {
    let mut w = xdrs::txs::ConstWriter::<8>::new();
    w.write_u64(0xdead1234beef5678);
    w.into_bytes()
};

const TXS_OPAQUE_FIXED_3: [u8; 4] = {
    let mut w = xdrs::txs::ConstWriter::<4>::new();
    w.write_opaque_fixed(&[0xAA, 0xBB, 0xCC]);
    w.into_bytes()
};

#[test]
fn txs_cross_validate_i32() {
    assert_eq!(&TXS_I32_NEG1, txs_runtime_encode(&-1i32).as_slice());
}

#[test]
fn txs_cross_validate_u64() {
    assert_eq!(
        &TXS_U64_DEADBEEF,
        txs_runtime_encode(&0xdead1234beef5678u64).as_slice()
    );
}

#[test]
fn txs_cross_validate_opaque_fixed() {
    let bytes: [u8; 3] = [0xAA, 0xBB, 0xCC];
    assert_eq!(&TXS_OPAQUE_FIXED_3, txs_runtime_encode(&bytes).as_slice());
}

// -- CXS cross-validation --

const CXS_I32_0: [u8; 1] = {
    let mut w = xdrs::cxs::ConstWriter::<1>::new();
    w.write_i32(0);
    w.into_bytes()
};

const CXS_I32_MAX: [u8; 5] = {
    let mut w = xdrs::cxs::ConstWriter::<5>::new();
    w.write_i32(i32::MAX);
    w.into_bytes()
};

const CXS_U32_DEADBEEF: [u8; 5] = {
    let mut w = xdrs::cxs::ConstWriter::<5>::new();
    w.write_u32(0xdeadbeef);
    w.into_bytes()
};

const CXS_I64_MAX: [u8; 9] = {
    let mut w = xdrs::cxs::ConstWriter::<9>::new();
    w.write_i64(i64::MAX);
    w.into_bytes()
};

const CXS_U64_DB: [u8; 9] = {
    let mut w = xdrs::cxs::ConstWriter::<9>::new();
    w.write_u64(0xdead1234beef5678);
    w.into_bytes()
};

#[test]
fn cxs_cross_validate_i32_zero() {
    assert_eq!(&CXS_I32_0, cxs_runtime_encode(&0i32).as_slice());
}

#[test]
fn cxs_cross_validate_i32_max() {
    assert_eq!(&CXS_I32_MAX, cxs_runtime_encode(&i32::MAX).as_slice());
}

#[test]
fn cxs_cross_validate_u32_deadbeef() {
    assert_eq!(
        &CXS_U32_DEADBEEF,
        cxs_runtime_encode(&0xdeadbeefu32).as_slice()
    );
}

#[test]
fn cxs_cross_validate_i64_max() {
    assert_eq!(&CXS_I64_MAX, cxs_runtime_encode(&i64::MAX).as_slice());
}

#[test]
fn cxs_cross_validate_u64_db() {
    assert_eq!(
        &CXS_U64_DB,
        cxs_runtime_encode(&0xdead1234beef5678u64).as_slice()
    );
}

// -- Derive macro tests: structs --

#[derive(xdrs::ConstWriteXDR, xdrs::WriteXDR)]
struct Foo {
    bytes: [u8; 3],
    blerg: i32,
}

const FOO: Foo = Foo {
    bytes: [0xAA, 0xBB, 0xCC],
    blerg: 7,
};

const FOO_CXS: [u8; 4] = {
    let mut w = xdrs::cxs::ConstWriter::<4>::new();
    FOO.const_write_cxs(&mut w);
    w.into_bytes()
};

const FOO_TXS: [u8; 8] = {
    let mut w = xdrs::txs::ConstWriter::<8>::new();
    FOO.const_write_txs(&mut w);
    w.into_bytes()
};

#[test]
fn derive_struct_cxs() {
    assert_eq!(&FOO_CXS, cxs_runtime_encode(&FOO).as_slice());
}

#[test]
fn derive_struct_txs() {
    assert_eq!(&FOO_TXS, txs_runtime_encode(&FOO).as_slice());
}

// -- Derive macro tests: enums --

#[derive(xdrs::ConstWriteXDR, xdrs::WriteXDR)]
#[repr(i32)]
enum Status {
    On,
    Off,
}

const STATUS_ON_CXS: [u8; 1] = {
    let mut w = xdrs::cxs::ConstWriter::<1>::new();
    Status::On.const_write_cxs(&mut w);
    w.into_bytes()
};

const STATUS_ON_TXS: [u8; 4] = {
    let mut w = xdrs::txs::ConstWriter::<4>::new();
    Status::On.const_write_txs(&mut w);
    w.into_bytes()
};

const STATUS_OFF_CXS: [u8; 1] = {
    let mut w = xdrs::cxs::ConstWriter::<1>::new();
    Status::Off.const_write_cxs(&mut w);
    w.into_bytes()
};

#[test]
fn derive_enum_bare() {
    assert_eq!(&STATUS_ON_CXS, cxs_runtime_encode(&Status::On).as_slice());
    assert_eq!(&STATUS_ON_TXS, txs_runtime_encode(&Status::On).as_slice());
    assert_eq!(&STATUS_OFF_CXS, cxs_runtime_encode(&Status::Off).as_slice());
}

// Enum with fields
#[derive(xdrs::ConstWriteXDR, xdrs::WriteXDR)]
#[repr(i32)]
enum Shape {
    Circle { radius: u32 } = 0,
    Rect { w: u32, h: u32 } = 1,
    Empty = 2,
}

const CIRCLE_CXS: [u8; 2] = {
    let mut w = xdrs::cxs::ConstWriter::<2>::new();
    (Shape::Circle { radius: 5 }).const_write_cxs(&mut w);
    w.into_bytes()
};

const RECT_TXS: [u8; 12] = {
    let mut w = xdrs::txs::ConstWriter::<12>::new();
    (Shape::Rect { w: 10, h: 20 }).const_write_txs(&mut w);
    w.into_bytes()
};

#[test]
fn derive_enum_with_fields() {
    let circle = Shape::Circle { radius: 5 };
    assert_eq!(&CIRCLE_CXS, cxs_runtime_encode(&circle).as_slice());

    let rect = Shape::Rect { w: 10, h: 20 };
    assert_eq!(&RECT_TXS, txs_runtime_encode(&rect).as_slice());
}

// Nested: struct containing enum
#[derive(xdrs::ConstWriteXDR, xdrs::WriteXDR)]
struct Config {
    version: u32,
    status: Status,
}

const CONFIG: Config = Config {
    version: 1,
    status: Status::On,
};

const CONFIG_CXS: [u8; 2] = {
    let mut w = xdrs::cxs::ConstWriter::<2>::new();
    CONFIG.const_write_cxs(&mut w);
    w.into_bytes()
};

const CONFIG_TXS: [u8; 8] = {
    let mut w = xdrs::txs::ConstWriter::<8>::new();
    CONFIG.const_write_txs(&mut w);
    w.into_bytes()
};

#[test]
fn derive_nested_struct_enum() {
    assert_eq!(&CONFIG_CXS, cxs_runtime_encode(&CONFIG).as_slice());
    assert_eq!(&CONFIG_TXS, txs_runtime_encode(&CONFIG).as_slice());
}
