#![allow(clippy::tabs_in_doc_comments)]
//! Datatypes for representing XDR definitions.
//! (Including `Display` implementations for rendering them.)
//!
//! This crate allows you to turn Rust structs, enums, and other types into
//! corresponding [XDR][RFC-4506] specifications. By implementing the
//! [`HasTypeDef`] trait (or using the `#[derive(xdrs::XDR)]` macro via the
//! umbrella `xdrs` crate with `derive` + `repr` features), your Rust types can
//! describe their XDR representation, enabling automatic generation of XDR
//! specification documents.
//!
//! ## Overview
//!
//! This crate provides types and traits for representing [XDR][RFC-4506]
//! specifications as defined in [RFC 4506, section
//! 6.3](https://datatracker.ietf.org/doc/html/rfc4506#section-6.3). It also
//! includes support for the [RPC language][RFC-5531] as defined in [RFC 5531,
//! section 12](https://datatracker.ietf.org/doc/html/rfc5531#section-12).
//!
//! ## Examples
//!
//! The easiest way to use this crate is through the `xdrs` umbrella crate with
//! `derive` + `repr` features, which provides the `#[derive(xdrs::XDR)]` macro
//! to automatically generate XDR type representations.
//!
//! ### Example 1: Simple Struct
//!
//! ```rust
//! use xdrs::xdr_repr::XDR;
//!
//! #[derive(xdrs::XDR)]
//! pub struct Person {
//!     pub name: [u8; 20],  // Fixed-length opaque data
//!     pub age: u32,
//! }
//!
//! // Generate the XDR specification
//! let xdr_spec = Person::def();
//! let formatted = xdr_spec.to_string();
//! let expected = r#"struct Person {
//! 	opaque name[20];
//! 	unsigned int age;
//! };
//! "#;
//! assert_eq!(expected, formatted);
//! ```
//!
//! ### Example 2: Struct with Arrays
//!
//! ```rust
//! use xdrs::xdr_repr::XDR;
//!
//! #[derive(xdrs::XDR)]
//! pub struct Message {
//!     pub id: u32,
//!     pub data: [u8; 16],
//!     pub tags: [u32; 10],  // Fixed-length array
//! }
//!
//! // Generate the XDR specification
//! let xdr_spec = Message::def();
//! let formatted = xdr_spec.to_string();
//! let expected = r#"struct Message {
//! 	unsigned int id;
//! 	opaque data[16];
//! 	unsigned int tags[10];
//! };
//! "#;
//! assert_eq!(expected, formatted);
//! ```
//!
//! ### Example 3: Using Attributes
//!
//! You can customize the generated XDR specification using attributes:
//!
//! ```rust
//! use xdrs::xdr_repr::{XDR, HasSpecification};
//!
//! // Use a different name in the XDR specification
//! #[derive(xdrs::XDR)]
//! #[xdr(name = "PersonRecord")]
//! pub struct Person {
//!     pub name: xdr_common::BytesM,
//!     pub age: u32,
//! }
//!
//! // Verify the generated XDR specification
//! let person_spec = Person::def().to_string();
//! let expected_person = r#"struct PersonRecord {
//! 	opaque name<>;
//! 	unsigned int age;
//! };
//! "#;
//! assert_eq!(expected_person, person_spec);
//!
//! // For enums (discriminated unions), customize the discriminant type and field name
//! #[derive(xdrs::XDR)]
//! #[repr(i32)]
//! pub enum Status {
//!     Ok = 0,
//!     Err = -1,
//! }
//!
//! let status_spec = Status::def().to_string();
//! let expected_status = r#"enum Status {
//! 	Ok = 0,
//! 	Err = -1,
//! };
//! "#;
//! assert_eq!(expected_status, status_spec);
//!
//! #[derive(xdrs::XDR)]
//! #[xdr(discrim_type = "Status", discrim_var = "stat")]
//! #[repr(i32)]
//! pub enum Message {
//!     Ok { contents: [u8; 100] } = 0,
//!     Err { err_msg: Vec<u8> } = -1,
//! }
//!
//! let message_spec = Message::spec().to_string();
//! // The spec contains both the enum and union definitions
//! let expected_message = r#"union Message switch (Status stat)
//! {
//! case Ok:
//! 	opaque contents[100];
//! case Err:
//! 	opaque err_msg<>;
//! };
//!
//! "#;
//! assert_eq!(expected_message, message_spec);
//! ```
//!
//! ### Example 4: Enum with Default Case
//!
//! You can mark an enum variant as the default case using `#[xdr(enum_default)]`:
//!
//! ```rust
//! use xdrs::xdr_repr::{XDR, HasSpecification};
//!
//! #[derive(xdrs::XDR)]
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
//! let formatted = xdr_spec.to_string();
//! let expected = r#"enum ResponseType {
//! 	Success = 0,
//! 	NotFound = 1,
//! };
//!
//! union Response switch (ResponseType which)
//! {
//! case Success:
//! 	opaque data[100];
//! case NotFound:
//! 	void;
//! default:
//! 	void;
//! };
//!
//! "#;
//! assert_eq!(expected, formatted);
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
//! - [`Specification`]: A complete XDR specification containing multiple definitions
//! - [`Definition`]: A single definition (type or constant)
//! - [`TypeDef`]: A type definition (struct, enum, union, or typedef)
//! - [`Declaration`]: A field or variable declaration
//! - [`TypeSpec`]: A type specification
//!
//! ## Attributes
//!
//! When using `#[derive(XDR)]`, you can customize the generated XDR specification
//! with the following attributes:
//!
//! - **`#[xdr(name = "XdrName")]`**: Use a different name in the XDR specification
//!   than the Rust type name. Applied to structs or enums.
//!
//! - **`#[xdr(discrim_type = "TypeName")]`**: For enums (discriminated unions), specify
//!   the name of an existing XDR type to use as the discriminant type instead of
//!   auto-generating one. The default is `{EnumName}Type`.  Rust variant names
//!   are used as `case` labels in the emitted IDL, so they must match the
//!   member names of the external discriminant enum.
//!
//! - **`#[xdr(discrim_var = "field_name")]`**: For enums (discriminated unions), specify
//!   the name of the discriminant field. The default is `"which"`.
//!
//! - **`#[xdr(enum_default)]`**: Mark an enum variant as the default case in a
//!   discriminated union. Applied to individual enum variants.
//!
//! ## Traits
//!
//! Traits allow Rust types to describe their XDR representation:
//! - [`HasTypeDef`]: Types that can describe their XDR type definition
//! - [`HasTypeSpec`]: Types that can describe their XDR type specification
//! - [`Declare`]: Types that can create a declaration with a given name
//! - [`XDR`]: Types that can produce a complete XDR definition
//!
//! ## Type Introspection with `get_type_json`
//!
//! The [`Declaration::get_type_json`] and [`TypeSpec::get_type_json`] methods provide
//! a way to introspect XDR type structures programmatically by generating JSON-parseable
//! descriptions of type information.
//!
//! ### Example Usage
//!
//! ```rust
//! use xdr_repr::*;
//!
//! // Inspect a void declaration
//! let void_decl = Declaration::Void;
//! assert_eq!(void_decl.get_type_json(), r#"{ "type": "void" }"#);
//!
//! // Inspect a fixed-length array
//! let array_decl = Declaration::FixedArray {
//!     name: ident("items"),
//!     elt_ty: TypeSpec::Unsigned,
//!     len: unsigned(5),
//! };
//! assert_eq!(
//!     array_decl.get_type_json(),
//!     r#"{ "type": "fixed array", "element_type": { "type": "unsigned" }, "len": 5 }"#
//! );
//!
//! // Inspect an optional type
//! let optional_decl = Declaration::Optional {
//!     name: ident("value"),
//!     opt_ty: TypeSpec::Bool,
//! };
//! assert_eq!(
//!     optional_decl.get_type_json(),
//!     r#"{ "type": "optional", "element_type": { "type": "bool" } }"#
//! );
//! ```
//!
//! ### Limitations
//!
//! **IMPORTANT:** Embedded enum types are **not supported** and will cause a **panic** at runtime.
//! Embedded struct and union types are supported.
//!
//! ```rust,should_panic
//! # use xdr_repr::*;
//! // This will PANIC:
//! let enum_type = TypeSpec::Enum(EnumBody(vec![]));
//! let json = enum_type.get_type_json();  // PANIC!
//! ```
//!
//! ```rust
//! # use xdr_repr::*;
//! let union_type = TypeSpec::Union(Box::new(UnionBody {
//!     discriminant_decl: declare_int("which"),
//!     case_specs: vec![
//!         CaseSpec {
//!             cases: vec![signed(0)],
//!             decl: declare_type("ok", TypeSpec::Bool),
//!         },
//!         CaseSpec {
//!             cases: vec![signed(1)],
//!             decl: declare_unsigned("err"),
//!         },
//!     ],
//!     default_decl: None,
//! }));
//! let json = union_type.get_type_json();
//! assert!(json.contains(r#""type": "union""#));
//! ```
//!
//! **Workaround:** Define embedded enums at the top level in your XDR specification
//! and reference them by name using `TypeSpec::Ident`. Embedded struct and union types
//! are fully supported.
//!
//! ```rust
//! # use xdr_repr::*;
//! // Instead, reference enums by name:
//! let named_type = TypeSpec::Ident(ident("MyEnum"));
//! let json = named_type.get_type_json();  // OK: "MyEnum"
//! ```
//!
//! The output is valid JSON that can be parsed by standard JSON parsers. Type and field identifiers
//! must be valid XDR-like identifiers (ASCII letters, digits, underscores only, starting with a letter or underscore).
//! Leading underscores are allowed as an extension for Rust compatibility. Invalid identifiers will cause a
//! panic with a descriptive error message.
//!
//! See [`Declaration::get_type_json`] and [`TypeSpec::get_type_json`] for detailed
//! documentation and more examples.
//!
//! ## JSON Type Format Specification
//!
//! This section provides a complete specification of the JSON output format for XDR type introspection.
//!
//! ### Primitive Types
//!
//! Primitive types are represented as simple JSON objects with a `"type"` field:
//!
//! | XDR Type | Rust Type | JSON Output |
//! |----------|-----------|-------------|
//! | `void` | (none) | `{ "type": "void" }` |
//! | `int` | `i32` | `{ "type": "int" }` |
//! | `unsigned int` | `u32` | `{ "type": "unsigned" }` |
//! | `hyper` | `i64` | `{ "type": "hyper" }` |
//! | `unsigned hyper` | `u64` | `{ "type": "unsigned hyper" }` |
//! | `float` | `f32` | `{ "type": "float" }` |
//! | `double` | `f64` | `{ "type": "double" }` |
//! | `quadruple` | (none) | `{ "type": "quadruple" }` |
//! | `bool` | `bool` | `{ "type": "bool" }` |
//!
//! ### Named Types
//!
//! References to named types (custom types, enums, unions defined elsewhere):
//!
//! ```json
//! { "type": "TypeName" }
//! ```
//!
//! Type names are validated as XDR identifiers (no special characters allowed).
//!
//! ### Fixed Opaque (Byte Arrays)
//!
//! Fixed-length opaque data, represented by `[u8; N]` in Rust:
//!
//! ```json
//! {
//!   "type": "fixed opaque",
//!   "len": 32
//! }
//! ```
//!
//! - **`len`**: The fixed size in bytes (integer)
//!
//! ### Variable Opaque
//!
//! Variable-length opaque data:
//!
//! ```json
//! {
//!   "type": "variable opaque",
//!   "max": 1024
//! }
//! ```
//!
//! - **`max`**: Maximum size in bytes (integer)
//!
//! ### String
//!
//! Variable-length string:
//!
//! ```json
//! {
//!   "type": "string",
//!   "max": 255
//! }
//! ```
//!
//! - **`max`**: Maximum length in characters (integer)
//!
//! ### Fixed Array
//!
//! Fixed-length array of typed elements:
//!
//! ```json
//! {
//!   "type": "fixed array",
//!   "element_type": { "type": "unsigned" },
//!   "len": 10
//! }
//! ```
//!
//! - **`element_type`**: Element type (recursive type specification)
//! - **`len`**: Fixed length of the array (integer)
//!
//! For simple element types, the output is compact:
//! ```json
//! { "type": "fixed array", "element_type": { "type": "int" }, "len": 5 }
//! ```
//!
//! For complex element types (embedded structs), the output is pretty-printed:
//! ```json
//! {
//!   "type": "fixed array",
//!   "element_type": {
//!     "type": "struct",
//!     "fields": [...]
//!   },
//!   "len": 5
//! }
//! ```
//!
//! ### Variable Array
//!
//! Variable-length array:
//!
//! ```json
//! {
//!   "type": "variable array",
//!   "element_type": { "type": "int" },
//!   "max": 100
//! }
//! ```
//!
//! - **`element_type`**: Element type (recursive type specification)
//! - **`max`**: Maximum number of elements (integer)
//!
//! ### Optional
//!
//! Optional (nullable) value:
//!
//! ```json
//! {
//!   "type": "optional",
//!   "element_type": { "type": "bool" }
//! }
//! ```
//!
//! - **`optional_type`**: The wrapped type (recursive type specification)
//!
//! ### Union
//!
//! Discriminated unions:
//!
//! ```json
//! {
//!   "type": "union",
//!   "discriminant": {
//!     "name": "which",
//!     "element_type": { "type": "int" }
//!   },
//!   "cases": [
//!     {
//!       "values": [0, "RECOVERABLE"],
//!       "element_type": { "type": "bool" }
//!     },
//!     {
//!       "values": [1],
//!       "element_type": { "type": "unsigned" }
//!     }
//!   ],
//!   "default": {
//!     "name": "fallback",
//!     "element_type": { "type": "bool" }
//!   }
//! }
//! ```
//!
//! - **`discriminant`**: Named declaration object for the union discriminant
//! - **`cases`**: Array of case objects
//!   - **`values`**: Discriminant values for the case; numeric labels serialize as JSON numbers, identifier-valued labels serialize as JSON strings
//!   - **`element_type`**: Case payload type (recursive type specification)
//! - **`default`**: Optional default declaration object, present when the union has a default branch
//!
//! ### Struct
//!
//! Struct with named fields:
//!
//! ```json
//! {
//!   "type": "struct",
//!   "fields": [
//!     {
//!       "name": "id",
//!       "element_type": { "type": "unsigned" }
//!     },
//!     {
//!       "name": "data",
//!       "element_type": { "type": "fixed opaque", "len": 16 }
//!     }
//!   ]
//! }
//! ```
//!
//! - **`fields`**: Array of field objects
//!   - **`name`**: Field name (validated as XDR identifier)
//!   - **`element_type`**: Field type (recursive type specification)
//!
//! Empty structs:
//! ```json
//! {
//!   "type": "struct",
//!   "fields": []
//! }
//! ```
//!
//! ### Tuple Types
//!
//! Rust tuples are represented as structs with auto-generated field names:
//!
//! ```rust
//! # use xdr_repr::*;
//! let tuple_decl = <(i32, u32, bool)>::declare(ident("point"));
//! let json = tuple_decl.get_type_json();
//! ```
//!
//! Produces:
//! ```json
//! {
//!   "type": "struct",
//!   "fields": [
//!     { "name": "unnamed_0", "element_type": { "type": "int" } },
//!     { "name": "unnamed_1", "element_type": { "type": "unsigned" } },
//!     { "name": "unnamed_2", "element_type": { "type": "bool" } }
//!   ]
//! }
//! ```
//!
//! Field names follow the pattern `unnamed_0`, `unnamed_1`, etc.
//!
//! ### Nested Structures
//!
//! Types can be nested arbitrarily. For example, a struct containing an array of structs:
//!
//! ```json
//! {
//!   "type": "struct",
//!   "fields": [
//!     {
//!       "name": "items",
//!       "element_type": {
//!         "type": "fixed array",
//!         "element_type": {
//!           "type": "struct",
//!           "fields": [
//!             { "name": "id", "element_type": { "type": "unsigned" } },
//!             { "name": "value", "element_type": { "type": "int" } }
//!           ]
//!         },
//!         "len": 10
//!       }
//!     }
//!   ]
//! }
//! ```
//!
//! ### Special Array Handling
//!
//! - **Byte arrays** (`[u8; N]`): Represented as `"fixed opaque"` (not `"fixed array"`)
//! - **Typed arrays** (`[T; N]` where T ≠ u8): Represented as `"fixed array"` with element type
//! - **Nested byte arrays** (`[[u8; M]; N]`): Outer array is `"fixed array"` with element type being a struct containing `"fixed opaque"`
//!
//! Example:
//! ```rust
//! # use xdr_repr::*;
//! # use xdrs::*;
//! // [u8; 32] → fixed opaque
//! let byte_array = <[u8; 32]>::declare(ident("hash"));
//! assert!(byte_array.get_type_json().contains("fixed opaque"));
//!
//! // [u32; 10] → fixed array
//! let int_array = <[u32; 10]>::declare(ident("numbers"));
//! assert!(int_array.get_type_json().contains("fixed array"));
//! ```
//!
//! ### Complete Example
//!
//! ```rust
//! # #[cfg(feature = "derive")]
//! # {
//! use xdr_repr::*;
//! use xdrs::*;
//!
//! #[derive(XDR)]
//! struct Transaction {
//!     _id: u64,
//!     _from: [u8; 32],
//!     _to: [u8; 32],
//!     _amount: i64,
//!     _signature: [u8; 64],
//! }
//!
//! let type_def = Transaction::type_def();
//! if let TypeDef::Struct(_, body) = type_def {
//!     let json = TypeSpec::Struct(body).get_type_json();
//!
//!     // The JSON is valid and parseable
//!     let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
//!     assert_eq!(parsed["type"], "struct");
//!     assert_eq!(parsed["fields"].as_array().unwrap().len(), 5);
//! }
//! # }
//! ```
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
pub mod result;
pub mod rpc;
pub mod ty;
pub mod xdr;

// export the types that represent XDR specs
pub use xdr::*;

// export XDR-spec constructors
pub use construct::*;

// export the traits that produce XDR representations
pub use result::*;
pub use ty::*;
