//! Writing Rust values as XDR data.
//!
//! This module provides the [`WriteXDR`] trait and implementations for
//! converting Rust values into [XDR][RFC-4506]-encoded data.
//!
//! [RFC-4506]: https://datatracker.ietf.org/doc/html/rfc4506

use crate::serialize::writer::*;
use crate::valid_u32::*;
use core::convert::Infallible;

/// Types that can be written as XDR data.
///
/// Types implementing this trait can be serialized into XDR format
/// using any writer that implements [`Writer`].
///
/// # Example
///
/// ```rust
/// use xdrs::cxs;
/// use core::mem::MaybeUninit;
///
/// let value: u32 = 42;
/// let mut buf = [MaybeUninit::uninit(); 16];
/// let written = cxs::encode_into(&value, &mut buf)?;
/// # Ok::<(), xdrs::WriteError>(())
/// ```
pub trait WriteXDR {
    /// Writes this value as XDR data to the writer.
    ///
    /// This method should write the appropriate XDR-encoded representation
    /// of `self` to the writer.
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()>;
}

/// Writes an XDR value to the writer.
///
/// This is a convenience function that calls [`WriteXDR::write_xdr`] on the value.
///
/// # Example
///
/// ```rust
/// use xdrs::cxs;
/// use core::mem::MaybeUninit;
///
/// let value: u32 = 42;
/// let mut buf = [MaybeUninit::uninit(); 16];
/// let written = cxs::encode_into(&value, &mut buf)?;
/// # Ok::<(), xdrs::WriteError>(())
/// ```
#[inline]
pub fn write_xdr<T, F, W>(this: &T, w: &mut W) -> WriteResult<()>
where
    T: WriteXDR,
    W: Writer<F>,
{
    this.write_xdr(w)
}

macro_rules! write_xdr_int_impls {
    { $( $write_method:ident for $int_ty:ty ),* $(,)? } => { $(

        impl WriteXDR for $int_ty {
            #[inline]
            fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
                w.$write_method(*self)
            }
        }

    )* };
}

write_xdr_int_impls! {
    write_i64 for i64,
    write_i32 for i32,
    write_i16 for i16,

    write_u64 for u64,
    write_u32 for u32,
    write_u16 for u16,

    write_bool for bool,
    write_f32 for f32,
    write_f64 for f64,
}

impl WriteXDR for [u8] {
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
        w.write_opaque_variable(self)
    }
}

impl WriteXDR for &[u8] {
    #[inline]
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
        w.write_opaque_variable(self)
    }
}

impl<const N: usize> WriteXDR for [u8; N] {
    #[inline]
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
        let () = ValidU32::<N>::OK;

        w.write_opaque_fixed(self)
    }
}

impl<T: WriteXDR, const N: usize> WriteXDR for [T; N] {
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
        let () = ValidU32::<N>::OK;

        for elt in self {
            elt.write_xdr(w)?;
        }
        Ok(())
    }
}

impl<T: WriteXDR> WriteXDR for [T] {
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
        if self.len() > u32::MAX as usize {
            return Err(WriteError::InvalidArrayLength);
        }
        let len = self.len() as u32;
        w.write_u32(len)?;

        for elt in self {
            elt.write_xdr(w)?;
        }
        Ok(())
    }
}

impl<T: WriteXDR> WriteXDR for &T {
    #[inline]
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
        (*self).write_xdr(w)
    }
}

impl<T: WriteXDR> WriteXDR for &[T] {
    #[inline(always)]
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
        <[T] as WriteXDR>::write_xdr(*self, w)
    }
}

impl WriteXDR for str {
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
        w.write_opaque_variable(self.as_bytes())
    }
}

impl WriteXDR for &str {
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
        w.write_opaque_variable((*self).as_bytes())
    }
}

#[cfg(feature = "alloc")]
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
impl WriteXDR for alloc::string::String {
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
        self.as_str().write_xdr(w)
    }
}

impl WriteXDR for () {
    fn write_xdr<F, W: Writer<F>>(&self, _w: &mut W) -> WriteResult<()> {
        Ok(())
    }
}

impl WriteXDR for Infallible {
    fn write_xdr<F, W: Writer<F>>(&self, _w: &mut W) -> WriteResult<()> {
        unreachable!()
    }
}

macro_rules! write_tuple {
    { $( ($v:ident, $t:ident) ),+ } => {

        impl<$($t),+> WriteXDR for ($($t),+)
        where
            $( $t: WriteXDR, )+
        {
            fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
                let ( $($v),+ ) = self;
                $( write_xdr($v, w)?; )+
                Ok(())
            }
        }

    };
}

write_tuple! { (t0, T0), (t1, T1) }
write_tuple! { (t0, T0), (t1, T1), (t2, T2) }
write_tuple! { (t0, T0), (t1, T1), (t2, T2), (t3, T3) }
write_tuple! { (t0, T0), (t1, T1), (t2, T2), (t3, T3), (t4, T4) }
write_tuple! { (t0, T0), (t1, T1), (t2, T2), (t3, T3), (t4, T4), (t5, T5) }
write_tuple! { (t0, T0), (t1, T1), (t2, T2), (t3, T3), (t4, T4), (t5, T5), (t6, T6) }
write_tuple! { (t0, T0), (t1, T1), (t2, T2), (t3, T3), (t4, T4), (t5, T5), (t6, T6), (t7, T7) }
write_tuple! { (t0, T0), (t1, T1), (t2, T2), (t3, T3), (t4, T4), (t5, T5), (t6, T6), (t7, T7), (t8, T8) }
write_tuple! { (t0, T0), (t1, T1), (t2, T2), (t3, T3), (t4, T4), (t5, T5), (t6, T6), (t7, T7), (t8, T8), (t9, T9) }
write_tuple! { (t0, T0), (t1, T1), (t2, T2), (t3, T3), (t4, T4), (t5, T5), (t6, T6), (t7, T7), (t8, T8), (t9, T9), (t10, T10) }
write_tuple! { (t0, T0), (t1, T1), (t2, T2), (t3, T3), (t4, T4), (t5, T5), (t6, T6), (t7, T7), (t8, T8), (t9, T9), (t10, T10), (t11, T11) }
write_tuple! { (t0, T0), (t1, T1), (t2, T2), (t3, T3), (t4, T4), (t5, T5), (t6, T6), (t7, T7), (t8, T8), (t9, T9), (t10, T10), (t11, T11), (t12, T12) }
write_tuple! { (t0, T0), (t1, T1), (t2, T2), (t3, T3), (t4, T4), (t5, T5), (t6, T6), (t7, T7), (t8, T8), (t9, T9), (t10, T10), (t11, T11), (t12, T12), (t13, T13) }
write_tuple! { (t0, T0), (t1, T1), (t2, T2), (t3, T3), (t4, T4), (t5, T5), (t6, T6), (t7, T7), (t8, T8), (t9, T9), (t10, T10), (t11, T11), (t12, T12), (t13, T13), (t14, T14) }
write_tuple! { (t0, T0), (t1, T1), (t2, T2), (t3, T3), (t4, T4), (t5, T5), (t6, T6), (t7, T7), (t8, T8), (t9, T9), (t10, T10), (t11, T11), (t12, T12), (t13, T13), (t14, T14), (t15, T15) }

// impl for single-element tuples
impl<T: WriteXDR> WriteXDR for (T,) {
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
        let (t,) = self;
        t.write_xdr(w)
    }
}

impl<T: WriteXDR> WriteXDR for Option<T> {
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
        if let Some(val) = self {
            w.write_i32(1)?;
            val.write_xdr(w)
        } else {
            w.write_i32(0)
        }
    }
}

const _: () = {
    use super::*;
    use crate::serialize::writer::Writer;
    use xdr_common::*;

    impl<const MAX: u32> WriteXDR for BytesM<MAX> {
        fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
            self.0.write_xdr(w)
        }
    }

    // Make VecM<u8> work like BytesM;
    #[cfg(feature = "alloc")]
    impl<const MAX: u32> WriteXDR for VecM<u8, MAX> {
        fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
            self.0.write_xdr(w)
        }
    }
    #[cfg(not(feature = "alloc"))]
    impl<const MAX: u32> WriteXDR for VecM<u8, MAX> {
        fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
            <Self as AsRef<[u8]>>::as_ref(self).write_xdr(w)
        }
    }

    impl<const MAX: u32> WriteXDR for StringM<MAX> {
        fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
            self.0.write_xdr(w)
        }
    }

    #[cfg(feature = "alloc")]
    impl<const MAX: u32, T> WriteXDR for VecM<T, MAX>
    where
        T: WriteXDR,
    {
        fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
            <Self as AsRef<[T]>>::as_ref(self).write_xdr(w)
        }
    }

    #[cfg(not(feature = "alloc"))]
    impl<const MAX: u32, T> WriteXDR for VecM<T, MAX>
    where
        T: WriteXDR + 'static,
    {
        fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
            <Self as AsRef<[T]>>::as_ref(self).write_xdr(w)
        }
    }

    #[cfg(feature = "alloc")]
    #[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
    impl<T: WriteXDR> WriteXDR for xdr_common::Box<T> {
        fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
            self.as_ref().write_xdr(w)
        }
    }
};

impl<T, E> WriteXDR for Result<T, E>
where
    T: WriteXDR,
    E: WriteXDR,
{
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
        match self {
            Ok(x) => {
                w.write_i32(0)?;
                x.write_xdr(w)
            }
            Err(e) => {
                w.write_i32(1)?;
                e.write_xdr(w)
            }
        }
    }
}

impl<T> WriteXDR for core::marker::PhantomData<T> {
    fn write_xdr<F, W: Writer<F>>(&self, _w: &mut W) -> WriteResult<()> {
        Ok(())
    }
}

#[cfg(feature = "alloc")]
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
const _: () = {
    use alloc::vec::Vec;

    impl WriteXDR for Vec<u8> {
        fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
            w.write_opaque_variable(self.as_ref())
        }
    }

    impl<T> WriteXDR for Vec<T>
    where
        T: WriteXDR,
    {
        fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
            let slice: &[T] = self.as_ref();
            slice.write_xdr(w)
        }
    }
};
