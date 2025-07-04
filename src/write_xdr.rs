//! Translate Rust values into XDR values

use crate::discrim::XDRDiscriminant;
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
    write_i8 for i8,

    write_u64 for u64,
    write_u32 for u32,
    write_u16 for u16,

    write_bool for bool,
}

#[cfg(feature = "extras")]
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

#[cfg(feature = "extras")]
impl WriteXDR for u128 {
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> Result<()> {
        let bytes = self.to_le_bytes();

        let leading_zero_bytes = self.leading_zeros() / 8;

        let n_send_bytes = 16 - leading_zero_bytes as usize;
        let buf = &bytes[..n_send_bytes];

        w.write_opaque_variable(buf)
    }
}

impl WriteXDR for &[u8] {
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> Result<()> {
        w.write_opaque_variable(self)
    }
}

impl<const N: usize> WriteXDR for &[u8; N] {
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> Result<()> {
        let () = ValidU32::<N>::OK;

        w.write_opaque_fixed(*self)
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

impl WriteXDR for () {
    fn write_xdr<F, W: Writer<F>>(&self, _w: &mut W) -> Result<()> {
        Ok(())
    }
}

impl<T1, T2> WriteXDR for (T1, T2)
where
    T1: WriteXDR,
    T2: WriteXDR,
{
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> Result<()> {
        write_xdr(&self.0, w)?;
        write_xdr(&self.1, w)?;
        Ok(())
    }
}

impl<T1, T2, T3> WriteXDR for (T1, T2, T3)
where
    T1: WriteXDR,
    T2: WriteXDR,
    T3: WriteXDR,
{
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> Result<()> {
        write_xdr(&self.0, w)?;
        write_xdr(&self.1, w)?;
        write_xdr(&self.2, w)?;
        Ok(())
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

impl<T, E> XDRDiscriminant for Result<T, E> {
    fn discriminant(&self) -> i32 {
        unsafe { *<*const _>::from(self).cast::<i32>() }
    }
}

impl<T, E> WriteXDR for Result<T, E>
where
    T: WriteXDR,
    E: WriteXDR,
{
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> Result<()> {
        let d = <Self as XDRDiscriminant>::discriminant(self);
        w.write_i32(d)?;

        match self {
            Ok(x) => <T as WriteXDR>::write_xdr(x, w),
            Err(e) => <E as WriteXDR>::write_xdr(e, w),
        }
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
