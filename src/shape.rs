//!
//! Traits [`Host`] and [`Visitor`] for guiding a visitor
//! through the structure of an XDR value.
//!

#![allow(unused)]

/// A type that wants to visit the components of an XDR value
pub trait Visitor {
    /// The final result of a visit
    type Return;

    /// A type the `Visitor` returns when it wants to end the visit
    type Error;

    /// A type that wants to visit the fields of a struct
    type VisitStruct<'v>: VisitorStruct<Self::Error>
    where
        Self: 'v;

    /// A type that wants to visit the elements of an array
    type VisitArray<'v, T>: VisitorArray<T, Self::Error>
    where
        Self: 'v;

    /// Visit an XDR string
    fn visit_string(&mut self, s: &str) -> Result<(), Self::Error>;

    /// Visit an XDR unsigned
    fn visit_u32(&mut self, n: u32) -> Result<(), Self::Error>;

    /// Visit an XDR structure
    fn visit_struct<'v>(
        &'v mut self,
        name: &str,
        n_fields: u32,
    ) -> Result<Self::VisitStruct<'v>, Self::Error>;

    /// Visit an XDR array
    fn visit_array<'v, T: Host>(
        &'v mut self,
        n_elts: usize,
    ) -> Result<Self::VisitArray<'v, T>, Self::Error>;

    /// Yield the final result, consuming the `Visitor`
    fn finish(self) -> Self::Return;
}

/// A type that wants to visit an XDR structure's fields
pub trait VisitorStruct<E> {
    /// Visit a field of an XDR structure
    fn visit_field<T: Host>(&mut self, name: &str, value: &T) -> Result<(), E>;

    /// Finish visiting the struct
    fn finish(self) -> Result<(), E>;
}

/// A type that wants to visit an XDR arrays's elements
pub trait VisitorArray<T, E> {
    /// Visit an element of an XDR array
    fn visit_elt(&mut self, value: &T) -> Result<(), E>
        where T: Host;

    /// Finish visiting the array
    fn finish(self) -> Result<(), E>;
}

/// A type that accepts visitors and guides them to the components of an XDR value
pub trait Host {
    /// Accept a [`Visitor`] and guide it to each component of the [`Host`] XDR value, consuming
    /// the visitor and returning its result.
    ///
    fn accept<V>(&self, mut v: V) -> Result<V::Return, V::Error>
    where
        V: Visitor,
    {
        self.guide(&mut v)?;

        Ok(v.finish())
    }

    /// Guide a [`Visitor`] around the [`Host`]'s establishment.
    ///
    fn guide<V>(&self, v: &mut V) -> Result<(), V::Error>
    where
        V: Visitor;
}

impl Host for &'static str {
    fn guide<V>(&self, v: &mut V) -> Result<(), V::Error>
    where
        V: Visitor,
    {
        v.visit_string(self)
    }
}

impl Host for u32 {
    fn guide<V>(&self, v: &mut V) -> Result<(), V::Error>
    where
        V: Visitor,
    {
        v.visit_u32(*self)
    }
}

#[cfg(feature = "alloc")]
impl<T: Host> Host for alloc::vec::Vec<T> {
    fn guide<V>(&self, v: &mut V) -> Result<(), V::Error>
    where
        V: Visitor,
    {
        let mut a = v.visit_array(self.len())?;

        for elt in self {
            a.visit_elt(elt)?;
        }

        a.finish()?;

        Ok(())
    }
}
