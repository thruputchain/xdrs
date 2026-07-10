//! Worst-case size bounds for heapless containers.
//!
//! heapless stores elements inline (no heap indirection), so unlike
//! `VecM<T, MAX>` these bounds are exact: naming the element's bound cannot
//! form a const-eval cycle, by the same argument as arrays.
#![cfg(all(feature = "heapless", feature = "derive"))]
#![allow(dead_code)]

use core::mem::MaybeUninit;
use heapless::{String, Vec};
use xdrs::VecM;

const fn cxs_max<T: xdrs::cxs::ConstMaxSize>() -> Option<usize> {
    T::MAX_SIZE
}

const fn txs_max<T: xdrs::txs::ConstMaxSize>() -> Option<usize> {
    T::MAX_SIZE
}

// Byte vectors and strings: opaque-variable bounds.
// CXS: size_u32(5) + 5 = 6. TXS: 4 + pad4(5) = 12.
const _: () = assert!(matches!(cxs_max::<Vec<u8, 5>>(), Some(6)));
const _: () = assert!(matches!(txs_max::<Vec<u8, 5>>(), Some(12)));
// CXS: size_u32(8) + 8 = 9. TXS: 4 + 8 = 12.
const _: () = assert!(matches!(cxs_max::<String<8>>(), Some(9)));
const _: () = assert!(matches!(txs_max::<String<8>>(), Some(12)));

// Element vectors: length prefix + N * element bound — exact, unlike VecM.
// CXS: size_u32(5) + 5 * max_size_u32() = 1 + 25. TXS: 4 + 5 * 4.
const _: () = assert!(matches!(cxs_max::<Vec<u32, 5>>(), Some(26)));
const _: () = assert!(matches!(txs_max::<Vec<u32, 5>>(), Some(24)));

// Nested by-value shapes fold through.
// CXS: 1 + 2 * opaque_fixed(6) = 13. TXS: 4 + 2 * pad4(6) = 20.
const _: () = assert!(matches!(cxs_max::<Vec<[u8; 6], 2>>(), Some(13)));
const _: () = assert!(matches!(txs_max::<Vec<[u8; 6], 2>>(), Some(20)));
// heapless inside heapless: CXS 1 + 2 * 6 = 13. TXS 4 + 2 * 12 = 28.
const _: () = assert!(matches!(cxs_max::<Vec<Vec<u8, 5>, 2>>(), Some(13)));
const _: () = assert!(matches!(txs_max::<Vec<Vec<u8, 5>, 2>>(), Some(28)));

// A VecM element is still a pointer cut: the None propagates.
const _: () = assert!(cxs_max::<Vec<VecM<u32, 3>, 2>>().is_none());
const _: () = assert!(txs_max::<Vec<VecM<u32, 3>, 2>>().is_none());

// A capacity that cannot fit a u32 length prefix has no bound.
#[cfg(target_pointer_width = "64")]
const _: () = assert!(cxs_max::<Vec<u8, { u32::MAX as usize + 1 }>>().is_none());
#[cfg(target_pointer_width = "64")]
const _: () = assert!(txs_max::<Vec<u8, { u32::MAX as usize + 1 }>>().is_none());

// -- The headline use case: derive bounds for a struct with heapless fields
// and size a buffer from them in a const context --

#[derive(xdrs::WriteXDR, xdrs::ConstXDRSize)]
struct Frame {
    payload: Vec<u8, 5>,
    kind: u32,
}

// CXS: 6 + 5 = 11. TXS: 12 + 4 = 16.
const _: () = assert!(matches!(cxs_max::<Frame>(), Some(11)));
const _: () = assert!(matches!(txs_max::<Frame>(), Some(16)));

const FRAME_CXS_BOUND: usize = match cxs_max::<Frame>() {
    Some(n) => n,
    None => panic!("unbounded"),
};

#[derive(xdrs::WriteXDR, xdrs::ConstXDRSize)]
#[repr(i32)]
enum Msg {
    Ping,
    Data(Vec<u8, 5>),
}

// CXS: max(size_i32(0), size_i32(1) + 6) = 7. TXS: max(4, 4 + 12) = 16.
const _: () = assert!(matches!(cxs_max::<Msg>(), Some(7)));
const _: () = assert!(matches!(txs_max::<Msg>(), Some(16)));

// -- Recursion through a heapless Vec --
//
// By-value recursion through `Vec<T, N>` is unrepresentable (inline storage
// would make the type infinitely sized), so naming the element's bound is
// safe. Actual recursion must pass through indirection, and the blanket
// `&T → None` cut ends the fold without ever evaluating the pointee's
// bound — the type derives cleanly and reports `None`.

#[derive(xdrs::ConstXDRSize)]
struct TreeNode<'a> {
    val: u32,
    kids: Vec<&'a TreeNode<'a>, 4>,
}

const _: () = assert!(cxs_max::<TreeNode<'static>>().is_none());
const _: () = assert!(txs_max::<TreeNode<'static>>().is_none());

#[test]
fn runtime_encodings_fit_the_bounds() {
    // Worst-case frame: full payload, largest kind. CXS hits the bound
    // exactly: size_u32(5)=1 + 5 bytes + size_u32(u32::MAX)=5.
    let frame = Frame {
        payload: Vec::try_from(&[0xFF; 5][..]).unwrap(),
        kind: u32::MAX,
    };

    let mut buf = [MaybeUninit::uninit(); FRAME_CXS_BOUND];
    let written = xdrs::cxs::encode_into(&frame, &mut buf).unwrap();
    assert_eq!(written, FRAME_CXS_BOUND);

    // A small frame fits within the same worst-case buffer.
    let small = Frame {
        payload: Vec::new(),
        kind: 0,
    };
    let mut buf = [MaybeUninit::uninit(); FRAME_CXS_BOUND];
    let written = xdrs::cxs::encode_into(&small, &mut buf).unwrap();
    assert!(written <= FRAME_CXS_BOUND);
}
