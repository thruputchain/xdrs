use crate::serialize::reader::*;
use crate::serialize::reader_path::*;
use delegate::*;

pub struct PathReader<R, H> {
    reader: R,
    path_handler: H,
}

impl<R, H> PathReader <R, H> {
    pub fn new(reader: R, path_handler: H) -> Self {
        Self { reader, path_handler }
    }
}

impl<R, H> PathHandler for PathReader<R, H>
where
    H: PathHandler,
{
    type Path = H::Path;

    delegate! {
        to self.path_handler {
            fn sequence_begin(&mut self);
            fn sequence_next(&mut self);
            fn sequence_end(&mut self);
            fn path(&self) -> Self::Path;
        }
    }
}

impl<'r, R, H> Reader<'r> for PathReader<R, H>
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

impl<'r, R, H, P> ReaderPath<'r, P> for PathReader<R, H>
where
    R: Reader<'r>,
    H: PathHandler<Path = P>,
{}
