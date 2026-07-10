//! Writing Rust values as XDR data.
//!
//! This module provides the [`WriteXDR`] trait and implementations for
//! converting Rust values into [XDR][RFC-4506]-encoded data.
//!
//! [RFC-4506]: https://datatracker.ietf.org/doc/html/rfc4506

use crate::serialize::writer::*;
use crate::serialize::writer_unchecked::*;
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

    /// Writes this value as XDR data to the writer, without runtime checks.
    ///
    /// This is the infallible, unsafe counterpart to
    /// [`write_xdr`](Self::write_xdr), for use with
    /// [`WriterUnchecked`] writers on code-size-sensitive targets.
    ///
    /// # Safety
    ///
    /// - The writer's underlying buffer must have enough space remaining
    ///   for the encoded form of `self`.
    /// - `self` must respect all declared XDR bounds (e.g. a
    ///   `VecM<T, MAX>` must not be longer than `MAX`), since a violation
    ///   would make the encoding exceed any precomputed size bound.
    ///
    /// Violations are `debug_assert!`ed; in release builds they are
    /// undefined behavior.
    unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W);
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

/// Writes an XDR value to the writer, without runtime checks.
///
/// This is a convenience function that calls
/// [`WriteXDR::write_xdr_unchecked`] on the value.
///
/// # Safety
///
/// Same contract as [`WriteXDR::write_xdr_unchecked`]: the writer must have
/// space for the encoded form of `this`, and `this` must respect all
/// declared XDR bounds.
#[inline]
pub unsafe fn write_xdr_unchecked<T, F, W>(this: &T, w: &mut W)
where
    T: WriteXDR,
    W: WriterUnchecked<F>,
{
    // SAFETY: forwarded caller contract.
    unsafe { this.write_xdr_unchecked(w) }
}

macro_rules! write_xdr_int_impls {
    { $( $write_method:ident for $int_ty:ty ),* $(,)? } => { $(

        impl WriteXDR for $int_ty {
            #[inline]
            fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
                w.$write_method(*self)
            }

            #[inline]
            unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
                // SAFETY: forwarded caller contract.
                unsafe { w.$write_method(*self) }
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

    unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
        // SAFETY: forwarded caller contract.
        unsafe { w.write_opaque_variable(self) }
    }
}

impl WriteXDR for &[u8] {
    #[inline]
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
        w.write_opaque_variable(self)
    }

    #[inline]
    unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
        // SAFETY: forwarded caller contract.
        unsafe { w.write_opaque_variable(self) }
    }
}

impl<const N: usize> WriteXDR for [u8; N] {
    #[inline]
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
        let () = ValidU32::<N>::OK;

        w.write_opaque_fixed(self)
    }

    #[inline]
    unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
        let () = ValidU32::<N>::OK;

        // SAFETY: forwarded caller contract.
        unsafe { w.write_opaque_fixed(self) }
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

    unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
        let () = ValidU32::<N>::OK;

        for elt in self {
            // SAFETY: the caller guarantees space for every element.
            unsafe { elt.write_xdr_unchecked(w) };
        }
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

    unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
        debug_assert!(self.len() <= u32::MAX as usize);
        // SAFETY: the caller guarantees space for the length prefix and
        // every element.
        unsafe {
            w.write_u32(self.len() as u32);
            for elt in self {
                elt.write_xdr_unchecked(w);
            }
        }
    }
}

impl<T: WriteXDR> WriteXDR for &T {
    #[inline]
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
        (*self).write_xdr(w)
    }

    #[inline]
    unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
        // SAFETY: forwarded caller contract.
        unsafe { (*self).write_xdr_unchecked(w) }
    }
}

impl<T: WriteXDR> WriteXDR for &mut T {
    #[inline]
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
        (**self).write_xdr(w)
    }

    #[inline]
    unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
        // SAFETY: forwarded caller contract.
        unsafe { (**self).write_xdr_unchecked(w) }
    }
}

impl<T: WriteXDR> WriteXDR for &[T] {
    #[inline(always)]
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
        <[T] as WriteXDR>::write_xdr(*self, w)
    }

    #[inline(always)]
    unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
        // SAFETY: forwarded caller contract.
        unsafe { <[T] as WriteXDR>::write_xdr_unchecked(*self, w) }
    }
}

impl WriteXDR for str {
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
        w.write_opaque_variable(self.as_bytes())
    }

    unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
        // SAFETY: forwarded caller contract.
        unsafe { w.write_opaque_variable(self.as_bytes()) }
    }
}

impl WriteXDR for &str {
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
        w.write_opaque_variable((*self).as_bytes())
    }

    unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
        // SAFETY: forwarded caller contract.
        unsafe { w.write_opaque_variable((*self).as_bytes()) }
    }
}

#[cfg(feature = "alloc")]
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
impl WriteXDR for alloc::string::String {
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
        self.as_str().write_xdr(w)
    }

    unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
        // SAFETY: forwarded caller contract.
        unsafe { self.as_str().write_xdr_unchecked(w) }
    }
}

impl WriteXDR for () {
    fn write_xdr<F, W: Writer<F>>(&self, _w: &mut W) -> WriteResult<()> {
        Ok(())
    }

    unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, _w: &mut W) {}
}

impl WriteXDR for Infallible {
    fn write_xdr<F, W: Writer<F>>(&self, _w: &mut W) -> WriteResult<()> {
        unreachable!()
    }

    unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, _w: &mut W) {
        match *self {}
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

            unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
                let ( $($v),+ ) = self;
                // SAFETY: the caller guarantees space for the whole tuple.
                unsafe { $( write_xdr_unchecked($v, w); )+ }
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

    unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
        let (t,) = self;
        // SAFETY: forwarded caller contract.
        unsafe { t.write_xdr_unchecked(w) }
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

    unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
        // SAFETY: the caller guarantees space for the tag and the payload.
        unsafe {
            if let Some(val) = self {
                w.write_i32(1);
                val.write_xdr_unchecked(w);
            } else {
                w.write_i32(0);
            }
        }
    }
}

const _: () = {
    use super::*;
    use crate::serialize::writer::Writer;
    use crate::serialize::writer_unchecked::WriterUnchecked;
    use xdr_common::*;

    impl<const MAX: u32> WriteXDR for BytesM<MAX> {
        fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
            if self.0.len() > MAX as usize {
                return Err(WriteError::InvalidArrayLength);
            }
            self.0.write_xdr(w)
        }

        unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
            debug_assert!(
                self.0.len() <= MAX as usize,
                "BytesM exceeds declared bound"
            );
            // SAFETY: the caller guarantees the value respects its declared
            // bound and that the writer has space for the encoded form.
            unsafe { self.0.write_xdr_unchecked(w) }
        }
    }

    // Make VecM<u8> work like BytesM;
    #[cfg(feature = "alloc")]
    impl<const MAX: u32> WriteXDR for VecM<u8, MAX> {
        fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
            if self.0.len() > MAX as usize {
                return Err(WriteError::InvalidArrayLength);
            }
            self.0.write_xdr(w)
        }

        unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
            debug_assert!(self.0.len() <= MAX as usize, "VecM exceeds declared bound");
            // SAFETY: the caller guarantees the value respects its declared
            // bound and that the writer has space for the encoded form.
            unsafe { self.0.write_xdr_unchecked(w) }
        }
    }
    #[cfg(not(feature = "alloc"))]
    impl<const MAX: u32> WriteXDR for VecM<u8, MAX> {
        fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
            if self.len() > MAX as usize {
                return Err(WriteError::InvalidArrayLength);
            }
            <Self as AsRef<[u8]>>::as_ref(self).write_xdr(w)
        }

        unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
            debug_assert!(self.len() <= MAX as usize, "VecM exceeds declared bound");
            // SAFETY: the caller guarantees the value respects its declared
            // bound and that the writer has space for the encoded form.
            unsafe { <Self as AsRef<[u8]>>::as_ref(self).write_xdr_unchecked(w) }
        }
    }

    impl<const MAX: u32> WriteXDR for StringM<MAX> {
        fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
            if self.0.len() > MAX as usize {
                return Err(WriteError::InvalidArrayLength);
            }
            self.0.write_xdr(w)
        }

        unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
            debug_assert!(
                self.0.len() <= MAX as usize,
                "StringM exceeds declared bound"
            );
            // SAFETY: the caller guarantees the value respects its declared
            // bound and that the writer has space for the encoded form.
            unsafe { self.0.write_xdr_unchecked(w) }
        }
    }

    #[cfg(feature = "alloc")]
    impl<const MAX: u32, T> WriteXDR for VecM<T, MAX>
    where
        T: WriteXDR,
    {
        fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
            if self.len() > MAX as usize {
                return Err(WriteError::InvalidArrayLength);
            }
            <Self as AsRef<[T]>>::as_ref(self).write_xdr(w)
        }

        unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
            debug_assert!(self.len() <= MAX as usize, "VecM exceeds declared bound");
            // SAFETY: the caller guarantees the value respects its declared
            // bound and that the writer has space for the encoded form.
            unsafe { <Self as AsRef<[T]>>::as_ref(self).write_xdr_unchecked(w) }
        }
    }

    #[cfg(not(feature = "alloc"))]
    impl<const MAX: u32, T> WriteXDR for VecM<T, MAX>
    where
        T: WriteXDR + 'static,
    {
        fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
            if self.len() > MAX as usize {
                return Err(WriteError::InvalidArrayLength);
            }
            <Self as AsRef<[T]>>::as_ref(self).write_xdr(w)
        }

        unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
            debug_assert!(self.len() <= MAX as usize, "VecM exceeds declared bound");
            // SAFETY: the caller guarantees the value respects its declared
            // bound and that the writer has space for the encoded form.
            unsafe { <Self as AsRef<[T]>>::as_ref(self).write_xdr_unchecked(w) }
        }
    }

    #[cfg(feature = "alloc")]
    #[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
    impl<T: WriteXDR> WriteXDR for xdr_common::Box<T> {
        fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
            self.as_ref().write_xdr(w)
        }

        unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
            // SAFETY: forwarded caller contract.
            unsafe { self.as_ref().write_xdr_unchecked(w) }
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

    unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
        // SAFETY: the caller guarantees space for the tag and the payload.
        unsafe {
            match self {
                Ok(x) => {
                    w.write_i32(0);
                    x.write_xdr_unchecked(w);
                }
                Err(e) => {
                    w.write_i32(1);
                    e.write_xdr_unchecked(w);
                }
            }
        }
    }
}

impl<T> WriteXDR for core::marker::PhantomData<T> {
    fn write_xdr<F, W: Writer<F>>(&self, _w: &mut W) -> WriteResult<()> {
        Ok(())
    }

    unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, _w: &mut W) {}
}

#[cfg(feature = "alloc")]
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
const _: () = {
    use crate::serialize::writer_unchecked::WriterUnchecked;
    use alloc::vec::Vec;

    impl WriteXDR for Vec<u8> {
        fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
            w.write_opaque_variable(self.as_ref())
        }

        unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
            // SAFETY: forwarded caller contract.
            unsafe { w.write_opaque_variable(self.as_ref()) }
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

        unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
            let slice: &[T] = self.as_ref();
            // SAFETY: forwarded caller contract.
            unsafe { slice.write_xdr_unchecked(w) }
        }
    }
};

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::MaybeUninit;

    #[test]
    fn tuple_unchecked_writes_txs() {
        let mut buf = [MaybeUninit::uninit(); 8];
        let mut w = crate::txs::unchecked_buf_writer::UncheckedBufWriter::new(&mut buf);
        // SAFETY: two u32s occupy exactly the 8-byte buffer.
        unsafe { (1u32, 2u32).write_xdr_unchecked(&mut w) };
        assert_eq!(w.written(), 8);
        // SAFETY: the first 8 bytes were just initialized.
        let bytes = unsafe { crate::mu::assume_init_ref(&buf[..8]) };
        assert_eq!(bytes, [0, 0, 0, 1, 0, 0, 0, 2]);
    }
}
