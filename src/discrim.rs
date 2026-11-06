//! The [`XDRDiscriminant`] trait, for types that switch an [XDR][RFC-4506]
//! union.
//!
//! [RFC-4506]: https://datatracker.ietf.org/doc/html/rfc4506

/// How a union discriminant is encoded on the wire, which the union's
/// switch type decides.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DiscriminantEncoding {
    /// An `int`, or an enum, whose values are `int`s.
    Signed,
    /// An `unsigned int`.
    Unsigned,
    /// A `bool`: reading rejects anything but 0 and 1.
    Bool,
}

/// A type that can switch an [XDR][RFC-4506] union: `i32`, `u32`, `bool`, or
/// an enum.
///
/// A derived union holds its discriminant as the `i32` tag of its
/// `#[repr(i32)]` enum.  Its switch type, named with
/// `#[xdr(discriminant = "...")]` (by default `i32`), decides how that tag
/// is encoded, and the type of a `#[xdr(default_variant)]`'s first field.
///
/// [RFC-4506]: https://datatracker.ietf.org/doc/html/rfc4506
pub trait XDRDiscriminant {
    /// How a discriminant of this type is encoded.  A constant rather than a
    /// method, so that the const writers can use it too.
    const ENCODING: DiscriminantEncoding;

    /// Returns this value as a union's `i32` tag.  An unsigned value keeps
    /// its bit pattern.
    fn discriminant(&self) -> i32;

    /// Returns the value whose tag is `d`, or `None` if no value of this type
    /// has that tag; the inverse of [`discriminant`](Self::discriminant).
    ///
    /// A derived union's reader calls this for its default arm and rejects
    /// a `None` as a bad discriminator: a closed enum, for instance, has no
    /// value for a tag that is not one of its members.
    fn from_discriminant(d: i32) -> Option<Self>
    where
        Self: Sized;
}

impl XDRDiscriminant for i32 {
    const ENCODING: DiscriminantEncoding = DiscriminantEncoding::Signed;

    fn discriminant(&self) -> i32 {
        *self
    }

    fn from_discriminant(d: i32) -> Option<Self> {
        Some(d)
    }
}

impl XDRDiscriminant for u32 {
    const ENCODING: DiscriminantEncoding = DiscriminantEncoding::Unsigned;

    fn discriminant(&self) -> i32 {
        *self as i32
    }

    fn from_discriminant(d: i32) -> Option<Self> {
        Some(d as u32)
    }
}

impl XDRDiscriminant for bool {
    const ENCODING: DiscriminantEncoding = DiscriminantEncoding::Bool;

    fn discriminant(&self) -> i32 {
        i32::from(*self)
    }

    fn from_discriminant(d: i32) -> Option<Self> {
        match d {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }
}
