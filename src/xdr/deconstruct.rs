//! Deconstruct Rust values into XDR values

use crate::codec::receiver::*;
use crate::codec::receiver::Result;

pub trait Deconstruct {
    fn deconstruct(&self, receiver: &mut dyn Receiver) -> Result<Acc>;
}

impl Deconstruct for i32 {
    fn deconstruct(&self, receiver: &mut dyn Receiver) -> Result<Acc> {
        receiver.receive_i32(*self)
    }
}
