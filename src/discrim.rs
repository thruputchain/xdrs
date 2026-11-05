//! The [`XDRDiscriminant`] trait, for types that act as [XDR][RFC-4506]
//! union discriminants.
//!
//! [RFC-4506]: https://datatracker.ietf.org/doc/html/rfc4506

/// Trait for types that have an XDR discriminant value.
///
/// The discriminant is used in [XDR][RFC-4506] unions and variants (as defined in RFC 4506)
/// to identify which variant is present in the serialized data.
///
/// [RFC-4506]: https://datatracker.ietf.org/doc/html/rfc4506
pub trait XDRDiscriminant {
    /// Returns the discriminant value for this type.
    ///
    /// This value is used when serializing/deserializing XDR union types
    /// to identify which variant is being encoded or decoded.
    fn discriminant(&self) -> i32;
}
