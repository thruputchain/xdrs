//! Tests that const writers produce the same output as runtime encoders.

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
    assert_eq!(
        &TXS_OPAQUE_FIXED_3,
        txs_runtime_encode(&bytes).as_slice()
    );
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
