//! Derive macros for XDR serialization and type representation.
//!
//! This crate provides procedural macros that automatically generate
//! implementations of `ReadXDR`, `WriteXDR`, and `XDR` traits for your Rust
//! types, allowing them to be serialized/deserialized to/from [XDR][RFC-4506]
//! format and to describe their XDR type representation.
//!
//! ## Macros
//!
//! - **`#[derive(ReadXDR)]`**: Automatically implements the `ReadXDR` trait
//!   for structs and enums, enabling deserialization from XDR-encoded data.
//!
//! - **`#[derive(WriteXDR)]`**: Automatically implements the `WriteXDR` trait
//!   for structs and enums, enabling serialization to XDR-encoded data.
//!
//! - **`#[derive(XDR)]`**: Automatically implements the `XDR` and `HasTypeDef`
//!   traits, allowing types to describe their XDR type representation for
//!   generation of specifications.
//!
//! ## Examples
//!
//! ### Serialization and Deserialization
//!
//! ```rust
//! use xdrs::{ReadXDR, WriteXDR, cxs};
//!
//! #[derive(ReadXDR, WriteXDR)]
//! struct Person {
//!     name: Vec<u8>,
//!     age: u32,
//! }
//!
//! let person = Person {
//!     name: b"Alice".to_vec(),
//!     age: 30,
//! };
//!
//! // Serialize
//! let encoded = cxs::encode(&person).unwrap();
//!
//! // Deserialize
//! let decoded: Person = cxs::decode(&encoded).unwrap();
//!
//! assert_eq!(person.name, decoded.name);
//! assert_eq!(person.age, decoded.age);
//! ```
//!
//! ### XDR Type Representation
//!
//! ```rust
//! use xdr_repr::XDR;
//!
//! #[derive(XDR)]
//! struct Message {
//!     id: u32,
//!     data: [u8; 16],
//! }
//!
//! // Generate XDR specification
//! let xdr_def = Message::def();
//!
//! // The generated XDR specification would be:
//! //
//! //     struct Message {
//! //         unsigned int id;
//! //         opaque data[16];
//! //     };
//! # let _ = xdr_def;
//! ```
//!
//! ### Enums
//!
//! ```rust
//! use xdrs::{ReadXDR, WriteXDR, cxs};
//!
//! #[derive(ReadXDR, WriteXDR, PartialEq, Debug)]
//! #[repr(i32)]
//! enum Status {
//!     Success = 0,
//!     Error { msg: Vec<u8> } = 1,
//! }
//!
//! let status = Status::Success;
//!
//! let encoded = cxs::encode(&status).unwrap();
//! let decoded: Status = cxs::decode(&encoded).unwrap();
//!
//! assert_eq!(status, decoded);
//! ```
//!
//! ## Attributes
//!
//! The derive macros support various attributes for customization.  When
//! deriving `XDR`:
//!
//! - **`#[xdr(name = "XdrName")]`**: Use a different name in the XDR
//!   specification
//! - **`#[xdr(discrim_type = "TypeName")]`**: Specify a custom discriminant
//!   type for enums
//! - **`#[xdr(discrim_var = "field_name")]`**: Specify a custom discriminant
//!   field name
//!
//! When deriving `ReadXDR`:
//! - `#[xdrs(from_type = "Type")]` - Deserialize to the intermediate type
//! "Type" before converting into the target type via `Into`.
//! - `#[xdrs(via = "Type")]` - Same as above.
//!
//! When deriving `WriteXDR`:
//! - `#[xdrs(into_type = "Type")]` - Convert (via `Into`) a different type before
//! serializing.
//! - `#[xdrs(via = "Type")]` - Same as above.
//!
//! [RFC-4506]: https://datatracker.ietf.org/doc/html/rfc4506

mod generate;
mod generate_type;
mod util;

use proc_macro::TokenStream;

/// Derives the `WriteXDR` trait for a struct or enum.
///
/// This macro generates an implementation that serializes the type to XDR format.
/// All field types must implement `WriteXDR`.
///
/// # Attributes
///
/// - `#[xdrs(into_type = "Type")]` - Convert (via `Into`) a different type before
/// serializing.
/// - `#[xdrs(via = "Type")]` - Same as above (also implies `from_type`).
///
#[proc_macro_derive(WriteXDR, attributes(xdrs))]
pub fn derive_write_xdr(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);

    generate::write_xdr(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Derives the `ReadXDR` trait for a struct or enum.
///
/// This macro generates an implementation that deserializes the type from XDR
/// format. All field types must implement `ReadXDR`.
///
/// # Attributes
///
/// - `#[xdrs(from_type = "Type")]` - Deserialize to the intermediate type
/// "Type" before converting into the target type via `Into`.
/// - `#[xdrs(via = "Type")]` - Same as above (also implies `into_type`).
///
#[proc_macro_derive(ReadXDR, attributes(xdrs))]
pub fn derive_read_xdr(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);

    generate::read_xdr(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Derives the `XDR` and `HasTypeDef` traits.
///
/// This macro generates an implementation that describes the type's XDR
/// representation, enabling automatic generation of XDR specifications.
///
/// # Attributes
///
/// - `#[xdr(name = "XdrName")]` - Use a different name in the XDR
///   specification
/// - `#[xdr(discrim_type = "TypeName")]` - Specify a custom discriminant type
///   for enums
/// - `#[xdr(discrim_var = "field_name")]` - Specify a custom discriminant
///   field name
///
#[proc_macro_derive(XDR, attributes(xdr))]
pub fn derive_xdr(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);

    generate_type::xdr(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}
