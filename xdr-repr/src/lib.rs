//! Datatypes for representing XDR definitions.
//! (Including `Display` implementations for rendering them.)
//!
//! See [RFC 4506, section 6.3](https://datatracker.ietf.org/doc/html/rfc4506#section-6.3) for the
//! XDR language.
//!
//! See [RFC 5531, section 12](https://datatracker.ietf.org/doc/html/rfc5531#section-12) for the
//! RPC language

mod test;
mod display_rpc;

pub mod xdr;
pub mod rpc;
pub mod ty;
pub mod construct;
pub mod format;

// export the types that represent XDR specs
pub use xdr::*;

// export the traits that produce XDR representations
pub use ty::*;

// Re-export XDR derive-macro
#[cfg(feature = "derive")]
pub use xdr_derive::XDR;
