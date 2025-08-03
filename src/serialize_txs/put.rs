//! Provide a generic TXS implementation for any type
//! that implements some basic byte operations

#![allow(clippy::upper_case_acronyms)]

use crate::serialize::writer::Result;
use crate::serialize::writer::*;

/// Implement this basic byte operation in order
/// to get a `Writer<TXS>` implementation for free
pub(crate) trait Put {
    // fn put_byte(&mut self, b: u8) -> Result<()>;
    fn put_bytes(&mut self, bytes: &[u8]) -> Result<()>;
}

pub struct TXS;

static PAD: [u8; 3] = [0; 3];

macro_rules! write_int {
    [ $($write:ident($nbytes:expr): $ty:ty),* $(,)? ] => {

    $(
        fn $write(&mut self, n: $ty) -> Result<()> {
            let bytes: [u8; $nbytes] = n.to_be_bytes();
            self.put_bytes(&bytes)
        }
    )*

    };
}

impl<P> Writer<TXS> for P
where
    P: Put,
{
    fn write_opaque_fixed(&mut self, bytes: &[u8]) -> Result<()> {
        self.put_bytes(bytes)?;

        let rem = bytes.len() % 4;
        if rem > 0 {
            let padlen = 4 - rem;
            self.put_bytes(&PAD[0..padlen])?;
        }

        Ok(())
    }

    fn write_opaque_variable(&mut self, bytes: &[u8]) -> Result<()> {
        if bytes.len() > u32::MAX as usize {
            return Err(Error::InvalidArrayLength);
        }

        self.write_u32(bytes.len() as u32)?;
        self.write_opaque_fixed(bytes)
    }

    fn write_bool(&mut self, v: bool) -> Result<()> {
        self.write_i32(if v { 1 } else { 0 })
    }

    write_int! [
        write_i64(8): i64,
        write_i32(4): i32,
        write_i16(2): i16,

        write_u64(8): u64,
        write_u32(4): u32,
        write_u16(2): u16,
    ];
}
