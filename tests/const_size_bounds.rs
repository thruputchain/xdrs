//! Tests worst-case size bounds: `const_max_cxs_size` / `const_max_txs_size`.
//!
//! Bounds fold through by-value layout only. Pointer-cut shapes (`&T`,
//! `&[T]`, `&str`, element-generic `VecM<T, MAX>`) report `None`, which is
//! what lets recursive types derive `ConstWriteXDR` without a const-eval
//! cycle — see `Node` below.
#![allow(dead_code)]

use core::marker::PhantomData;
use xdrs::{BytesM, StringM, VecM};

#[derive(xdrs::ConstWriteXDR)]
struct Primitives {
    a: u32,
    b: i64,
    c: bool,
    d: f32,
    e: [u8; 5],
    f: (u16, i16),
    g: PhantomData<u64>,
}

// CXS worst case: 5 + 9 + 1 + 4 + 5 + (3 + 3) + 0.
const _: () = assert!(matches!(Primitives::const_max_cxs_size(), Some(30)));
// TXS: 4 + 8 + 4 + 4 + pad4(5) + (4 + 4) + 0.
const _: () = assert!(matches!(Primitives::const_max_txs_size(), Some(36)));

/// A CXS bound sizes a `ConstWriter` buffer, and worst-case values fill it
/// exactly (`into_bytes` asserts the buffer is filled).
const CXS_BOUND: usize = match Primitives::const_max_cxs_size() {
    Some(n) => n,
    None => panic!("unbounded"),
};
const TXS_BOUND: usize = match Primitives::const_max_txs_size() {
    Some(n) => n,
    None => panic!("unbounded"),
};

const WORST: Primitives = Primitives {
    a: u32::MAX,
    b: i64::MAX,
    c: true,
    d: 0.0,
    e: [0xAB; 5],
    f: (u16::MAX, i16::MAX),
    g: PhantomData,
};

const WORST_CXS: [u8; CXS_BOUND] = {
    let mut w = xdrs::cxs::ConstWriter::<CXS_BOUND>::new();
    WORST.const_write_cxs(&mut w);
    w.into_bytes()
};

// TXS is fixed-width, so every value's encoded size equals the bound.
const SMALL: Primitives = Primitives {
    a: 0,
    b: 0,
    c: false,
    d: 0.0,
    e: [0; 5],
    f: (0, 0),
    g: PhantomData,
};

const SMALL_TXS: [u8; TXS_BOUND] = {
    let mut w = xdrs::txs::ConstWriter::<TXS_BOUND>::new();
    SMALL.const_write_txs(&mut w);
    w.into_bytes()
};

#[test]
fn value_sizes_never_exceed_bounds() {
    assert!(WORST.const_cxs_size() <= CXS_BOUND);
    assert_eq!(WORST.const_cxs_size(), CXS_BOUND);
    assert!(SMALL.const_cxs_size() <= CXS_BOUND);
    assert_eq!(SMALL.const_txs_size(), TXS_BOUND);
    assert_eq!(WORST.const_txs_size(), TXS_BOUND);
}

// -- Nested by-value leaves fold through the leaf's bound --

#[derive(xdrs::ConstWriteXDR)]
struct Outer {
    inner: Primitives,
    pair: [Primitives; 2],
    maybe: Option<Primitives>,
    choice: Result<u32, Primitives>,
}

// CXS: 30 + 2*30 + (1 + 30) + (1 + max(5, 30)).
const _: () = assert!(matches!(Outer::const_max_cxs_size(), Some(152)));
// TXS: 36 + 2*36 + (4 + 36) + (4 + max(4, 36)).
const _: () = assert!(matches!(Outer::const_max_txs_size(), Some(188)));

// -- Enums bound to the max over variants --

#[derive(xdrs::ConstWriteXDR)]
#[repr(i32)]
enum Choice {
    Small(u16),
    Big(u64, u64),
}

// CXS: max(size_i32(0) + 3, size_i32(1) + 9 + 9) = max(4, 19).
const _: () = assert!(matches!(Choice::const_max_cxs_size(), Some(19)));
// TXS: max(4 + 4, 4 + 8 + 8).
const _: () = assert!(matches!(Choice::const_max_txs_size(), Some(20)));

#[derive(xdrs::ConstWriteXDR)]
#[repr(i32)]
enum WithDefault {
    Known(u16),
    #[xdr(default_variant)]
    Other(i32),
}

// The default arm's discriminant is a runtime i32: CXS worst case 5.
const _: () = assert!(matches!(WithDefault::const_max_cxs_size(), Some(5)));
// TXS: max(4 + 4, 4).
const _: () = assert!(matches!(WithDefault::const_max_txs_size(), Some(8)));

// -- Bounded byte containers keep exact bounds --

#[derive(xdrs::ConstWriteXDR)]
struct Bytes {
    blob: BytesM<16>,
    name: StringM<8>,
    raw: VecM<u8, 4>,
}

// CXS: (size_u32(16) + 16) + (size_u32(8) + 8) + (size_u32(4) + 4).
const _: () = assert!(matches!(Bytes::const_max_cxs_size(), Some(31)));
// TXS: (4 + 16) + (4 + 8) + (4 + 4).
const _: () = assert!(matches!(Bytes::const_max_txs_size(), Some(40)));

// A `MAX` that is not spelled out defaults to `u32::MAX`: a huge but finite
// bound on 64-bit targets.
#[derive(xdrs::ConstWriteXDR)]
struct DefaultMax {
    blob: BytesM,
}

#[cfg(target_pointer_width = "64")]
const _: () = assert!(matches!(
    DefaultMax::const_max_cxs_size(),
    Some(n) if n == 5 + u32::MAX as usize
));
#[cfg(target_pointer_width = "64")]
const _: () = assert!(matches!(
    DefaultMax::const_max_txs_size(),
    Some(n) if n == 4 + u32::MAX as usize + 1
));

// -- Pointer cuts and unbounded lengths report None --

#[derive(xdrs::ConstWriteXDR)]
struct Cuts<'a> {
    by_ref: &'a u32,
}

const _: () = assert!(Cuts::const_max_cxs_size().is_none());
const _: () = assert!(Cuts::const_max_txs_size().is_none());

#[derive(xdrs::ConstWriteXDR)]
struct Slices<'a> {
    raw: &'a [u8],
    name: &'a str,
    items: &'a [u32],
}

const _: () = assert!(Slices::const_max_cxs_size().is_none());
const _: () = assert!(Slices::const_max_txs_size().is_none());

// Element-generic VecM is heap-backed indirection: cut even with a MAX.
#[derive(xdrs::ConstWriteXDR)]
struct Items {
    items: VecM<u32, 5>,
}

const _: () = assert!(Items::const_max_cxs_size().is_none());
const _: () = assert!(Items::const_max_txs_size().is_none());

// A single unbounded field makes the whole type unbounded, even alongside
// bounded ones.
#[derive(xdrs::ConstWriteXDR)]
struct Mixed<'a> {
    n: u32,
    raw: &'a [u8],
}

const _: () = assert!(Mixed::const_max_cxs_size().is_none());

// An unbounded variant makes the whole enum unbounded.
#[derive(xdrs::ConstWriteXDR)]
#[repr(i32)]
enum MixedEnum<'a> {
    Bounded(u32),
    Unbounded(&'a [u8]),
}

const _: () = assert!(MixedEnum::const_max_cxs_size().is_none());
const _: () = assert!(MixedEnum::const_max_txs_size().is_none());

// A zero-length array encodes nothing, so it stays bounded even with an
// unbounded element type.
#[derive(xdrs::ConstWriteXDR)]
struct ZeroLen<'a> {
    n: u32,
    none_of_them: [&'a [u8]; 0],
}

const _: () = assert!(matches!(ZeroLen::const_max_cxs_size(), Some(5)));
const _: () = assert!(matches!(ZeroLen::const_max_txs_size(), Some(4)));

// A finite bound that overflows `usize` reports None instead of failing
// const evaluation: each element bounds to ~2^32, and 2^16 * 2^16 elements
// push the product past 2^64. (Compile-time only; never instantiated.)
#[derive(xdrs::ConstWriteXDR)]
struct Overflowing {
    blobs: [[BytesM; 65536]; 65536],
}

const _: () = assert!(Overflowing::const_max_cxs_size().is_none());
const _: () = assert!(Overflowing::const_max_txs_size().is_none());

// -- The point of the pointer cut: recursive types derive and bound to None
// without a const-eval cycle --

#[derive(xdrs::ConstWriteXDR)]
struct Node<'a> {
    val: u32,
    next: Option<&'a Node<'a>>,
}

const _: () = assert!(Node::const_max_cxs_size().is_none());
const _: () = assert!(Node::const_max_txs_size().is_none());

// -- int128 --

#[cfg(feature = "int128")]
#[derive(xdrs::ConstWriteXDR)]
struct Wide {
    a: i128,
    b: u128,
}

#[cfg(feature = "int128")]
const _: () = assert!(matches!(Wide::const_max_cxs_size(), Some(36)));
#[cfg(feature = "int128")]
const _: () = assert!(matches!(Wide::const_max_txs_size(), Some(32)));

#[test]
fn bounds_are_stable() {
    // Const asserts above are the real test; this keeps the target non-empty
    // and exercises the fns at runtime too.
    assert_eq!(Primitives::const_max_cxs_size(), Some(30));
    assert_eq!(Primitives::const_max_txs_size(), Some(36));
    assert_eq!(Choice::const_max_cxs_size(), Some(19));
    assert_eq!(Items::const_max_cxs_size(), None);
    assert_eq!(WORST_CXS.len(), CXS_BOUND);
    assert_eq!(SMALL_TXS.len(), TXS_BOUND);
}
