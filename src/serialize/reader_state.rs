use super::reader::*;

pub trait State {
    fn sequence_begin(&mut self);
    fn sequence_next(&mut self);
    fn sequence_end(&mut self);

    fn path(&self) -> &[u32];
}

pub trait ReaderState<'r>:  Reader<'r> + State {}
