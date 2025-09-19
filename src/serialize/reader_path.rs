use super::reader::*;

pub trait PathHandler {
    type Path;

    fn sequence_begin(&mut self);
    fn sequence_next(&mut self);
    fn sequence_end(&mut self);

    fn path(&self) -> Self::Path;
}

pub trait ReaderPath<'r, P>:  Reader<'r> + PathHandler<Path = P> {}
