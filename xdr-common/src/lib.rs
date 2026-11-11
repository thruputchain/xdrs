//! The stable layer of [XDRS](https://docs.rs/thruput-xdrs): everything a
//! type's public API exposes when it implements XDR.
//!
//! - Length-constrained collections: [`VecM`], [`BytesM`] and [`StringM`].
//!
//! - Traits [`ReadXDR`] and [`WriteXDR`], for converting between Rust values
//!   and the [XDR][RFC-4506] data model, with their implementations for core,
//!   `alloc`, `heapless` and this crate's types.
//!
//! - Trait [`EncodesAs`], for declaring that a type encodes to the same bytes
//!   as some XDR type, so it can be passed where that type is expected.
//!
//! - Trait [`XDRDiscriminant`], for the types that switch a union.
//!
//! - Traits [`Reader`], [`Writer`] and [`WriterUnchecked`], the interface
//!   between the XDR data model and a serialization format, with the
//!   [`ReadError`] and [`WriteError`] they report.
//!
//! - Per format, [`cxs::ConstMaxSize`] / [`txs::ConstMaxSize`] (worst-case
//!   encoded sizes) and [`cxs::ConstWriter`] / [`txs::ConstWriter`]
//!   (encoding in `const` contexts), which `#[derive(ConstXDRSize)]` and
//!   `#[derive(ConstWriteXDR)]` target.
//!
//! The codecs that implement [`Reader`] and [`Writer`] (buffer readers and
//! writers, `encode`/`decode`) live in `thruput-xdrs`, which re-exports
//! everything here at the same paths. Depend on `thruput-xdrs` to encode and
//! decode. This crate is versioned separately so that the codecs can make
//! breaking releases without breaking every crate whose types implement
//! these traits; see "Versioning and releases" in the XDRS README.
//!
//! # Features
//!
//! - **`alloc`** - Heap allocation support (required for most of the
//!   collection functionality), and `Vec`, `String`, `Box`, `Rc` and `Arc`
//!   implementations of the traits.
//! - **`serde`** - serde serialization/deserialization of the collections
//!   (implies `alloc`).
//! - **`schemars`** - JSON Schema generation via schemars (implies `alloc`).
//! - **`heapless`** - Trait implementations for `heapless::Vec` and
//!   `heapless::String`.
//! - **`detailed-errors`** - [`ReadError`] keeps its precise variants and
//!   diagnostic payloads; without it, it is a unit struct. Off here, and on
//!   by default in `thruput-xdrs`, so that depending on this crate directly
//!   never changes the error representation a build gets.
//!
//! # no_std Support
//!
//! This crate is `no_std`. Enable the `alloc` feature for heap-allocated
//! collections.
//!
//! # Examples
//!
//! ```ignore
//! use xdr_common::{VecM, BytesM, StringM};
//!
//! // Create a vector with max length of 10
//! let v: VecM<u32, 10> = vec![1, 2, 3].try_into()?;
//!
//! // Create a byte array with max length of 100
//! let b: BytesM<100> = b"hello".as_slice().try_into()?;
//!
//! // Create a string with max length of 50
//! let s: StringM<50> = "world".try_into()?;
//! ```
//!
//! [RFC-4506]: https://datatracker.ietf.org/doc/html/rfc4506

#![cfg_attr(docsrs, feature(doc_cfg))]
#![no_std]
#![warn(missing_docs)]
#![allow(ambiguous_glob_reexports)]

#[cfg(target_pointer_width = "16")]
compile_error!("xdrs does not support 16-bit targets");

#[cfg(feature = "alloc")]
extern crate alloc;

mod cfg;
pub use cfg::{Box, Vec, box_as_ref};

mod error;
pub use error::*;

/// The maximum number of bytes a decoder will preallocate for a
/// variable-length collection before reading its elements.
///
/// Decoded lengths are untrusted, so capacity reservations are capped at
/// this many bytes to prevent memory-exhaustion attacks from maliciously
/// large length prefixes; collections grow normally beyond it.
#[cfg(feature = "alloc")]
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
pub const MAX_PREALLOC_BYTES: usize = 0x1_0000;

mod vecm;
pub use vecm::*;

mod bytesm;
pub use bytesm::*;

mod stringm;
pub use stringm::*;

#[cfg(feature = "serde")]
#[cfg_attr(docsrs, doc(cfg(feature = "serde")))]
pub mod serde_hex;

// Traits for translating Rust values to and from the XDR data model
pub mod discrim;
pub mod encodes_as;
pub mod read_xdr;
pub mod write_xdr;

// The abstract interface for serialization formats
pub mod serialize;

/// CXS ("Compact XDR Serialization") const-context support: worst-case sizes
/// and the const writer. The codec itself is `xdrs::cxs`.
pub mod cxs {
    pub mod const_writer;
    pub use const_writer::ConstWriter;

    pub mod max_size;
    pub use max_size::ConstMaxSize;
}

/// TXS ("Traditional XDR Serialization") const-context support: worst-case
/// sizes and the const writer. The codec itself is `xdrs::txs`.
pub mod txs {
    pub mod const_writer;
    pub use const_writer::ConstWriter;

    pub mod max_size;
    pub use max_size::ConstMaxSize;
}

pub(crate) mod mu;

pub(crate) mod valid_u32;

pub use discrim::*;
pub use encodes_as::*;
pub use read_xdr::*;
pub use write_xdr::*;

pub use serialize::reader::ReadError;
pub use serialize::reader::ReadResult;
pub use serialize::reader::*;

pub use serialize::writer::WriteError;
pub use serialize::writer::WriteResult;
pub use serialize::writer::*;

pub use serialize::writer_unchecked::*;

mod int128;

#[cfg(feature = "heapless")]
#[cfg_attr(docsrs, doc(cfg(feature = "heapless")))]
mod heapless;
