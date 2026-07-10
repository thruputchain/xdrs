use core::mem::MaybeUninit;
use xdrs::cxs;

/// Encode via the fallible reference path; return the bytes in a fixed buffer + len.
///
/// Note: `cxs::encode_into` returns `WriteResult<usize>` (number of bytes written),
/// so we read back the initialized portion of the `MaybeUninit` buffer ourselves.
fn reference_cxs<T: xdrs::WriteXDR>(v: &T) -> ([u8; 64], usize) {
    let mut mu = [MaybeUninit::<u8>::uninit(); 64];
    let n = cxs::encode_into(v, &mut mu).expect("reference encode failed");
    let mut out = [0u8; 64];
    // SAFETY: encode_into guarantees that the first n bytes of `mu` are initialized.
    let mut i = 0;
    while i < n {
        out[i] = unsafe { mu[i].assume_init() };
        i += 1;
    }
    (out, n)
}

macro_rules! assert_cursor_matches {
    ($write_method:ident, $val:expr) => {{
        let v = $val;
        let (expected, len) = reference_cxs(&v);
        let mut buf = [0u8; 64];
        let mut c = cxs::Cursor::new(&mut buf);
        c.$write_method(v);
        assert_eq!(c.pos(), len, "size mismatch for {:?}", v);
        assert_eq!(c.as_bytes(), &expected[..len], "bytes mismatch for {:?}", v);
    }};
}

#[test]
fn cursor_i32_width_boundaries() {
    for v in [
        0i32,
        1,
        -1,
        31,
        32,
        -31,
        -32,
        62,
        63,
        127,
        128,
        255,
        256,
        32767,
        32768,
        65535,
        65536,
        i32::MAX,
        i32::MIN,
    ] {
        assert_cursor_matches!(write_i32, v);
    }
}

#[test]
fn cursor_u32_u64_i64_boundaries() {
    for v in [0u32, 62, 63, 255, 256, 65535, 65536, u32::MAX] {
        assert_cursor_matches!(write_u32, v);
    }
    for v in [
        0u64,
        62,
        63,
        65536,
        u32::MAX as u64,
        u32::MAX as u64 + 1,
        u64::MAX,
    ] {
        assert_cursor_matches!(write_u64, v);
    }
    for v in [
        0i64,
        -32,
        32768,
        i32::MAX as i64 + 1,
        i32::MIN as i64 - 1,
        i64::MAX,
        i64::MIN,
    ] {
        assert_cursor_matches!(write_i64, v);
    }
}

#[test]
fn cursor_u16_i16_bool_floats() {
    for v in [0u16, 62, 63, 255, 256, u16::MAX] {
        assert_cursor_matches!(write_u16, v);
    }
    for v in [0i16, -31, -32, 127, 128, i16::MAX, i16::MIN] {
        assert_cursor_matches!(write_i16, v);
    }
    assert_cursor_matches!(write_bool, true);
    assert_cursor_matches!(write_bool, false);
    assert_cursor_matches!(write_f32, 1.5f32);
    assert_cursor_matches!(write_f64, -2.75f64);
}

#[cfg(feature = "int128")]
#[test]
fn cursor_i128_u128() {
    for v in [0i128, -1, i64::MAX as i128 + 1, i128::MAX, i128::MIN] {
        let (expected, len) = reference_cxs(&v);
        let mut buf = [0u8; 64];
        let mut c = cxs::Cursor::new(&mut buf);
        c.write_i128(v);
        assert_eq!((c.pos(), c.as_bytes()), (len, &expected[..len]));
    }
    let mut buf = [0u8; 64];
    let mut c = cxs::Cursor::new(&mut buf);
    c.write_u128(u128::MAX);
    let (expected, len) = reference_cxs(&u128::MAX);
    assert_eq!((c.pos(), c.as_bytes()), (len, &expected[..len]));
}

#[test]
fn cursor_opaque() {
    // variable: length prefix + bytes; fixed: raw bytes
    let data = [0xde, 0xad, 0xbe, 0xef];
    let (expected, len) = reference_cxs(&&data[..]); // &[u8] → opaque variable
    let mut buf = [0u8; 64];
    let mut c = cxs::Cursor::new(&mut buf);
    c.write_opaque_variable(&data);
    assert_eq!((c.pos(), c.as_bytes()), (len, &expected[..len]));

    let mut buf2 = [0u8; 4];
    let mut c2 = cxs::Cursor::new(&mut buf2);
    c2.write_opaque_fixed(&data);
    assert_eq!(c2.as_bytes(), &data);
}

// Const-context usability: build a const array through Cursor.
const CONST_VIA_CURSOR: [u8; 3] = {
    let mut buf = [0u8; 3];
    let mut c = cxs::Cursor::new(&mut buf);
    c.write_i32(-32); // 0x3f 0xe0
    c.write_i32(1); // 0x01
    assert!(c.pos() == 3);
    buf
};

#[test]
fn cursor_const_context() {
    assert_eq!(CONST_VIA_CURSOR, [0x3f, 0xe0, 0x01]);
}

#[test]
#[should_panic]
fn cursor_overflow_panics() {
    let mut buf = [0u8; 1];
    let mut c = cxs::Cursor::new(&mut buf);
    c.write_u32(u32::MAX); // needs 5 bytes
}

// new_uninit: byte-equality against the encode_into reference, over an
// uninitialized backing buffer (no zero-init).
#[test]
fn cursor_new_uninit_matches_reference() {
    let mut buf = [MaybeUninit::<u8>::uninit(); 64];
    let mut c = cxs::Cursor::new_uninit(&mut buf);
    c.write_u32(300);
    c.write_i32(-32);
    let (expected, len) = reference_cxs(&(300u32, -32i32));
    assert_eq!(c.pos(), len);
    assert_eq!(c.as_bytes(), &expected[..len]);
}

// new_uninit is usable in const context.
const CXS_UNINIT_LEN: usize = {
    let mut buf = [MaybeUninit::uninit(); 8];
    let mut c = cxs::Cursor::new_uninit(&mut buf);
    c.write_u32(300);
    c.pos()
};

#[test]
fn cursor_new_uninit_const_context() {
    assert_eq!(CXS_UNINIT_LEN, 3); // 0x7f 0x2c 0x01
}

#[test]
#[should_panic]
fn cursor_new_uninit_overflow_panics() {
    let mut buf = [MaybeUninit::<u8>::uninit(); 1];
    let mut c = cxs::Cursor::new_uninit(&mut buf);
    c.write_u32(u32::MAX); // needs 5 bytes
}

// -- TXS Cursor tests --

use xdrs::txs;

/// Encode via the fallible reference path; return the bytes in a fixed buffer + len.
///
/// Note: `txs::encode_into` returns `WriteResult<usize>` (number of bytes written),
/// so we read back the initialized portion of the `MaybeUninit` buffer ourselves.
fn reference_txs<T: xdrs::WriteXDR>(v: &T) -> ([u8; 64], usize) {
    let mut mu = [MaybeUninit::<u8>::uninit(); 64];
    let n = txs::encode_into(v, &mut mu).expect("reference encode failed");
    let mut out = [0u8; 64];
    // SAFETY: encode_into guarantees that the first n bytes of `mu` are initialized.
    let mut i = 0;
    while i < n {
        out[i] = unsafe { mu[i].assume_init() };
        i += 1;
    }
    (out, n)
}

macro_rules! assert_txs_cursor_matches {
    ($write_method:ident, $val:expr) => {{
        let v = $val;
        let (expected, len) = reference_txs(&v);
        let mut buf = [0u8; 64];
        let mut c = xdrs::txs::Cursor::new(&mut buf);
        c.$write_method(v);
        assert_eq!(c.pos(), len);
        assert_eq!(c.as_bytes(), &expected[..len]);
    }};
}

#[test]
fn txs_cursor_all_primitives() {
    for v in [0i32, -1, i32::MAX, i32::MIN] {
        assert_txs_cursor_matches!(write_i32, v);
    }
    for v in [0u32, u32::MAX] {
        assert_txs_cursor_matches!(write_u32, v);
    }
    for v in [0i64, i64::MAX, i64::MIN] {
        assert_txs_cursor_matches!(write_i64, v);
    }
    for v in [0u64, u64::MAX] {
        assert_txs_cursor_matches!(write_u64, v);
    }
    for v in [0i16, i16::MIN, i16::MAX] {
        assert_txs_cursor_matches!(write_i16, v);
    }
    for v in [0u16, u16::MAX] {
        assert_txs_cursor_matches!(write_u16, v);
    }
    assert_txs_cursor_matches!(write_bool, true);
    assert_txs_cursor_matches!(write_bool, false);
    assert_txs_cursor_matches!(write_f32, 1.5f32);
    assert_txs_cursor_matches!(write_f64, -2.75f64);
}

#[test]
fn txs_cursor_opaque_padding() {
    // 5 data bytes → variable = 4 (len) + 5 + 3 (pad) = 12
    let data = [1u8, 2, 3, 4, 5];
    let (expected, len) = reference_txs(&&data[..]);
    assert_eq!(len, 12);
    let mut buf = [0u8; 64];
    let mut c = xdrs::txs::Cursor::new(&mut buf);
    c.write_opaque_variable(&data);
    assert_eq!((c.pos(), c.as_bytes()), (len, &expected[..len]));
}

// new_uninit: byte-equality against the encode_into reference, over an
// uninitialized backing buffer (no zero-init).
#[test]
fn txs_cursor_new_uninit_matches_reference() {
    let mut buf = [MaybeUninit::<u8>::uninit(); 64];
    let mut c = txs::Cursor::new_uninit(&mut buf);
    c.write_u32(300);
    c.write_i32(-32);
    let (expected, len) = reference_txs(&(300u32, -32i32));
    assert_eq!(c.pos(), len);
    assert_eq!(c.as_bytes(), &expected[..len]);
}

// new_uninit is usable in const context.
const TXS_UNINIT_LEN: usize = {
    let mut buf = [MaybeUninit::uninit(); 8];
    let mut c = txs::Cursor::new_uninit(&mut buf);
    c.write_u32(300);
    c.pos()
};

#[test]
fn txs_cursor_new_uninit_const_context() {
    assert_eq!(TXS_UNINIT_LEN, 4); // big-endian u32 is always 4 bytes
}
