//! Translate Rust values into XDR values

use crate::serialize::writer::Result;
use crate::serialize::writer::*;

pub trait WriteXDR {
    fn write_xdr<F, W: Writer<F>>(&self, w: W) -> Result;
}

pub fn write_xdr<T, F, W>(this: &T, w: W) -> Result<()>
where
    T: WriteXDR,
    W: Writer<F>,
{
    this.write_xdr(w)
}

macro_rules! write_xdr_int_impls {
    { $( $write_method:ident for $int_ty:ty ),* $(,)? } => { $(

        impl WriteXDR for $int_ty {
            fn write_xdr<F, W: Writer<F>>(&self, w: W) -> Result<()> {
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

impl<T: WriteXDR> WriteXDR for Option<T> {
    fn write_xdr<F, W: Writer<F>>(&self, w: W) -> Result<()> {
        w.write_opt(self)
    }
}

impl<T: WriteXDR, const N: usize> WriteXDR for [T; N] {
    fn write_xdr<F, W: Writer<F>>(&self, w: W) -> Result<()> {
        w.write_array(self)
    }
}

impl<'a> WriteXDR for &'a [u8] {
    fn write_xdr<F, W: Writer<F>>(&self, w: W) -> Result<()> {
        w.write_bytes(self)
    }
}

impl<'a, const N: usize> WriteXDR for &'a [u8; N] {
    fn write_xdr<F, W: Writer<F>>(&self, w: W) -> Result<()> {
        w.write_byte_array(&**self)
    }
}

impl<const N: usize> WriteXDR for [u8; N] {
    fn write_xdr<F, W: Writer<F>>(&self, w: W) -> Result<()> {
        w.write_byte_array(&*self)
    }
}

#[cfg(feature = "alloc")]
impl WriteXDR for alloc::vec::Vec<u8> {
    fn write_xdr<F, W: Writer<F>>(&self, w: W) -> Result<()> {
        w.write_bytes(self.as_ref())
    }
}
