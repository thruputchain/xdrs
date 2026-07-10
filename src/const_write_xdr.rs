//! The infallible cursor-based encoding trait.
//!
//! [`ConstWriteXDR`] is the runtime companion to the inherent `const fn`
//! encoders emitted by `#[derive(ConstWriteXDR)]`: non-generic methods over
//! [`cxs::Cursor`] / [`txs::Cursor`]
//! that panic on buffer overflow instead of returning `Result`, plus
//! worst-case encoded sizes for compile-time buffer sizing.

use crate::encoded_max;
use crate::valid_u32::ValidU32;
use crate::{cxs, txs};
use core::convert::Infallible;
use core::marker::PhantomData;

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

impl ConstWriteXDR for [u8] {
    const MAX_CXS_LEN: Option<usize> = None;
    const MAX_TXS_LEN: Option<usize> = None;
    fn write_cxs(&self, w: &mut cxs::Cursor<'_>) {
        w.write_opaque_variable(self)
    }
    fn write_txs(&self, w: &mut txs::Cursor<'_>) {
        w.write_opaque_variable(self)
    }
    fn cxs_size(&self) -> usize {
        cxs::const_writer::size_opaque_variable(self.len())
    }
    fn txs_size(&self) -> usize {
        txs::const_writer::size_opaque_variable(self.len())
    }
}

impl ConstWriteXDR for &[u8] {
    const MAX_CXS_LEN: Option<usize> = None;
    const MAX_TXS_LEN: Option<usize> = None;
    #[inline]
    fn write_cxs(&self, w: &mut cxs::Cursor<'_>) {
        w.write_opaque_variable(self)
    }
    #[inline]
    fn write_txs(&self, w: &mut txs::Cursor<'_>) {
        w.write_opaque_variable(self)
    }
    #[inline]
    fn cxs_size(&self) -> usize {
        cxs::const_writer::size_opaque_variable(self.len())
    }
    #[inline]
    fn txs_size(&self) -> usize {
        txs::const_writer::size_opaque_variable(self.len())
    }
}

impl<const N: usize> ConstWriteXDR for [u8; N] {
    const MAX_CXS_LEN: Option<usize> = Some(cxs::const_writer::size_opaque_fixed(N));
    const MAX_TXS_LEN: Option<usize> = Some(txs::const_writer::size_opaque_fixed(N));
    #[inline]
    fn write_cxs(&self, w: &mut cxs::Cursor<'_>) {
        let () = ValidU32::<N>::OK;
        w.write_opaque_fixed(self)
    }
    #[inline]
    fn write_txs(&self, w: &mut txs::Cursor<'_>) {
        let () = ValidU32::<N>::OK;
        w.write_opaque_fixed(self)
    }
    #[inline]
    fn cxs_size(&self) -> usize {
        cxs::const_writer::size_opaque_fixed(N)
    }
    #[inline]
    fn txs_size(&self) -> usize {
        txs::const_writer::size_opaque_fixed(N)
    }
}

impl<T: ConstWriteXDR, const N: usize> ConstWriteXDR for [T; N] {
    const MAX_CXS_LEN: Option<usize> = encoded_max::mul(N, T::MAX_CXS_LEN);
    const MAX_TXS_LEN: Option<usize> = encoded_max::mul(N, T::MAX_TXS_LEN);
    fn write_cxs(&self, w: &mut cxs::Cursor<'_>) {
        let () = ValidU32::<N>::OK;
        let mut i = 0;
        while i < N {
            self[i].write_cxs(w);
            i += 1;
        }
    }
    fn write_txs(&self, w: &mut txs::Cursor<'_>) {
        let () = ValidU32::<N>::OK;
        let mut i = 0;
        while i < N {
            self[i].write_txs(w);
            i += 1;
        }
    }
    fn cxs_size(&self) -> usize {
        let mut s = 0;
        let mut i = 0;
        while i < N {
            s += self[i].cxs_size();
            i += 1;
        }
        s
    }
    fn txs_size(&self) -> usize {
        let mut s = 0;
        let mut i = 0;
        while i < N {
            s += self[i].txs_size();
            i += 1;
        }
        s
    }
}

impl<T: ConstWriteXDR> ConstWriteXDR for [T] {
    const MAX_CXS_LEN: Option<usize> = None;
    const MAX_TXS_LEN: Option<usize> = None;
    fn write_cxs(&self, w: &mut cxs::Cursor<'_>) {
        assert!(
            self.len() <= u32::MAX as usize,
            "array length exceeds u32::MAX"
        );
        w.write_u32(self.len() as u32);
        for elt in self {
            elt.write_cxs(w);
        }
    }
    fn write_txs(&self, w: &mut txs::Cursor<'_>) {
        assert!(
            self.len() <= u32::MAX as usize,
            "array length exceeds u32::MAX"
        );
        w.write_u32(self.len() as u32);
        for elt in self {
            elt.write_txs(w);
        }
    }
    fn cxs_size(&self) -> usize {
        let mut s = cxs::const_writer::size_u32(self.len() as u32);
        for elt in self {
            s += elt.cxs_size();
        }
        s
    }
    fn txs_size(&self) -> usize {
        let mut s = 4;
        for elt in self {
            s += elt.txs_size();
        }
        s
    }
}

impl<T: ConstWriteXDR> ConstWriteXDR for &T {
    const MAX_CXS_LEN: Option<usize> = T::MAX_CXS_LEN;
    const MAX_TXS_LEN: Option<usize> = T::MAX_TXS_LEN;
    #[inline]
    fn write_cxs(&self, w: &mut cxs::Cursor<'_>) {
        (*self).write_cxs(w)
    }
    #[inline]
    fn write_txs(&self, w: &mut txs::Cursor<'_>) {
        (*self).write_txs(w)
    }
    #[inline]
    fn cxs_size(&self) -> usize {
        (*self).cxs_size()
    }
    #[inline]
    fn txs_size(&self) -> usize {
        (*self).txs_size()
    }
}

impl<T: ConstWriteXDR> ConstWriteXDR for &mut T {
    const MAX_CXS_LEN: Option<usize> = T::MAX_CXS_LEN;
    const MAX_TXS_LEN: Option<usize> = T::MAX_TXS_LEN;
    #[inline]
    fn write_cxs(&self, w: &mut cxs::Cursor<'_>) {
        (**self).write_cxs(w)
    }
    #[inline]
    fn write_txs(&self, w: &mut txs::Cursor<'_>) {
        (**self).write_txs(w)
    }
    #[inline]
    fn cxs_size(&self) -> usize {
        (**self).cxs_size()
    }
    #[inline]
    fn txs_size(&self) -> usize {
        (**self).txs_size()
    }
}

impl<T: ConstWriteXDR> ConstWriteXDR for &[T] {
    const MAX_CXS_LEN: Option<usize> = None;
    const MAX_TXS_LEN: Option<usize> = None;
    #[inline(always)]
    fn write_cxs(&self, w: &mut cxs::Cursor<'_>) {
        <[T] as ConstWriteXDR>::write_cxs(*self, w)
    }
    #[inline(always)]
    fn write_txs(&self, w: &mut txs::Cursor<'_>) {
        <[T] as ConstWriteXDR>::write_txs(*self, w)
    }
    #[inline(always)]
    fn cxs_size(&self) -> usize {
        <[T] as ConstWriteXDR>::cxs_size(*self)
    }
    #[inline(always)]
    fn txs_size(&self) -> usize {
        <[T] as ConstWriteXDR>::txs_size(*self)
    }
}

impl ConstWriteXDR for str {
    const MAX_CXS_LEN: Option<usize> = None;
    const MAX_TXS_LEN: Option<usize> = None;
    fn write_cxs(&self, w: &mut cxs::Cursor<'_>) {
        w.write_opaque_variable(self.as_bytes())
    }
    fn write_txs(&self, w: &mut txs::Cursor<'_>) {
        w.write_opaque_variable(self.as_bytes())
    }
    fn cxs_size(&self) -> usize {
        cxs::const_writer::size_opaque_variable(self.len())
    }
    fn txs_size(&self) -> usize {
        txs::const_writer::size_opaque_variable(self.len())
    }
}

impl ConstWriteXDR for &str {
    const MAX_CXS_LEN: Option<usize> = None;
    const MAX_TXS_LEN: Option<usize> = None;
    fn write_cxs(&self, w: &mut cxs::Cursor<'_>) {
        w.write_opaque_variable((*self).as_bytes())
    }
    fn write_txs(&self, w: &mut txs::Cursor<'_>) {
        w.write_opaque_variable((*self).as_bytes())
    }
    fn cxs_size(&self) -> usize {
        cxs::const_writer::size_opaque_variable((*self).len())
    }
    fn txs_size(&self) -> usize {
        txs::const_writer::size_opaque_variable((*self).len())
    }
}

#[cfg(feature = "alloc")]
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
impl ConstWriteXDR for alloc::string::String {
    const MAX_CXS_LEN: Option<usize> = None;
    const MAX_TXS_LEN: Option<usize> = None;
    fn write_cxs(&self, w: &mut cxs::Cursor<'_>) {
        self.as_str().write_cxs(w)
    }
    fn write_txs(&self, w: &mut txs::Cursor<'_>) {
        self.as_str().write_txs(w)
    }
    fn cxs_size(&self) -> usize {
        self.as_str().cxs_size()
    }
    fn txs_size(&self) -> usize {
        self.as_str().txs_size()
    }
}

impl ConstWriteXDR for () {
    const MAX_CXS_LEN: Option<usize> = Some(0);
    const MAX_TXS_LEN: Option<usize> = Some(0);
    fn write_cxs(&self, _w: &mut cxs::Cursor<'_>) {}
    fn write_txs(&self, _w: &mut txs::Cursor<'_>) {}
    fn cxs_size(&self) -> usize {
        0
    }
    fn txs_size(&self) -> usize {
        0
    }
}

impl ConstWriteXDR for Infallible {
    const MAX_CXS_LEN: Option<usize> = Some(0);
    const MAX_TXS_LEN: Option<usize> = Some(0);
    fn write_cxs(&self, _w: &mut cxs::Cursor<'_>) {
        match *self {}
    }
    fn write_txs(&self, _w: &mut txs::Cursor<'_>) {
        match *self {}
    }
    fn cxs_size(&self) -> usize {
        match *self {}
    }
    fn txs_size(&self) -> usize {
        match *self {}
    }
}

macro_rules! const_write_tuple {
    { $( ($v:ident, $t:ident) ),+ } => {
        impl<$($t),+> ConstWriteXDR for ($($t),+)
        where
            $( $t: ConstWriteXDR, )+
        {
            const MAX_CXS_LEN: Option<usize> = {
                let mut acc: Option<usize> = Some(0);
                $( acc = encoded_max::add(acc, <$t as ConstWriteXDR>::MAX_CXS_LEN); )+
                acc
            };
            const MAX_TXS_LEN: Option<usize> = {
                let mut acc: Option<usize> = Some(0);
                $( acc = encoded_max::add(acc, <$t as ConstWriteXDR>::MAX_TXS_LEN); )+
                acc
            };
            fn write_cxs(&self, w: &mut cxs::Cursor<'_>) {
                let ( $($v),+ ) = self;
                $( $v.write_cxs(w); )+
            }
            fn write_txs(&self, w: &mut txs::Cursor<'_>) {
                let ( $($v),+ ) = self;
                $( $v.write_txs(w); )+
            }
            fn cxs_size(&self) -> usize {
                let ( $($v),+ ) = self;
                0 $( + $v.cxs_size() )+
            }
            fn txs_size(&self) -> usize {
                let ( $($v),+ ) = self;
                0 $( + $v.txs_size() )+
            }
        }
    };
}

const_write_tuple! { (t0, T0), (t1, T1) }
const_write_tuple! { (t0, T0), (t1, T1), (t2, T2) }
const_write_tuple! { (t0, T0), (t1, T1), (t2, T2), (t3, T3) }
const_write_tuple! { (t0, T0), (t1, T1), (t2, T2), (t3, T3), (t4, T4) }
const_write_tuple! { (t0, T0), (t1, T1), (t2, T2), (t3, T3), (t4, T4), (t5, T5) }
const_write_tuple! { (t0, T0), (t1, T1), (t2, T2), (t3, T3), (t4, T4), (t5, T5), (t6, T6) }
const_write_tuple! { (t0, T0), (t1, T1), (t2, T2), (t3, T3), (t4, T4), (t5, T5), (t6, T6), (t7, T7) }
const_write_tuple! { (t0, T0), (t1, T1), (t2, T2), (t3, T3), (t4, T4), (t5, T5), (t6, T6), (t7, T7), (t8, T8) }
const_write_tuple! { (t0, T0), (t1, T1), (t2, T2), (t3, T3), (t4, T4), (t5, T5), (t6, T6), (t7, T7), (t8, T8), (t9, T9) }
const_write_tuple! { (t0, T0), (t1, T1), (t2, T2), (t3, T3), (t4, T4), (t5, T5), (t6, T6), (t7, T7), (t8, T8), (t9, T9), (t10, T10) }
const_write_tuple! { (t0, T0), (t1, T1), (t2, T2), (t3, T3), (t4, T4), (t5, T5), (t6, T6), (t7, T7), (t8, T8), (t9, T9), (t10, T10), (t11, T11) }
const_write_tuple! { (t0, T0), (t1, T1), (t2, T2), (t3, T3), (t4, T4), (t5, T5), (t6, T6), (t7, T7), (t8, T8), (t9, T9), (t10, T10), (t11, T11), (t12, T12) }
const_write_tuple! { (t0, T0), (t1, T1), (t2, T2), (t3, T3), (t4, T4), (t5, T5), (t6, T6), (t7, T7), (t8, T8), (t9, T9), (t10, T10), (t11, T11), (t12, T12), (t13, T13) }
const_write_tuple! { (t0, T0), (t1, T1), (t2, T2), (t3, T3), (t4, T4), (t5, T5), (t6, T6), (t7, T7), (t8, T8), (t9, T9), (t10, T10), (t11, T11), (t12, T12), (t13, T13), (t14, T14) }
const_write_tuple! { (t0, T0), (t1, T1), (t2, T2), (t3, T3), (t4, T4), (t5, T5), (t6, T6), (t7, T7), (t8, T8), (t9, T9), (t10, T10), (t11, T11), (t12, T12), (t13, T13), (t14, T14), (t15, T15) }

impl<T: ConstWriteXDR> ConstWriteXDR for (T,) {
    const MAX_CXS_LEN: Option<usize> = T::MAX_CXS_LEN;
    const MAX_TXS_LEN: Option<usize> = T::MAX_TXS_LEN;
    fn write_cxs(&self, w: &mut cxs::Cursor<'_>) {
        let (t,) = self;
        t.write_cxs(w);
    }
    fn write_txs(&self, w: &mut txs::Cursor<'_>) {
        let (t,) = self;
        t.write_txs(w);
    }
    fn cxs_size(&self) -> usize {
        let (t,) = self;
        t.cxs_size()
    }
    fn txs_size(&self) -> usize {
        let (t,) = self;
        t.txs_size()
    }
}

impl<T: ConstWriteXDR> ConstWriteXDR for Option<T> {
    // CXS discriminant (i32 0|1) encodes in 1 byte; TXS in 4.
    const MAX_CXS_LEN: Option<usize> = encoded_max::add(Some(1), T::MAX_CXS_LEN);
    const MAX_TXS_LEN: Option<usize> = encoded_max::add(Some(4), T::MAX_TXS_LEN);
    fn write_cxs(&self, w: &mut cxs::Cursor<'_>) {
        match self {
            Some(v) => {
                w.write_i32(1);
                v.write_cxs(w);
            }
            None => w.write_i32(0),
        }
    }
    fn write_txs(&self, w: &mut txs::Cursor<'_>) {
        match self {
            Some(v) => {
                w.write_i32(1);
                v.write_txs(w);
            }
            None => w.write_i32(0),
        }
    }
    fn cxs_size(&self) -> usize {
        match self {
            Some(v) => cxs::const_writer::size_i32(1) + v.cxs_size(),
            None => cxs::const_writer::size_i32(0),
        }
    }
    fn txs_size(&self) -> usize {
        match self {
            Some(v) => 4 + v.txs_size(),
            None => 4,
        }
    }
}

impl<T, E> ConstWriteXDR for Result<T, E>
where
    T: ConstWriteXDR,
    E: ConstWriteXDR,
{
    const MAX_CXS_LEN: Option<usize> =
        encoded_max::add(Some(1), encoded_max::max(T::MAX_CXS_LEN, E::MAX_CXS_LEN));
    const MAX_TXS_LEN: Option<usize> =
        encoded_max::add(Some(4), encoded_max::max(T::MAX_TXS_LEN, E::MAX_TXS_LEN));
    fn write_cxs(&self, w: &mut cxs::Cursor<'_>) {
        match self {
            Ok(x) => {
                w.write_i32(0);
                x.write_cxs(w);
            }
            Err(e) => {
                w.write_i32(1);
                e.write_cxs(w);
            }
        }
    }
    fn write_txs(&self, w: &mut txs::Cursor<'_>) {
        match self {
            Ok(x) => {
                w.write_i32(0);
                x.write_txs(w);
            }
            Err(e) => {
                w.write_i32(1);
                e.write_txs(w);
            }
        }
    }
    fn cxs_size(&self) -> usize {
        match self {
            Ok(x) => cxs::const_writer::size_i32(0) + x.cxs_size(),
            Err(e) => cxs::const_writer::size_i32(1) + e.cxs_size(),
        }
    }
    fn txs_size(&self) -> usize {
        match self {
            Ok(x) => 4 + x.txs_size(),
            Err(e) => 4 + e.txs_size(),
        }
    }
}

impl<T> ConstWriteXDR for PhantomData<T> {
    const MAX_CXS_LEN: Option<usize> = Some(0);
    const MAX_TXS_LEN: Option<usize> = Some(0);
    fn write_cxs(&self, _w: &mut cxs::Cursor<'_>) {}
    fn write_txs(&self, _w: &mut txs::Cursor<'_>) {}
    fn cxs_size(&self) -> usize {
        0
    }
    fn txs_size(&self) -> usize {
        0
    }
}

const _: () = {
    use super::*;
    use xdr_common::*;

    impl<const MAX: u32> ConstWriteXDR for BytesM<MAX> {
        const MAX_CXS_LEN: Option<usize> =
            encoded_max::add(Some(cxs::const_writer::size_u32(MAX)), Some(MAX as usize));
        const MAX_TXS_LEN: Option<usize> =
            Some(4 + txs::const_writer::size_opaque_fixed(MAX as usize));
        fn write_cxs(&self, w: &mut cxs::Cursor<'_>) {
            let bytes: &[u8] = <Self as AsRef<[u8]>>::as_ref(self);
            assert!(
                bytes.len() <= MAX as usize,
                "value length exceeds declared bound"
            );
            w.write_opaque_variable(bytes);
        }
        fn write_txs(&self, w: &mut txs::Cursor<'_>) {
            let bytes: &[u8] = <Self as AsRef<[u8]>>::as_ref(self);
            assert!(
                bytes.len() <= MAX as usize,
                "value length exceeds declared bound"
            );
            w.write_opaque_variable(bytes);
        }
        fn cxs_size(&self) -> usize {
            cxs::const_writer::size_opaque_variable(<Self as AsRef<[u8]>>::as_ref(self).len())
        }
        fn txs_size(&self) -> usize {
            txs::const_writer::size_opaque_variable(<Self as AsRef<[u8]>>::as_ref(self).len())
        }
    }

    // Make VecM<u8> work like BytesM.
    #[cfg(feature = "alloc")]
    impl<const MAX: u32> ConstWriteXDR for VecM<u8, MAX> {
        const MAX_CXS_LEN: Option<usize> =
            encoded_max::add(Some(cxs::const_writer::size_u32(MAX)), Some(MAX as usize));
        const MAX_TXS_LEN: Option<usize> =
            Some(4 + txs::const_writer::size_opaque_fixed(MAX as usize));
        fn write_cxs(&self, w: &mut cxs::Cursor<'_>) {
            let bytes: &[u8] = <Self as AsRef<[u8]>>::as_ref(self);
            assert!(
                bytes.len() <= MAX as usize,
                "value length exceeds declared bound"
            );
            w.write_opaque_variable(bytes);
        }
        fn write_txs(&self, w: &mut txs::Cursor<'_>) {
            let bytes: &[u8] = <Self as AsRef<[u8]>>::as_ref(self);
            assert!(
                bytes.len() <= MAX as usize,
                "value length exceeds declared bound"
            );
            w.write_opaque_variable(bytes);
        }
        fn cxs_size(&self) -> usize {
            cxs::const_writer::size_opaque_variable(<Self as AsRef<[u8]>>::as_ref(self).len())
        }
        fn txs_size(&self) -> usize {
            txs::const_writer::size_opaque_variable(<Self as AsRef<[u8]>>::as_ref(self).len())
        }
    }
    #[cfg(not(feature = "alloc"))]
    impl<const MAX: u32> ConstWriteXDR for VecM<u8, MAX> {
        const MAX_CXS_LEN: Option<usize> =
            encoded_max::add(Some(cxs::const_writer::size_u32(MAX)), Some(MAX as usize));
        const MAX_TXS_LEN: Option<usize> =
            Some(4 + txs::const_writer::size_opaque_fixed(MAX as usize));
        fn write_cxs(&self, w: &mut cxs::Cursor<'_>) {
            let bytes: &[u8] = <Self as AsRef<[u8]>>::as_ref(self);
            assert!(
                bytes.len() <= MAX as usize,
                "value length exceeds declared bound"
            );
            w.write_opaque_variable(bytes);
        }
        fn write_txs(&self, w: &mut txs::Cursor<'_>) {
            let bytes: &[u8] = <Self as AsRef<[u8]>>::as_ref(self);
            assert!(
                bytes.len() <= MAX as usize,
                "value length exceeds declared bound"
            );
            w.write_opaque_variable(bytes);
        }
        fn cxs_size(&self) -> usize {
            cxs::const_writer::size_opaque_variable(<Self as AsRef<[u8]>>::as_ref(self).len())
        }
        fn txs_size(&self) -> usize {
            txs::const_writer::size_opaque_variable(<Self as AsRef<[u8]>>::as_ref(self).len())
        }
    }

    impl<const MAX: u32> ConstWriteXDR for StringM<MAX> {
        const MAX_CXS_LEN: Option<usize> =
            encoded_max::add(Some(cxs::const_writer::size_u32(MAX)), Some(MAX as usize));
        const MAX_TXS_LEN: Option<usize> =
            Some(4 + txs::const_writer::size_opaque_fixed(MAX as usize));
        fn write_cxs(&self, w: &mut cxs::Cursor<'_>) {
            let bytes: &[u8] = <Self as AsRef<[u8]>>::as_ref(self);
            assert!(
                bytes.len() <= MAX as usize,
                "value length exceeds declared bound"
            );
            w.write_opaque_variable(bytes);
        }
        fn write_txs(&self, w: &mut txs::Cursor<'_>) {
            let bytes: &[u8] = <Self as AsRef<[u8]>>::as_ref(self);
            assert!(
                bytes.len() <= MAX as usize,
                "value length exceeds declared bound"
            );
            w.write_opaque_variable(bytes);
        }
        fn cxs_size(&self) -> usize {
            cxs::const_writer::size_opaque_variable(<Self as AsRef<[u8]>>::as_ref(self).len())
        }
        fn txs_size(&self) -> usize {
            txs::const_writer::size_opaque_variable(<Self as AsRef<[u8]>>::as_ref(self).len())
        }
    }

    #[cfg(feature = "alloc")]
    impl<const MAX: u32, T> ConstWriteXDR for VecM<T, MAX>
    where
        T: ConstWriteXDR,
    {
        const MAX_CXS_LEN: Option<usize> = encoded_max::add(
            Some(cxs::const_writer::size_u32(MAX)),
            encoded_max::mul(MAX as usize, T::MAX_CXS_LEN),
        );
        const MAX_TXS_LEN: Option<usize> =
            encoded_max::add(Some(4), encoded_max::mul(MAX as usize, T::MAX_TXS_LEN));
        fn write_cxs(&self, w: &mut cxs::Cursor<'_>) {
            let slice: &[T] = <Self as AsRef<[T]>>::as_ref(self);
            assert!(
                slice.len() <= MAX as usize,
                "value length exceeds declared bound"
            );
            <[T] as ConstWriteXDR>::write_cxs(slice, w);
        }
        fn write_txs(&self, w: &mut txs::Cursor<'_>) {
            let slice: &[T] = <Self as AsRef<[T]>>::as_ref(self);
            assert!(
                slice.len() <= MAX as usize,
                "value length exceeds declared bound"
            );
            <[T] as ConstWriteXDR>::write_txs(slice, w);
        }
        fn cxs_size(&self) -> usize {
            let slice: &[T] = <Self as AsRef<[T]>>::as_ref(self);
            <[T] as ConstWriteXDR>::cxs_size(slice)
        }
        fn txs_size(&self) -> usize {
            let slice: &[T] = <Self as AsRef<[T]>>::as_ref(self);
            <[T] as ConstWriteXDR>::txs_size(slice)
        }
    }

    #[cfg(not(feature = "alloc"))]
    impl<const MAX: u32, T> ConstWriteXDR for VecM<T, MAX>
    where
        T: ConstWriteXDR + 'static,
    {
        const MAX_CXS_LEN: Option<usize> = encoded_max::add(
            Some(cxs::const_writer::size_u32(MAX)),
            encoded_max::mul(MAX as usize, T::MAX_CXS_LEN),
        );
        const MAX_TXS_LEN: Option<usize> =
            encoded_max::add(Some(4), encoded_max::mul(MAX as usize, T::MAX_TXS_LEN));
        fn write_cxs(&self, w: &mut cxs::Cursor<'_>) {
            let slice: &[T] = <Self as AsRef<[T]>>::as_ref(self);
            assert!(
                slice.len() <= MAX as usize,
                "value length exceeds declared bound"
            );
            <[T] as ConstWriteXDR>::write_cxs(slice, w);
        }
        fn write_txs(&self, w: &mut txs::Cursor<'_>) {
            let slice: &[T] = <Self as AsRef<[T]>>::as_ref(self);
            assert!(
                slice.len() <= MAX as usize,
                "value length exceeds declared bound"
            );
            <[T] as ConstWriteXDR>::write_txs(slice, w);
        }
        fn cxs_size(&self) -> usize {
            let slice: &[T] = <Self as AsRef<[T]>>::as_ref(self);
            <[T] as ConstWriteXDR>::cxs_size(slice)
        }
        fn txs_size(&self) -> usize {
            let slice: &[T] = <Self as AsRef<[T]>>::as_ref(self);
            <[T] as ConstWriteXDR>::txs_size(slice)
        }
    }

    #[cfg(feature = "alloc")]
    #[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
    impl<T: ConstWriteXDR> ConstWriteXDR for xdr_common::Box<T> {
        const MAX_CXS_LEN: Option<usize> = T::MAX_CXS_LEN;
        const MAX_TXS_LEN: Option<usize> = T::MAX_TXS_LEN;
        fn write_cxs(&self, w: &mut cxs::Cursor<'_>) {
            self.as_ref().write_cxs(w)
        }
        fn write_txs(&self, w: &mut txs::Cursor<'_>) {
            self.as_ref().write_txs(w)
        }
        fn cxs_size(&self) -> usize {
            self.as_ref().cxs_size()
        }
        fn txs_size(&self) -> usize {
            self.as_ref().txs_size()
        }
    }
};

#[cfg(feature = "alloc")]
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
const _: () = {
    use alloc::vec::Vec;

    impl ConstWriteXDR for Vec<u8> {
        const MAX_CXS_LEN: Option<usize> = None;
        const MAX_TXS_LEN: Option<usize> = None;
        fn write_cxs(&self, w: &mut cxs::Cursor<'_>) {
            w.write_opaque_variable(self.as_ref())
        }
        fn write_txs(&self, w: &mut txs::Cursor<'_>) {
            w.write_opaque_variable(self.as_ref())
        }
        fn cxs_size(&self) -> usize {
            cxs::const_writer::size_opaque_variable(self.len())
        }
        fn txs_size(&self) -> usize {
            txs::const_writer::size_opaque_variable(self.len())
        }
    }

    impl<T> ConstWriteXDR for Vec<T>
    where
        T: ConstWriteXDR,
    {
        const MAX_CXS_LEN: Option<usize> = None;
        const MAX_TXS_LEN: Option<usize> = None;
        fn write_cxs(&self, w: &mut cxs::Cursor<'_>) {
            let slice: &[T] = self.as_ref();
            slice.write_cxs(w)
        }
        fn write_txs(&self, w: &mut txs::Cursor<'_>) {
            let slice: &[T] = self.as_ref();
            slice.write_txs(w)
        }
        fn cxs_size(&self) -> usize {
            let slice: &[T] = self.as_ref();
            slice.cxs_size()
        }
        fn txs_size(&self) -> usize {
            let slice: &[T] = self.as_ref();
            slice.txs_size()
        }
    }
};
