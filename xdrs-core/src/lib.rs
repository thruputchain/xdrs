//! # _XDRS core_
//!
//! The stable trait layer of [XDRS](https://docs.rs/thruput-xdrs): the part
//! of the library that a type's public API exposes when it implements XDR.
//!
//! - Traits [`ReadXDR`] and [`WriteXDR`], for converting between Rust values
//!   and the [XDR][RFC-4506] data model, with their implementations for core,
//!   `alloc`, `heapless` and `xdr_common` types.
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
//! decode; this crate exists so that the codecs can make breaking releases
//! without breaking every crate whose types implement these traits.
//!
//! ## Features
//!
//! - **`alloc`**: `Vec`, `String`, `Box`, `Rc` and `Arc` implementations.
//! - **`heapless`**: implementations for `heapless::Vec` and
//!   `heapless::String`.
//! - **`detailed-errors`** (default): [`ReadError`] keeps its precise variants
//!   and diagnostic payloads; without it, it is a unit struct.
//!
//! [RFC-4506]: https://datatracker.ietf.org/doc/html/rfc4506

#![no_std]
#![warn(missing_docs)]
#![allow(ambiguous_glob_reexports)]
#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(target_pointer_width = "16")]
compile_error!("xdrs does not support 16-bit targets");

#[cfg(feature = "alloc")]
extern crate alloc;

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

pub use xdr_common::*;

#[cfg(feature = "heapless")]
#[cfg_attr(docsrs, doc(cfg(feature = "heapless")))]
mod heapless;
