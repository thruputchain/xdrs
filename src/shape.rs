//!
//! Traits [`Host`] and [`Visitor`] for guiding a visitor
//! through the structure of an XDR value.
//!

#![allow(unused)]

/// A type that wants to visit a host
pub trait Visitor {
    /// A type the `Visitor` returns when it wants to end the visit
    type Error;

    /// A type that will visit the parts of a struct
    type VisitStruct<'s>: StructVisitor<Self::Error>
    where
        Self: 's;

    /// The final result of a visit
    type Return;

    /// Visit an XDR structure
    fn visit_struct<'a>(
        &'a mut self,
        name: &str,
        n_fields: u32,
    ) -> Result<Self::VisitStruct<'a>, Self::Error>;

    fn visit_string(&mut self, s: &str) -> Result<(), Self::Error>;

    fn visit_u32(&mut self, n: u32) -> Result<(), Self::Error>;

    /// Yield the final result, consuming the `Visitor`
    fn into_return(self) -> Self::Return;
}

/// A type that wants to visit an XDR structure's fields
pub trait StructVisitor<E> {
    /// Visit a field of an XDR structure
    fn visit_field<T: Host>(&mut self, name: &str, value: T) -> Result<(), E>;
}

/// A type that accepts visitors
pub trait Host {
    /// Accept a visitor and guide it to each XDR component of the `Host` value.
    fn accept<V>(&self, mut v: V) -> Result<V::Return, V::Error>
    where
        V: Visitor,
    {
        self.guide(&mut v)?;

        Ok(v.into_return())
    }

    /// Guide a [`Visitor`] around the [`Host`]'s establishment.
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


