//! Translate Rust values into XDR values

use crate::codec::writer::*;
use crate::codec::writer::Result;
use crate::bytes::*;

pub trait WriteXDR {
    fn write_xdr<W: Writer>(&self, w: W) -> Result;
}

pub fn write_xdr<T, W>(this: &T, w: W) -> Result<()>
where
    T: WriteXDR, W: Writer
{
    this.write_xdr(w)
}

macro_rules! write_xdr_int_impls {
    { $( $write_method:ident for $int_ty:ty ),* $(,)? } => { $(

        impl WriteXDR for $int_ty {
            fn write_xdr<W: Writer>(&self, w: W) -> Result<()> {
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
    write_u8 for u8,
}

impl<'a> WriteXDR for Bytes<'a> {
    fn write_xdr<W: Writer>(&self, w: W) -> Result<()> {
        w.write_bytes(&self.as_ref())
    }
}

impl<'a, const N: usize> WriteXDR for ByteArray<'a, N> {
    fn write_xdr<W: Writer>(&self, w: W) -> Result<()> {
        w.write_byte_array(self.as_ref())
    }
}

impl<const N: usize> WriteXDR for OwnedByteArray<N> {
    fn write_xdr<W: Writer>(&self, w: W) -> Result<()> {
        w.write_byte_array(self.as_ref())
    }
}
