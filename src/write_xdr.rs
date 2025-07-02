//! Translate Rust values into XDR values

use crate::serialize::writer::Result;
use crate::serialize::writer::*;

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

impl<'a> WriteXDR for &'a [u8] {
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> Result<()> {
        w.write_opaque_variable(*self)
    }
}

impl<'a, const N: usize> WriteXDR for &'a [u8; N] {
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
            return Err(Error::InvalidArrayLength)
        }
        let len = self.len() as u32;
        w.write_u32(len)?;

        for elt in self {
            elt.write_xdr(w)?;
        }
        Ok(())
    }
}


#[cfg(feature = "alloc")]
impl WriteXDR for alloc::vec::Vec<u8> {
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> Result<()> {
        w.write_opaque_variable(self.as_ref())
    }
}

#[cfg(feature = "alloc")]
impl<T> WriteXDR for alloc::vec::Vec<T>
where
    T: WriteXDR
{
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> Result<()> {
        let slice: &[T] = self.as_ref();
        slice.write_xdr(w)
    }
}

// Hack to clamp const generic usize at compile-time
struct ValidU32<const N: usize>;
impl<const N: usize> ValidU32<N> {
    const OK: () = assert!(N <= u32::MAX as usize, "N must fit in a u32");
}
