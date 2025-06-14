//! Deconstruct Rust values into XDR values

use crate::codec::write::*;
use crate::codec::write::Result;

pub trait Deconstruct {
    fn deconstruct(&self, w: &mut dyn Write) -> Result<Acc>;
}

impl Deconstruct for i32 {
    fn deconstruct(&self, w: &mut dyn Write) -> Result<Acc> {
        w.write_i32(*self)
    }
}
