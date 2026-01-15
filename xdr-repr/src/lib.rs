//! Datatypes for representing XDR definitions.
//! (Including `Display` implementations for rendering them.)
//!
//! This crate allows you to turn Rust structs, enums, and other types into corresponding
//! [XDR][RFC-4506] specifications. By implementing the [`HasTypeDef`] trait (or using the
//! `#[derive(XDR)]` macro with the `derive` feature), your Rust types can describe their
//! XDR representation, enabling automatic generation of XDR specification documents.
//!
//! ## Overview
//!
//! This crate provides types and traits for representing [XDR][RFC-4506] specifications
//! as defined in [RFC 4506, section 6.3](https://datatracker.ietf.org/doc/html/rfc4506#section-6.3).
//! It also includes support for the [RPC language][RFC-5531] as defined in
//! [RFC 5531, section 12](https://datatracker.ietf.org/doc/html/rfc5531#section-12).
//!
//! ## Examples
//!
//! The easiest way to use this crate is with the `derive` feature, which provides
//! the `#[derive(XDR)]` macro to automatically generate XDR type representations.
//!
//! ### Example 1: Simple Struct
//!
//! ```rust
//! # #[cfg(feature = "derive")]
//! # {
//! use xdr_repr::XDR;
//!
//! #[derive(XDR)]
//! pub struct Person {
//!     pub name: [u8; 20],  // Fixed-length opaque data
//!     pub age: u32,
//! }
//!
//! // Generate the XDR specification
//! let xdr_spec = Person::def();
//! // The generated XDR specification would be:
//! // struct Person {
//! //     opaque name[20];
//! //     unsigned int age;
//! // };
//! # let _ = xdr_spec;
//! # }
//! ```
//!
//! ### Example 2: Struct with Arrays
//!
//! ```rust
//! # #[cfg(feature = "derive")]
//! # {
//! use xdr_repr::XDR;
//!
//! #[derive(XDR)]
//! pub struct Message {
//!     pub id: u32,
//!     pub data: [u8; 16],
//!     pub tags: [u32; 10],  // Fixed-length array
//! }
//!
//! // Generate the XDR specification
//! let xdr_spec = Message::def();
//! // The generated XDR specification would be:
//! // struct Message {
//! //     unsigned int id;
//! //     opaque data[16];
//! //     unsigned int tags[10];
//! // };
//! # let _ = xdr_spec;
//! # }
//! ```
//!
//! ### Example 3: Using Attributes
//!
//! You can customize the generated XDR specification using attributes:
//!
//! ```rust
//! # #[cfg(feature = "derive")]
//! # {
//! use xdr_repr::XDR;
//!
//! // Use a different name in the XDR specification
//! #[derive(XDR)]
//! #[xdr(name = "PersonRecord")]
//! pub struct Person {
//!     pub name: Vec<u8>,
//!     pub age: u32,
//! }
//!
//! // For enums (discriminated unions), customize the discriminant type and field name
//! #[derive(XDR)]
//! #[repr(i32)]
//! pub enum Status {
//!     Ok = 0,
//!     Err = -1,
//! }
//!
//! #[derive(XDR)]
//! #[xdr(discrim_type = "Status", discrim_var = "stat")]
//! #[repr(i32)]
//! pub enum Message {
//!     Ok { contents: [u8; 100] } = 0,
//!     Err { err_msg: Vec<u8> } = -1,
//! }
//! # let _person = Person::def();
//! # let _status = Status::def();
//! # let _message = Message::def();
//! # }
//! ```
//!
//! ### Example 4: Enum with Default Case
//!
//! You can mark an enum variant as the default case using `#[xdr(enum_default)]`:
//!
//! ```rust
//! # #[cfg(feature = "derive")]
//! # {
//! use xdr_repr::{XDR, HasSpecification};
//!
//! #[derive(XDR)]
//! #[repr(i32)]
//! pub enum Response {
//!     Success { data: [u8; 100] } = 0,
//!     NotFound = 1,
//!     
//!     // This variant handles all other discriminant values
//!     // It must have exactly one i32 field
//!     #[xdr(enum_default)]
//!     Default { code: i32 },
//! }
//!
//! // Generate the XDR specification
//! let xdr_spec = Response::spec();
//! // The generated XDR specification would be:
//! // enum ResponseType {
//! //     Success = 0,
//! //     NotFound = 1,
//! // };
//! // 
//! // union Response switch (ResponseType which)
//! // {
//! // case Success:
//! //     opaque data[100];
//! // case NotFound:
//! //     void;
//! // default:
//! //     int code;
//! // };
//! # let _ = xdr_spec;
//! # }
//! ```
//!
//! ### Manual Implementation
//!
//! If you prefer not to use the derive macro, you can manually implement the [`HasTypeDef`] trait:
//!
//! ```rust
//! use xdr_repr::*;
//!
//! pub struct Person {
//!     pub name: [u8; 20],  // Fixed-length opaque
//!     pub age: u32,
//! }
//!
//! impl HasTypeDef for Person {
//!     fn type_def() -> TypeDef {
//!         struct_def("Person", vec![
//!             <[u8; 20]>::declare(ident("name")),
//!             <u32>::declare(ident("age")),
//!         ])
//!     }
//! }
//!
//! // Now Person::def() can be used to generate the XDR specification
//! # let _ = Person::def();
//! ```
//!
//! ## Main Types
//!
//! The main types in this crate represent the structure of XDR specifications:
//! - [`Specification`] - A complete XDR specification containing multiple definitions
//! - [`Definition`] - A single definition (type or constant)
//! - [`TypeDef`] - A type definition (struct, enum, union, or typedef)
//! - [`Declaration`] - A field or variable declaration
//! - [`TypeSpec`] - A type specification
//!
//! ## Attributes
//!
//! When using `#[derive(XDR)]`, you can customize the generated XDR specification
//! with the following attributes:
//!
//! - **`#[xdr(name = "XdrName")]`** - Use a different name in the XDR specification
//!   than the Rust type name. Applied to structs or enums.
//!
//! - **`#[xdr(discrim_type = "TypeName")]`** - For enums (discriminated unions), specify
//!   the name of an existing XDR type to use as the discriminant type instead of
//!   auto-generating one. The default is `{EnumName}Type`.
//!
//! - **`#[xdr(discrim_var = "field_name")]`** - For enums (discriminated unions), specify
//!   the name of the discriminant field. The default is `"which"`.
//!
//! - **`#[xdr(enum_default)]`** - Mark an enum variant as the default case in a
//!   discriminated union. Applied to individual enum variants.
//!
//! ## Traits
//!
//! Traits allow Rust types to describe their XDR representation:
//! - [`HasTypeDef`] - Types that can describe their XDR type definition
//! - [`HasTypeSpec`] - Types that can describe their XDR type specification
//! - [`Declare`] - Types that can create a declaration with a given name
//! - [`XDR`] - Types that can produce a complete XDR definition
//!
//! [RFC-4506]: https://datatracker.ietf.org/doc/html/rfc4506
//! [RFC-5531]: https://datatracker.ietf.org/doc/html/rfc5531

#![no_std]
#![cfg_attr(docsrs, feature(doc_cfg))]

extern crate alloc;

mod display_rpc;

pub mod test;

pub mod construct;
pub mod format;
pub mod rpc;
pub mod ty;
pub mod result;
pub mod xdr;

// export the types that represent XDR specs
pub use xdr::*;

// export XDR-spec constructors
pub use construct::*;

// export the traits that produce XDR representations
pub use ty::*;
pub use result::*;

// Re-export XDR derive-macro
#[cfg(feature = "derive")]
#[cfg_attr(docsrs, doc(cfg(feature = "derive")))]
pub use xdr_derive::XDR;
