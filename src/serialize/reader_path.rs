use super::reader::*;

pub trait PathHandler {
    type Path;

    fn sequence_begin(&mut self);
    fn sequence_next(&mut self);
    fn sequence_end(&mut self);

    fn path(&self) -> Self::Path;
}

pub trait ReaderPath<'r, P>: Reader<'r> + PathHandler<Path = P> {}

pub struct NullPath;

pub trait FromPath<T> {
    fn from_path(x: T) -> Self;
}

impl<T> FromPath<T> for NullPath {
    fn from_path(_: T) -> Self {
        Self
    }
}

pub trait IntoPath<P> {
    fn into_path(self) -> P;
}

impl<P, P2> IntoPath<P2> for P
where
    P2: FromPath<P>
{
    fn into_path(self) -> P2 {
        <P2 as FromPath<P>>::from_path(self)
    }
}
