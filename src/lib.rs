//! # _XDRS_ (XDR Serialization)
//!
//! This crate provides:
//!
//! - Traits [`ReadXDR`] and [`WriteXDR`], for converting between Rust values
//! and the [XDR][RFC-4506] data model.
//!
//! - Traits [`Reader`] and [`Writer`], for converting between the
//! [XDR][RFC-4506] data model and some serialization format.
//!
//! - Implementations of [`Reader`] and [`Writer`]:
//!   - [CXS](serialize_cxs) ("Compact XDR Serialization")
//!   - [TXS](serialize_txs) ("Traditional XDR Serialization")
//!
//! The library is `no_std`, but a few extra things can be
//! enabled with feature `alloc`.
//!
//! ## Features
//!
//! This crate supports several optional features:
//!
//! ### Serialization formats
//!
//! - **`cxs`** (enabled by default): Enables the CXS (Compact XDR
//! Serialization) codec. Provides the [`serialize_cxs`] module with compact,
//! variable-length encoding.
//!
//! - **`txs`** (enabled by default): Enables the TXS (Traditional XDR
//! Serialization) codec. Provides the [`serialize_txs`] module with standard
//! [RFC 4506] encoding.
//!
//! - **`strict`** (enabled by default): When reading CXS data, requires that
//! integers use the minimal-size encoding. Rejects non-minimal encodings as
//! errors.
//!
//! ### Allocation
//!
//! - **`alloc`**: Enables allocation-based interfaces:
//!   - `Vec<u8>` and `Vec<T>` implementations of [`ReadXDR`] and [`WriteXDR`]
//!   - `String` implementations of [`ReadXDR`] and [`WriteXDR`]
//!   - `BufWriter` types in [`serialize_cxs`] and [`serialize_txs`] modules
//!   for growable buffers
//!   - `encode()` functions that return owned `Vec<u8>`
//!
//! ### Extended types
//!
//! - **`extras`**: Enables convenient extensions to the XDR specification:
//!   - `i128` and `u128` support (using variable-length opaque encoding by
//!   default)
//!
//! - **`int128_xdr`** (requires `extras`): Uses an alternate encoding for
//! 128-bit integers, encoding them as two 64-bit integers (high bits and low
//! bits) instead of variable-length opaque.
//!
//! ### Optional dependencies
//!
//! - **`derive`**: Enables derive macros from the `xdr-derive` crate:
//!   - `#[derive(ReadXDR)]`: Automatically implement [`ReadXDR`]
//!   - `#[derive(WriteXDR)]`: Automatically implement [`WriteXDR`]
//!   - `#[derive(XDR)]`: Automatically implement XDR type representation
//!   traits
//!
//! - **`repr`** (requires `alloc`): Enables the `xdr-repr` crate for XDR
//! specification representation. Provides types and traits for describing XDR
//! type definitions.
//!
//! - **`xdr_common`**: Enables support for types from the `xdr_common` crate:
//!   - `xdr_common::Box<T>` implementations
//!   - `xdr_common::BytesM<MAX>` and `xdr_common::VecM<T, MAX>` implementations
//!
//! - **`ns`**: Enables namespace support in `xdr-repr` for C++-like scoping of
//! XDR definitions.
//!
//! [RFC-4506]: https://datatracker.ietf.org/doc/html/rfc4506

#![no_std]
#![allow(ambiguous_glob_reexports)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![allow(hidden_glob_reexports)]

#[cfg(feature = "alloc")]
extern crate alloc;

// Traits for translating Rust values to and from the XDR data model
pub mod discrim;
pub mod read_xdr;
pub mod write_xdr;

pub mod serialize;

#[cfg(feature = "cxs")]
#[cfg_attr(docsrs, doc(cfg(feature = "cxs")))]
pub mod serialize_cxs;

#[cfg(feature = "txs")]
#[cfg_attr(docsrs, doc(cfg(feature = "txs")))]
pub mod serialize_txs;

pub mod mu; // used by integration tests

pub(crate) mod valid_u32;

pub use discrim::*;
pub use read_xdr::*;
pub use write_xdr::*;

pub use serialize::reader::Error as ReadError;
pub use serialize::reader::Result as ReadResult;
pub use serialize::reader::*;

pub use serialize::writer::Error as WriteError;
pub use serialize::writer::Result as WriteResult;
pub use serialize::writer::*;

#[cfg(feature = "repr")]
#[cfg_attr(docsrs, doc(cfg(feature = "repr")))]
pub use xdr_repr;

#[cfg(feature = "derive")]
#[cfg_attr(docsrs, doc(cfg(feature = "derive")))]
pub use xdr_derive::*;

#[cfg(feature = "xdr_common")]
#[cfg_attr(docsrs, doc(cfg(feature = "xdr_common")))]
pub use xdr_common;

#[cfg(feature = "xdr_common")]
#[cfg_attr(docsrs, doc(cfg(feature = "xdr_common")))]
pub use xdr_common::*;

#[cfg(all(not(feature = "extras"), feature = "int128_xdr"))]
compile_error!("Feature \"int128_xdr\" requires also requesting support for \
    non-standard types by enabling feature \"extras\".");
