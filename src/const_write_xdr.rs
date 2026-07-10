//! The infallible cursor-based encoding trait.
//!
//! [`ConstWriteXDR`] is the runtime companion to the inherent `const fn`
//! encoders emitted by `#[derive(ConstWriteXDR)]`: non-generic methods over
//! [`cxs::Cursor`] / [`txs::Cursor`]
//! that panic on buffer overflow instead of returning `Result`, plus
//! worst-case encoded sizes for compile-time buffer sizing.

use crate::{cxs, txs};

/// Types with an infallible, cursor-based XDR encoding and a (possibly
/// unbounded) worst-case encoded size.
///
/// Implementations write through [`cxs::Cursor`] / [`txs::Cursor`], which
/// panic on buffer overflow. Size a buffer from `MAX_CXS_LEN` /
/// `MAX_TXS_LEN` (when `Some`) to make encoding infallible:
///
/// ```rust
/// use xdrs::ConstWriteXDR;
/// const N: usize = match <u64 as ConstWriteXDR>::MAX_CXS_LEN {
///     Some(n) => n,
///     None => panic!("unbounded"),
/// };
/// let mut buf = [0u8; N];
/// let mut c = xdrs::cxs::Cursor::new(&mut buf);
/// 42u64.write_cxs(&mut c);
/// assert_eq!(c.as_bytes(), &[42]);
/// ```
pub trait ConstWriteXDR {
    /// Worst-case CXS encoded size over all values of the type, or `None`
    /// if unbounded.
    const MAX_CXS_LEN: Option<usize>;
    /// Worst-case TXS encoded size over all values of the type, or `None`
    /// if unbounded.
    const MAX_TXS_LEN: Option<usize>;

    /// Writes this value in CXS format. Panics on cursor overflow.
    fn write_cxs(&self, w: &mut cxs::Cursor<'_>);
    /// Writes this value in TXS format. Panics on cursor overflow.
    fn write_txs(&self, w: &mut txs::Cursor<'_>);
    /// Exact CXS encoded size of this value in bytes.
    fn cxs_size(&self) -> usize;
    /// Exact TXS encoded size of this value in bytes.
    fn txs_size(&self) -> usize;
}

// NOTE: u8/i8 intentionally do NOT implement ConstWriteXDR, so that
// `[u8; N]`/`[u8]` can take the opaque-bytes fast path without overlapping
// the generic `[T; N]`/`[T]` impls (same structure as WriteXDR).
macro_rules! const_write_xdr_prim_impls {
    { $( ($ty:ty, $write_method:ident, $size_fn:ident, $cxs_max:literal, $txs_max:literal) ),* $(,)? } => { $(
        impl ConstWriteXDR for $ty {
            const MAX_CXS_LEN: Option<usize> = Some($cxs_max);
            const MAX_TXS_LEN: Option<usize> = Some($txs_max);
            #[inline]
            fn write_cxs(&self, w: &mut cxs::Cursor<'_>) { w.$write_method(*self) }
            #[inline]
            fn write_txs(&self, w: &mut txs::Cursor<'_>) { w.$write_method(*self) }
            #[inline]
            fn cxs_size(&self) -> usize { cxs::const_writer::$size_fn(*self) }
            #[inline]
            fn txs_size(&self) -> usize { txs::const_writer::$size_fn(*self) }
        }
    )* };
}

const_write_xdr_prim_impls! {
    (i16, write_i16, size_i16, 3, 4),
    (i32, write_i32, size_i32, 5, 4),
    (i64, write_i64, size_i64, 9, 8),
    (u16, write_u16, size_u16, 3, 4),
    (u32, write_u32, size_u32, 5, 4),
    (u64, write_u64, size_u64, 9, 8),
    (bool, write_bool, size_bool, 1, 4),
    (f32, write_f32, size_f32, 4, 4),
    (f64, write_f64, size_f64, 8, 8),
}
