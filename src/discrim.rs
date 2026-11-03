//! The [`XDRDiscriminant`] trait, for types that act as [XDR][RFC-4506]
//! union discriminants.
//!
//! [RFC-4506]: https://datatracker.ietf.org/doc/html/rfc4506

use crate::serialize::reader::{ReadResult, Reader};

/// Trait for types that have an XDR discriminant value.
///
/// The discriminant is used in [XDR][RFC-4506] unions and variants (as defined in RFC 4506)
/// to identify which variant is present in the serialized data.
///
/// A union's `#[xdr(default_variant)]` stores the discriminant in its first
/// field, whose type must implement this trait. The derived reader decodes
/// the discriminant with [`read_discriminant`](Self::read_discriminant), so
/// the field type decides which wire values are valid: a `bool`
/// discriminant accepts only 0 and 1, even in the default arm.
///
/// [RFC-4506]: https://datatracker.ietf.org/doc/html/rfc4506
pub trait XDRDiscriminant {
    /// Returns the discriminant value for this type.
    ///
    /// This value is used when serializing/deserializing XDR union types
    /// to identify which variant is being encoded or decoded.
    fn discriminant(&self) -> i32;

    /// Reads a discriminant of this type from the wire, rejecting values
    /// the type cannot represent.
    fn read_discriminant<'r, R: Reader<'r>>(r: &mut R) -> ReadResult<Self>
    where
        Self: Sized;
}

impl XDRDiscriminant for i32 {
    fn discriminant(&self) -> i32 {
        *self
    }

    fn read_discriminant<'r, R: Reader<'r>>(r: &mut R) -> ReadResult<Self> {
        r.read_i32()
    }
}

impl XDRDiscriminant for u32 {
    fn discriminant(&self) -> i32 {
        *self as i32
    }

    fn read_discriminant<'r, R: Reader<'r>>(r: &mut R) -> ReadResult<Self> {
        r.read_u32()
    }
}

impl XDRDiscriminant for bool {
    fn discriminant(&self) -> i32 {
        i32::from(*self)
    }

    fn read_discriminant<'r, R: Reader<'r>>(r: &mut R) -> ReadResult<Self> {
        r.read_bool()
    }
}
