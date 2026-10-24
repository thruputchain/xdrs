//! Differential tests for the unchecked write path: for every value, the
//! unchecked writers must produce byte-identical output to the checked
//! writers, in both formats. Also exercises the debug_assert guards on
//! contract violations (debug builds only).

use core::mem::MaybeUninit as MU;
use xdrs::{BytesM, StringM, VecM, WriteXDR, cxs, txs};

fn assert_unchecked_matches_checked<T: WriteXDR>(value: &T) {
    // TXS
    let mut cbuf = [MU::uninit(); 512];
    let clen = txs::encode_into(value, &mut cbuf).unwrap();
    let mut ubuf = [MU::uninit(); 512];
    // SAFETY: the checked encode just confirmed the value fits in 512
    // bytes, and all test values respect their declared bounds.
    let ulen = unsafe { txs::encode_into_unchecked(value, &mut ubuf) };
    assert_eq!(clen, ulen, "TXS length mismatch");
    // SAFETY: both writers initialized their first `len` bytes.
    let (c, u) = unsafe {
        (
            cbuf[..clen].assume_init_ref(),
            ubuf[..ulen].assume_init_ref(),
        )
    };
    assert_eq!(c, u, "TXS bytes mismatch");

    // CXS
    let mut cbuf = [MU::uninit(); 512];
    let clen = cxs::encode_into(value, &mut cbuf).unwrap();
    let mut ubuf = [MU::uninit(); 512];
    // SAFETY: as above.
    let ulen = unsafe { cxs::encode_into_unchecked(value, &mut ubuf) };
    assert_eq!(clen, ulen, "CXS length mismatch");
    // SAFETY: both writers initialized their first `len` bytes.
    let (c, u) = unsafe {
        (
            cbuf[..clen].assume_init_ref(),
            ubuf[..ulen].assume_init_ref(),
        )
    };
    assert_eq!(c, u, "CXS bytes mismatch");
}

// Same shape as tests/const_size_bounds.rs Primitives — known to support
// both WriteXDR and ConstXDRSize (max bounds).
#[derive(xdrs::WriteXDR, xdrs::ConstXDRSize)]
struct Primitives {
    a: u32,
    b: i64,
    c: bool,
    d: f32,
    e: [u8; 5],
    f: (u16, i16),
    g: core::marker::PhantomData<u64>,
}

#[derive(xdrs::WriteXDR)]
struct Nested {
    p: Primitives,
    q: Option<u32>,
    r: Result<u32, i64>,
    s: [u32; 3],
}

#[derive(xdrs::WriteXDR)]
#[repr(i32)]
enum Shape {
    Unit,
    Tuple(u32, i64),
    Struct { x: [u8; 3], y: bool },
}

#[derive(xdrs::WriteXDR)]
struct Unit;

fn worst_primitives() -> Primitives {
    Primitives {
        a: u32::MAX,
        b: i64::MAX,
        c: true,
        d: 0.5,
        e: [0xAB; 5],
        f: (u16::MAX, i16::MIN),
        g: core::marker::PhantomData,
    }
}

fn small_primitives() -> Primitives {
    Primitives {
        a: 0,
        b: 0,
        c: false,
        d: 0.0,
        e: [0; 5],
        f: (0, 0),
        g: core::marker::PhantomData,
    }
}

#[test]
fn structs_match() {
    assert_unchecked_matches_checked(&worst_primitives());
    assert_unchecked_matches_checked(&small_primitives());
    assert_unchecked_matches_checked(&Unit);
    assert_unchecked_matches_checked(&Nested {
        p: worst_primitives(),
        q: Some(7),
        r: Err(-1),
        s: [1, 70000, u32::MAX],
    });
    assert_unchecked_matches_checked(&Nested {
        p: small_primitives(),
        q: None,
        r: Ok(3),
        s: [0; 3],
    });
}

#[test]
fn enums_match() {
    assert_unchecked_matches_checked(&Shape::Unit);
    assert_unchecked_matches_checked(&Shape::Tuple(300, -5_000_000_000));
    assert_unchecked_matches_checked(&Shape::Struct {
        x: [1, 2, 3],
        y: true,
    });
}

#[test]
fn containers_match() {
    let v = VecM::<u32, 10>(vec![1, 2, 300]);
    let b = BytesM::<8>(vec![1, 2, 3]);
    let s = StringM::<16>(b"hello".to_vec());
    assert_unchecked_matches_checked(&v);
    assert_unchecked_matches_checked(&b);
    assert_unchecked_matches_checked(&s);

    let bytes: &[u8] = b"some raw bytes";
    assert_unchecked_matches_checked(&bytes);
    assert_unchecked_matches_checked(&"a str");
    assert_unchecked_matches_checked(&vec![1u32, 2, 3]);
    assert_unchecked_matches_checked(&(1u32, true, 3.5f64));
}

#[test]
fn int128_matches() {
    assert_unchecked_matches_checked(&i128::MIN);
    assert_unchecked_matches_checked(&u128::MAX);
    assert_unchecked_matches_checked(&0i128);
}

// The headline use case: size the buffer with the const worst-case bound,
// then encode without checks.
#[test]
fn buffer_sized_from_const_max_bound() {
    const TXS_BOUND: usize = match <Primitives as xdrs::txs::ConstMaxSize>::MAX_SIZE {
        Some(n) => n,
        None => panic!("unbounded"),
    };
    const CXS_BOUND: usize = match <Primitives as xdrs::cxs::ConstMaxSize>::MAX_SIZE {
        Some(n) => n,
        None => panic!("unbounded"),
    };

    let value = worst_primitives();

    let mut buf = [MU::uninit(); TXS_BOUND];
    // SAFETY: the buffer is sized to the worst-case TXS bound.
    let written = unsafe { txs::encode_into_unchecked(&value, &mut buf) };
    // TXS is fixed-width: the worst case is exact.
    assert_eq!(written, TXS_BOUND);

    let mut buf = [MU::uninit(); CXS_BOUND];
    // SAFETY: the buffer is sized to the worst-case CXS bound.
    let written = unsafe { cxs::encode_into_unchecked(&value, &mut buf) };
    assert!(written <= CXS_BOUND);

    // A small value fits within the same worst-case buffer.
    let small = small_primitives();
    let mut buf = [MU::uninit(); CXS_BOUND];
    // SAFETY: CXS_BOUND bounds the encoded size of every Primitives value.
    let written = unsafe { cxs::encode_into_unchecked(&small, &mut buf) };
    assert!(written <= CXS_BOUND);
}

// Contract violations must be caught by debug_assert in debug builds.
// Gated on debug_assertions: without them these calls would be UB.
#[cfg(debug_assertions)]
mod debug_assert_violations {
    use super::*;

    #[test]
    #[should_panic(expected = "UncheckedBufWriter: buffer overflow")]
    fn short_buffer_txs() {
        let mut buf = [MU::uninit(); 3]; // a u32 needs 4
        let _ = unsafe { txs::encode_into_unchecked(&42u32, &mut buf) };
    }

    #[test]
    #[should_panic(expected = "UncheckedBufWriter: buffer overflow")]
    fn short_buffer_cxs() {
        let mut buf: [MU<u8>; 0] = [];
        let _ = unsafe { cxs::encode_into_unchecked(&42u32, &mut buf) };
    }

    #[test]
    #[should_panic(expected = "exceeds declared bound")]
    fn overlong_vecm() {
        // VecM's inner Vec is public, so the declared bound can be violated
        // by construction; the unchecked path debug_asserts it.
        let v = VecM::<u32, 2>(vec![1, 2, 3]);
        let mut buf = [MU::uninit(); 64];
        let _ = unsafe { txs::encode_into_unchecked(&v, &mut buf) };
    }

    #[test]
    #[should_panic(expected = "exceeds declared bound")]
    fn overlong_bytesm() {
        let b = BytesM::<2>(vec![1, 2, 3]);
        let mut buf = [MU::uninit(); 64];
        let _ = unsafe { cxs::encode_into_unchecked(&b, &mut buf) };
    }
}
