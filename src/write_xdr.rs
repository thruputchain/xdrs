//! Translate Rust values into XDR values

use crate::serialize::writer::Result;
use crate::serialize::writer::*;
use crate::valid_u32::*;

pub trait WriteXDR {
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> Result;
}

pub fn write_xdr<T, F, W>(this: &T, w: &mut W) -> Result<()>
where
    T: WriteXDR,
    W: Writer<F>,
{
    this.write_xdr(w)
}

macro_rules! write_xdr_int_impls {
    { $( $write_method:ident for $int_ty:ty ),* $(,)? } => { $(

        impl WriteXDR for $int_ty {
            fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> Result<()> {
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
}

#[cfg(all(feature = "extras", not(feature = "int128_xdr")))]
const _: () = {
    impl WriteXDR for i128 {
        fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> Result<()> {
            let bytes = self.to_le_bytes();

            let leading_sign_bytes = if *self >= 0 {
                self.leading_zeros()
            } else {
                self.leading_ones()
            } / 8;

            let n_send_bytes = 16 - leading_sign_bytes as usize;
            let buf = &bytes[..n_send_bytes];

            w.write_opaque_variable(buf)
        }
    }

    impl WriteXDR for u128 {
        fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> Result<()> {
            let bytes = self.to_le_bytes();

            let leading_zero_bytes = self.leading_zeros() / 8;

            let n_send_bytes = 16 - leading_zero_bytes as usize;
            let buf = &bytes[..n_send_bytes];

            w.write_opaque_variable(buf)
        }
    }
};

// An alternative way to serialize 128-bit ints
#[cfg(all(feature = "extras", feature = "int128_xdr"))]
const _: () = {
    impl WriteXDR for i128 {
        fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> Result<()> {
            let highbits = (*self >> 64) as i64;
            let lowbits = (*self & 0xffffffffffffffff) as u64;

            write_xdr(&highbits, w)?;
            write_xdr(&lowbits, w)
        }
    }

    impl WriteXDR for u128 {
        fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> Result<()> {
            let highbits = (*self >> 64) as u64;
            let lowbits = (*self & 0xffffffffffffffff) as u64;

            write_xdr(&highbits, w)?;
            write_xdr(&lowbits, w)
        }
    }
};

impl WriteXDR for [u8] {
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> Result<()> {
        w.write_opaque_variable(self)
    }
}

impl WriteXDR for &[u8] {
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> Result<()> {
        w.write_opaque_variable(self)
    }
}

impl<const N: usize> WriteXDR for [u8; N] {
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> Result<()> {
        let () = ValidU32::<N>::OK;

        w.write_opaque_fixed(self)
    }
}

impl<T: WriteXDR, const N: usize> WriteXDR for [T; N] {
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> Result<()> {
        let () = ValidU32::<N>::OK;

        for elt in self {
            elt.write_xdr(w)?;
        }
        Ok(())
    }
}

impl<T: WriteXDR> WriteXDR for [T] {
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> Result<()> {
        if self.len() > u32::MAX as usize {
            return Err(Error::InvalidArrayLength);
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
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> Result<()> {
        (*self).write_xdr(w)
    }
}

impl WriteXDR for str {
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> Result<()> {
        w.write_opaque_variable(self.as_bytes())
    }
}

impl WriteXDR for &str {
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> Result<()> {
        w.write_opaque_variable((*self).as_bytes())
    }
}

#[cfg(feature = "alloc")]
impl WriteXDR for alloc::string::String {
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> Result<()> {
        self.as_str().write_xdr(w)
    }
}

impl WriteXDR for () {
    fn write_xdr<F, W: Writer<F>>(&self, _w: &mut W) -> Result<()> {
        Ok(())
    }
}

macro_rules! write_tuple {
    { $( ($v:ident, $t:ident) ),+ } => {

        impl<$($t),+> WriteXDR for ($($t),+)
        where
            $( $t: WriteXDR, )+
        {
            fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> Result<()> {
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
write_tuple! { (t0, T0), (t1, T1), (t2, T2), (t3, T3), (t4, T4), (t5, T5), (t6, T6), (t7, T7), (t8, T8), (t9, T9), (t10, T10), (t11, T11), (t12, T12), (t13, T13), (t14, T14), (t15, T15), (t16, T16) }

// impl for single-element tuples
impl<T: WriteXDR> WriteXDR for (T,) {
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> Result<()> {
        let (t,) = self;
        t.write_xdr(w)
    }
}

impl<T: WriteXDR> WriteXDR for Option<T> {
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> Result<()> {
        if let Some(val) = self {
            w.write_i32(1)?;
            val.write_xdr(w)
        } else {
            w.write_i32(0)
        }
    }
}

#[cfg(feature = "xdr_common")]
const _: () = {
    use super::*;
    use crate::serialize::writer::{Result, Writer};
    use xdr_common::*;

    impl<const MAX: u32> WriteXDR for BytesM<MAX> {
        fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> Result<()> {
            self.0.write_xdr(w)
        }
    }

    impl<const MAX: u32, T> WriteXDR for VecM<T, MAX>
    where
        T: WriteXDR,
    {
        fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> Result<()> {
            let slice: &[T] = self.as_ref();
            slice.write_xdr(w)
        }
    }

    #[cfg(feature = "alloc")]
    impl<T: WriteXDR> WriteXDR for xdr_common::Box<T> {
        fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> Result<()> {
            self.as_ref().write_xdr(w)
        }
    }
};

impl<T, E> WriteXDR for Result<T, E>
where
    T: WriteXDR,
    E: WriteXDR,
{
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> Result<()> {
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
    fn write_xdr<F, W: Writer<F>>(&self, _w: &mut W) -> Result<()> {
        Ok(())
    }
}

#[cfg(feature = "alloc")]
const _: () = {
    use alloc::vec::Vec;

    impl WriteXDR for Vec<u8> {
        fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> Result<()> {
            w.write_opaque_variable(self.as_ref())
        }
    }

    impl<T> WriteXDR for Vec<T>
    where
        T: WriteXDR,
    {
        fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> Result<()> {
            let slice: &[T] = self.as_ref();
            slice.write_xdr(w)
        }
    }
};
