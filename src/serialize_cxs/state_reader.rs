use crate::serialize::reader::*;
use crate::serialize::reader_state::*;
use core::ops::{Deref, DerefMut};
use delegate::*;
use alloc::{vec, vec::Vec};

pub struct StateReader<R> {
    reader: R,
    path: Vec<u32>,
}

impl<R> StateReader<R> {
    pub fn new(reader: R) -> Self {
        let path = vec![];
        Self { reader, path }
    }
}

impl<R> Deref for StateReader<R> {
    type Target = R;

    fn deref(&self) -> &Self::Target {
        &self.reader
    }
}

impl<R> DerefMut for StateReader<R> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.reader
    }
}

impl<R> State for StateReader<R> {
    fn sequence_begin(&mut self) {
        self.path.push(0);
    }
    fn sequence_next(&mut self) {
        let i = self.path.pop().unwrap();
        self.path.push(i + 1);
    }
    fn sequence_end(&mut self) {
        let _ = self.path.pop();
    }

    fn path(&self) -> &[u32] {
        self.path.as_ref()
    }
}

impl<'r, R> Reader<'r> for StateReader<R>
where
    R: Reader<'r>
{
    delegate! {
        to self.reader {
            fn has_more_input(&self) -> bool;
            fn read_i64(&mut self) -> Result<i64>;
            fn read_i32(&mut self) -> Result<i32>;
            fn read_i16(&mut self) -> Result<i16>;

            fn read_u64(&mut self) -> Result<u64>;
            fn read_u32(&mut self) -> Result<u32>;
            fn read_u16(&mut self) -> Result<u16>;
            fn read_opaque_fixed(&mut self, len: u32) -> Result<&'r [u8]>;
            fn read_opaque_variable(&mut self, max: u32) -> Result<&'r [u8]>;
        }
    }
}

impl<'r, R> ReaderState<'r> for StateReader<R>
where
    R: Reader<'r> {}
