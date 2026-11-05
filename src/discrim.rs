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

/// Returns an `i32` that is not in `used`, for the in-memory tag of a
/// `#[xdr(default_variant)]`.
///
/// The default variant's tag never goes on the wire, but Rust requires the
/// enum's tags to be distinct, and an implicit tag (the previous variant's
/// plus one) can collide with a concrete arm.  Pass every concrete arm's
/// discriminant; it is evaluated at compile time:
///
/// ```text
/// #[derive(ReadXDR, WriteXDR)]
/// #[repr(i32)]
/// enum Reply {
///     Ok = 0,
///     Bad = -1,
///     #[xdr(default_variant)]
///     Other(i32) = xdrs::unused_discriminant(&[0, -1]), // implicitly 0
/// }
/// ```
///
/// ```
/// const TAG: i32 = xdrs::unused_discriminant(&[0, -1, i32::MIN]);
/// assert!(TAG != 0 && TAG != -1 && TAG != i32::MIN);
/// ```
pub const fn unused_discriminant(used: &[i32]) -> i32 {
    let mut candidate = i32::MIN;
    loop {
        let mut i = 0;
        let mut taken = false;
        while i < used.len() {
            if used[i] == candidate {
                taken = true;
                break;
            }
            i += 1;
        }
        if !taken {
            return candidate;
        }
        candidate += 1;
    }
}
