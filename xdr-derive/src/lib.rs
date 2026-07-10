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
//! use xdrs::xdr_repr::XDR;
//!
//! #[derive(xdrs::XDR)]
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
//!
//! - `#[xdr(from_type = "Type")]` - Deserialize to the intermediate type
//!   "Type" before converting into the target type via `Into`.
//! - `#[xdr(via = "Type")]` - Same as above.
//!
//! ### ReadXDR Notes
//!
//! The lifetime name `'_r` is reserved for the generated reader lifetime. Types
//! deriving `ReadXDR` must not declare their own generic lifetime with that
//! name.
//!
//! When deriving `WriteXDR`:
//!
//! - `#[xdr(into_type = "Type")]` - Convert (via `Into`) a different type before
//!   serializing.
//! - `#[xdr(via = "Type")]` - Same as above.
//!
//! When deriving `ReadXDR`, `WriteXDR`, or `XDR` on an enum, the trailing
//! variant may be marked `#[xdr(default_variant)]` to act as the union's
//! default arm; see the individual derive macros for details.
//!
//! ### Crate path override
//!
//! All three derives accept `#[xdr(crate = "path")]` to override the default
//! `::xdrs` import path (useful when the dependency is renamed or re-exported).
//!
//! [RFC-4506]: https://datatracker.ietf.org/doc/html/rfc4506

#![warn(missing_docs)]

mod attrs;
mod generate;
mod generate_const;
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
/// - `#[xdr(into_type = "Type")]` - Convert (via `Into`) a different type before
///   serializing.
/// - `#[xdr(via = "Type")]` - Same as above (also implies `from_type`).
/// - `#[xdr(default_variant)]` - On an enum variant: mark it as the union's
///   default arm. The variant must be the trailing variant and must not
///   specify a discriminant, and its first field must be `i32`, which stores
///   the wire discriminant and is emitted verbatim when encoding.
/// - `#[xdr(crate = "path::to::xdrs")]` - Override the path to the `xdrs` crate
///   (useful when re-exporting from another crate).
///
/// # Limitations
///
/// - **No recursive types.** XDR encoding is not self-describing; a recursive
///   type will loop at runtime. The macro does not detect or reject recursion.
///   In particular, `#[xdr(into_type = "Self")]` is an infinite loop — the
///   generated impl converts `self` into `Self` (a no-op) and re-enters the
///   same encoder until the stack overflows. The surrogate type must always be
///   a **distinct**, non-recursive type.
/// - **All field types must be serializable.** Every field (or the surrogate
///   type, when using `into_type`/`via`) must independently satisfy `WriteXDR`.
///   The macro copies the input type's generic bounds verbatim — it does **not**
///   infer or propagate trait bounds required by a surrogate type's own generic
///   parameters. For example, `#[xdr(into_type = "VecM<T, 8>")]` on a generic
///   `Wrapper<T>` will not automatically add `T: WriteXDR`; you must supply the
///   bound yourself.
/// - **Default variant discriminant is not validated.** When a `Default(i32, ..)`
///   variant is encoded, the writer emits the stored discriminant without
///   checking whether it collides with a concrete variant. Encoding a `Default`
///   with a known discriminant value will produce bytes that the reader decodes
///   as the concrete variant, not as `Default`.
///
#[proc_macro_derive(WriteXDR, attributes(xdr, xdrs))]
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
/// - `#[xdr(from_type = "Type")]` - Deserialize to the intermediate type
///   "Type" before converting into the target type via `Into`.
/// - `#[xdr(via = "Type")]` - Same as above (also implies `into_type`).
/// - `#[xdr(default_variant)]` - On an enum variant (which must be the
///   trailing variant): mark it as the union's default arm. Discriminants that
///   match no other variant decode to this variant, with the wire discriminant
///   stored in its first field (which must be `i32`). Without a default
///   variant, unknown discriminants fail with `ReadError::BadDiscriminator`.
/// - `#[xdr(crate = "path::to::xdrs")]` - Override the path to the `xdrs` crate
///   (useful when re-exporting from another crate).
///
/// # Limitations
///
/// - **No recursive types.** XDR decoding is not self-describing; a recursive
///   type will loop at runtime. The macro does not detect or reject recursion.
///   In particular, `#[xdr(from_type = "Self")]` is an infinite loop — the
///   generated impl reads a `Self` and converts it into `Self` (a no-op),
///   re-entering the same decoder until the stack overflows. The surrogate type
///   must always be a **distinct**, non-recursive type.
/// - **All field types must be deserializable.** Every field (or the surrogate
///   type, when using `from_type`/`via`) must independently satisfy `ReadXDR`.
///   The macro copies the input type's generic bounds verbatim — it does **not**
///   infer or propagate trait bounds required by a surrogate type's own generic
///   parameters. For example, `#[xdr(from_type = "VecM<T, 8>")]` on a generic
///   `Wrapper<T>` will not automatically add `T: ReadXDR`; you must supply the
///   bound yourself.
/// - **The lifetime name `'_r` is reserved.** The generated `ReadXDR` impl uses
///   `'_r` as its reader lifetime. A type deriving `ReadXDR` must not declare a
///   generic lifetime parameter with that same name; choose another name for
///   user-defined lifetimes.
///
#[proc_macro_derive(ReadXDR, attributes(xdr, xdrs))]
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
///   for enums; the value must be a single XDR identifier (e.g. `"MyEnumType"`),
///   not a multi-word type expression such as `"unsigned int"`. Passing a
///   multi-word string causes a panic during schema emission.
/// - `#[xdr(discrim_var = "field_name")]` - Specify a custom discriminant
///   field name
/// - `#[xdr(default_variant)]` - On an enum variant: mark it as the union's
///   `default:` arm in the generated XDR specification. The variant must be
///   the trailing variant, must not specify a discriminant, and its first
///   field must be `i32`.
///
/// All three derive macros (`ReadXDR`, `WriteXDR`, `XDR`) accept:
///
/// - `#[xdr(crate = "path::to::xdrs")]` - Override the path to the `xdrs` crate
///   (useful when re-exporting from another crate).
///
/// # Limitations
///
/// - **Raw identifiers produce invalid XDR IDL.** A field declared as
///   `r#type: u32` emits `r#type` into the schema, which is not a valid XDR
///   identifier. Use `#[xdr(name = "type_field")]` (or a rename that avoids the
///   reserved Rust keyword entirely) to give the field a valid XDR name.
///
/// - **Reference fields support only one indirection layer.** `xdr-repr`
///   implements `HasTypeSpec` for `&T` and `&mut T`, but not for `&&T` or
///   deeper nesting. A struct field of type `&&u32` or `&&bool` compiles for
///   `WriteXDR` (which delegates recursively) but fails to derive `XDR` schema
///   metadata. Flatten the reference to a single `&T` if schema support is
///   needed.
///
/// - **Const-generic monomorphs share the XDR type name.** When a struct
///   contains two fields of the same generic wrapper type with different const
///   bounds — for example `Packet<4>` and `Packet<8>` — the generated XDR
///   schema names both parent fields `Packet`, creating a collision. Define
///   distinct concrete type aliases (`type Packet4 = Packet<4>;`) so each
///   monomorph gets its own XDR name.
///
#[proc_macro_derive(XDR, attributes(xdr, xdrs))]
pub fn derive_xdr(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);

    generate_type::xdr(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Derives const-compatible XDR encoding methods for compile-time use, plus a
/// runtime [`ConstWriteXDR`](../xdrs/trait.ConstWriteXDR.html) trait impl.
///
/// Generates four inherent `const fn` methods on the type:
/// - `const_write_cxs(&self, w: &mut cxs::Cursor<'_>)`
/// - `const_write_txs(&self, w: &mut txs::Cursor<'_>)`
/// - `const_cxs_size(&self) -> usize`
/// - `const_txs_size(&self) -> usize`
///
/// These methods can be used in `const` blocks to encode values at compile
/// time. The writers are slice-backed `cxs::Cursor` / `txs::Cursor` rather
/// than the buffer-const-generic `ConstWriter`; construct one over a
/// stack buffer whose length is taken from the size methods:
///
/// ```rust,ignore
/// static ENCODED: [u8; MY_VAL.const_txs_size()] = {
///     let mut buf = [0u8; MY_VAL.const_txs_size()];
///     let mut w = xdrs::txs::Cursor::new(&mut buf);
///     MY_VAL.const_write_txs(&mut w);
///     assert!(w.pos() == MY_VAL.const_txs_size());
///     buf
/// };
/// ```
///
/// # Trait impl and worst-case sizes
///
/// The derive also emits an `impl ConstWriteXDR for YourType`. Its `write_cxs` /
/// `write_txs` / `cxs_size` / `txs_size` methods forward to the inherent
/// `const fn`s above, and it provides the associated consts `MAX_CXS_LEN` /
/// `MAX_TXS_LEN` — the worst-case encoded length over all values of the type
/// (`None` when unbounded, e.g. a field is a `&[T]` or `String`). These consts
/// are `add`/`max` folds over the fields' own `MAX_*_LEN`, so every field type
/// (and every enum-variant field type) must itself implement `ConstWriteXDR`.
///
/// # `#[xdrs(runtime_only)]`
///
/// Apply `#[xdrs(runtime_only)]` to skip the inherent `const fn`s and emit only
/// the trait impl, with direct (non-`const`) method bodies. Use this for types
/// whose fields have no inherent const encoders — for example a struct holding a
/// [`StringM`](../xdrs/struct.StringM.html), whose encoding is not `const`-compatible. The `MAX_*_LEN`
/// consts are still provided.
///
/// # Limitations
///
/// - Does not support `#[xdr(into_type)]` or `#[xdr(via)]` attributes on the
///   derive input, struct fields, or enum variant fields. The `via`/`into_type`
///   conversions require `Into::into()` which is not `const fn`.
/// - Supports lifetime and const generics, but not type parameters
/// - Field types may use recursive concrete shapes inline, including `Option<T>`,
///   `Result<T, E>`, `&[T]`, `[T; N]`, and tuples, as long as leaf types are
///   concrete and const-encodable
///
/// # Generated code hygiene
///
/// The generated `const fn` methods are syntax-directed: type recognition uses
/// raw identifiers rather than Rust's resolved types, and generated code emits
/// unqualified names into the call site scope. This means:
///
/// - **Use bare primitive names.** `u32` is recognized; `core::primitive::u32`
///   is not and will be treated as a named leaf type.
/// - **Do not shadow built-in types.** A local `type u8 = MyType;` will cause
///   the opaque byte fast-path to misfire. Similarly, shadowing `str` will
///   confuse string detection.
/// - Length guards use `::core::assert!()` to resist call-site macro shadowing.
/// - **Avoid naming const generics `i`, `arr`, `slice`, or `total`.** The
///   generated array/slice iteration uses these as local bindings, and Rust
///   does not allow a `let` binding to shadow an in-scope const parameter.
/// - **Avoid naming modules `cxs` or `txs` in discriminant paths.** The
///   generated const block imports `use xdrs::cxs; use xdrs::txs;`, which can
///   capture discriminant expressions whose paths start with those segments.
///
/// # Standard library type recognition
///
/// The macro recognizes `Option`, `Result`, `PhantomData`, and `Infallible` by
/// their **last path segment** (e.g. both `Option<T>` and `std::option::Option<T>`
/// match). This is a syntactic check — type aliases are not resolved. A field
/// declared as `type MyOpt = Option<Foo>; ..  my_field: MyOpt` will **not** be
/// recognized as an `Option` and will instead be treated as a named leaf whose
/// type must provide its own `const_cxs_size` / `const_write_cxs` / etc. inherent
/// methods (i.e. it must itself derive `ConstWriteXDR` or provide them manually).
/// The same applies to `PhantomData` — if you need a type-alias, wrap it in a
/// newtype and derive `ConstWriteXDR` on the newtype instead.
///
#[proc_macro_derive(ConstWriteXDR, attributes(xdr, xdrs))]
pub fn derive_const_write_xdr(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);

    generate_const::const_write_xdr(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}
