//! Deconstruct Rust values into XDR values

use crate::codec::write::*;
use crate::codec::write::Result;
use crate::bytes::*;

pub use crate::codec::write::Acc;

pub trait Deconstruct {
    fn deconstruct(&self, w: &mut dyn Write) -> Result<Acc>;
}

impl Deconstruct for i32 {
    fn deconstruct(&self, w: &mut dyn Write) -> Result<Acc> {
        w.write_i32(*self)
    }
}

impl Deconstruct for u32 {
    fn deconstruct(&self, w: &mut dyn Write) -> Result<Acc> {
        w.write_u32(*self)
    }
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

pub fn deconstruct_byte_slice(bytes: &&[u8], w: &mut dyn Write) -> Result<Acc> {
    w.write_bytes(*bytes)
}

pub fn deconstruct_byte_array(bytes: &&[u8], w: &mut dyn Write) -> Result<Acc> {
    w.write_byte_array(*bytes)
}

pub type DeconstructFn<T> = fn (&T, &mut dyn Write) -> Result<Acc>;
