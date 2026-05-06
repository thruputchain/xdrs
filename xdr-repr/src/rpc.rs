//! Representation of a SunRPC program definition.
//!
//! This module provides types for representing RPC program definitions
//! as specified in [RFC 5531, section 12](https://datatracker.ietf.org/doc/html/rfc5531#section-12).

pub use super::xdr::TypeSpec;
use super::xdr::validate_xdr_identifier;
use alloc::string::String;
use alloc::vec::Vec;

/// An identifier in an RPC program definition.
///
/// Validated at construction to contain only ASCII letters, digits,
/// and underscores per RFC 4506 §6.1.
pub struct Id(String);

impl Id {
    /// Creates a new identifier from a string.
    ///
    /// # Panics
    ///
    /// Panics if `s` is not a valid XDR identifier (ASCII letters, digits,
    /// and underscores; must start with a letter or underscore).
    pub fn new(s: impl Into<String>) -> Self {
        let s = s.into();
        validate_xdr_identifier(&s);
        Self(s)
    }

    /// Returns a string slice of this identifier.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A constant value in an RPC program definition.
pub struct Constant(pub usize);

/// An RPC program definition.
///
/// Programs contain one or more versions, each with their own procedures.
pub struct Program {
    /// The program identifier.
    pub id: Id,
    /// The program number.
    pub value: Constant,
    /// The versions of this program.
    pub versions: Vec<Version>,
}

/// A version of an RPC program.
///
/// Versions contain a set of procedures that can be called.
pub struct Version {
    /// The version identifier.
    pub id: Id,
    /// The version number.
    pub value: Constant,
    /// The procedures in this version.
    pub procedures: Vec<Procedure>,
}

/// A procedure in an RPC program.
///
/// Procedures define the signature of an RPC call, including
/// the return type and argument types.
pub struct Procedure {
    /// The procedure identifier.
    pub id: Id,
    /// The procedure number.
    pub value: Constant,
    /// The return type of the procedure, if any.
    pub return_type: Option<TypeSpec>,
    /// The argument types of the procedure.
    pub arg_types: Vec<Arg>,
}

/// An argument to an RPC procedure.
pub struct Arg {
    /// The type specification of the argument.
    pub ty: TypeSpec,
    /// The name of the argument.
    pub name: String,
}
