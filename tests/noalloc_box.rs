//! No-`alloc` `Box<T>` is an inline newtype around `T`, so a derived type
//! with a boxed field decodes from any buffer, not only `'static` data, and
//! `Box::new`, `Deref` and `AsRef<T>` read the same as with the `alloc` `Box`.
//!
//! Regression: when the alias was `&'static T`, the derived
//! `ReadXDR<'_r>` of a type with a `Box` field failed to compile ("lifetime
//! may not live long enough"). This file compiles to nothing with `alloc`.
#![cfg(not(feature = "alloc"))]

use core::mem::MaybeUninit;
use xdrs::{Box, ConstWriteXDR, ReadXDR, WriteXDR, cxs, txs};

#[derive(Debug, PartialEq, ReadXDR, WriteXDR, xdrs::ConstXDRSize)]
#[repr(i32)]
enum Key {
    Small { small: [u8; 4] } = 0,
    Big { big: Box<[u8; 8]> } = 1,
}

#[derive(Debug, PartialEq, ReadXDR, WriteXDR, xdrs::ConstXDRSize)]
struct Holder {
    key: Option<Key>,
}

#[test]
fn boxed_field_roundtrips_from_stack_buffer() {
    let val = Holder {
        key: Some(Key::Big {
            big: Box::new([1, 2, 3, 4, 5, 6, 7, 8]),
        }),
    };

    let mut buf = [MaybeUninit::<u8>::uninit(); 64];
    let n = txs::encode_into(&val, &mut buf).unwrap();
    // A local copy, so the decode lifetime is not 'static.
    let mut local = [0u8; 64];
    for (d, s) in local.iter_mut().zip(&buf[..n]) {
        // SAFETY: encode_into initialized the first `n` bytes.
        *d = unsafe { s.assume_init() };
    }
    assert_eq!(txs::decode::<Holder>(&local[..n]).unwrap(), val);

    let mut buf = [MaybeUninit::<u8>::uninit(); 64];
    let n = cxs::encode_into(&val, &mut buf).unwrap();
    let mut local = [0u8; 64];
    for (d, s) in local.iter_mut().zip(&buf[..n]) {
        // SAFETY: encode_into initialized the first `n` bytes.
        *d = unsafe { s.assume_init() };
    }
    assert_eq!(cxs::decode::<Holder>(&local[..n]).unwrap(), val);
}

// The newtype is transparent: no size overhead over the pointee.
const _: () = assert!(core::mem::size_of::<Box<[u8; 8]>>() == 8);

// An inline `Box` keeps the pointee's size bound in no-`alloc` builds.
const _: () = assert!(<Holder as xdrs::txs::ConstMaxSize>::MAX_SIZE.is_some());
const _: () = assert!(<Holder as xdrs::cxs::ConstMaxSize>::MAX_SIZE.is_some());

// `ConstWriteXDR` reaches a boxed field through `box_as_ref`, since `Deref`
// is not const. `Box::new` is const in no-`alloc` builds, so the whole value
// can be built and encoded at compile time.
#[derive(Debug, Clone, Copy, PartialEq, ConstWriteXDR, WriteXDR)]
struct Leaf {
    a: u32,
    b: [u8; 3],
}

#[derive(Debug, PartialEq, ConstWriteXDR, WriteXDR)]
struct Boxed {
    n: u32,
    leaf: Box<Leaf>,
    maybe: Option<Box<Leaf>>,
    raw: Box<[u8; 4]>,
}

const BOXED: Boxed = Boxed {
    n: 7,
    leaf: Box::new(Leaf { a: 1, b: [2, 3, 4] }),
    maybe: Some(Box::new(Leaf { a: 5, b: [6, 7, 8] })),
    raw: Box::new([9, 10, 11, 12]),
};

const BOXED_TXS: [u8; BOXED.const_txs_size()] = {
    let mut w = txs::ConstWriter::<{ BOXED.const_txs_size() }>::new();
    BOXED.const_write_txs(&mut w);
    w.into_bytes()
};

const BOXED_CXS: [u8; BOXED.const_cxs_size()] = {
    let mut w = cxs::ConstWriter::<{ BOXED.const_cxs_size() }>::new();
    BOXED.const_write_cxs(&mut w);
    w.into_bytes()
};

#[test]
fn boxed_const_write_matches_runtime() {
    let mut buf = [MaybeUninit::<u8>::uninit(); 64];
    let n = txs::encode_into(&BOXED, &mut buf).unwrap();
    // SAFETY: encode_into initialized the first `n` bytes.
    assert_eq!(&BOXED_TXS[..], unsafe { buf[..n].assume_init_ref() });

    let mut buf = [MaybeUninit::<u8>::uninit(); 64];
    let n = cxs::encode_into(&BOXED, &mut buf).unwrap();
    // SAFETY: encode_into initialized the first `n` bytes.
    assert_eq!(&BOXED_CXS[..], unsafe { buf[..n].assume_init_ref() });
}
