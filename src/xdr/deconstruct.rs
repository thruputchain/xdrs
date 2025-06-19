//! Deconstruct Rust values into XDR values

use crate::codec::write::*;
use crate::codec::write::Result;
use crate::bytes::*;

pub use crate::codec::write::Acc;

pub trait Deconstruct {
    fn deconstruct(&self, w: &mut dyn Write) -> Result<Acc>;
}

pub fn deconstruct<T>(this: &T, w: &mut dyn Write) -> Result<Acc>
where
    T: Deconstruct
{
    this.deconstruct(w)
}

macro_rules! deconstruct_int_impls {
    { $( $write_method:ident for $int_ty:ty ),* $(,)? } => { $(

        impl Deconstruct for $int_ty {
            fn deconstruct(&self, w: &mut dyn Write) -> Result<Acc> {
                w.$write_method(*self)
            }
        }

    )* };
}

deconstruct_int_impls! {
    write_i64 for i64,
    write_i32 for i32,
    write_i16 for i16,
    write_i8 for i8,

    write_u64 for u64,
    write_u32 for u32,
    write_u16 for u16,
    write_u8 for u8,
}

impl<'a> Deconstruct for Bytes<'a> {
    fn deconstruct(&self, w: &mut dyn Write) -> Result<Acc> {
        w.write_bytes(&self.as_ref())
    }
}

impl<'a, const N: usize> Deconstruct for ByteArray<'a, N> {
    fn deconstruct(&self, w: &mut dyn Write) -> Result<Acc> {
        w.write_byte_array(self.as_ref())
    }
}

impl<const N: usize> Deconstruct for OwnedByteArray<N> {
    fn deconstruct(&self, w: &mut dyn Write) -> Result<Acc> {
        w.write_byte_array(self.as_ref())
    }
}

pub fn deconstruct_byte_slice(bytes: &&[u8], w: &mut dyn Write) -> Result<Acc> {
    w.write_bytes(*bytes)
}

pub fn deconstruct_byte_array(bytes: &&[u8], w: &mut dyn Write) -> Result<Acc> {
    w.write_byte_array(*bytes)
}

pub type DeconstructFn<T> = fn (&T, &mut dyn Write) -> Result<Acc>;
