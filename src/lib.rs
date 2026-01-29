//! # _XDRS_ (XDR Serialization)
//!
//! This crate provides:
//!
//! - Traits [`ReadXDR`] and [`WriteXDR`], for converting between Rust values
//!   and the [XDR][RFC-4506] data model.
//!
//! - Traits [`Reader`] and [`Writer`], for converting between the
//!   [XDR][RFC-4506] data model and some serialization format.
//!
//! - Implementations of [`Reader`] and [`Writer`]:
//!   - [CXS](cxs) ("Compact XDR Serialization")
//!   - [TXS](txs) ("Traditional XDR Serialization")
//!
//! The library is `no_std`, but a few extra things can be
//! enabled with feature `alloc`.
//!
//! ## Features
//!
//! This crate supports several optional features:
//!
//! ### CXS options
//!
//! - **`permissive`**: When reading CXS data, don't reject (as is the default)
//!   integers using non-minimal-size encoding.
//!
//! ### Allocation
//!
//! - **`alloc`**: Enables allocation-based interfaces:
//!   - `Vec<u8>` and `Vec<T>` implementations of [`ReadXDR`] and [`WriteXDR`]
//!   - `String` implementations of [`ReadXDR`] and [`WriteXDR`]
//!   - `BufWriter` types in [`cxs`] and [`txs`] modules
//!     for growable buffers
//!   - `encode()` functions that return owned `Vec<u8>`
//!
//! ### Extended types
//!
//! - **`int128`**: Enables convenient extensions to the XDR specification:
//!   support for `i128` and `u128`. These types are encoded as two 64-bit
//!   integers (high bits and low bits).
//!
//! ### Optional dependencies
//!
//! - **`derive`**: Enables derive macros from the `xdr-derive` crate:
//!   - `#[derive(ReadXDR)]`: Automatically implement [`ReadXDR`]
//!   - `#[derive(WriteXDR)]`: Automatically implement [`WriteXDR`]
//!   - `#[derive(XDR)]`: Automatically implement XDR type representation
//!     traits
//!
//! - **`repr`** (requires `alloc`): Enables the `xdr-repr` crate for XDR
//!   specification representation. Provides types and traits for describing XDR
//!   type definitions.
//!
//! - **`xdr_common`**: Enables support for types from the `xdr_common` crate:
//!   - `xdr_common::Box<T>` implementations
//!   - `xdr_common::BytesM<MAX>` and `xdr_common::VecM<T, MAX>` implementations
//!
//! - **`ns`**: Enables namespace support in `xdr-repr` for C++-like scoping of
//!   XDR definitions.
//!
//! ### Deprecated features
//!
//! - **`strict`**: Now ignored. The default is strict processing; use
//!   feature `permissive` to disable that.
//! - **`cxs`**: Now ignored.  CXS is now always available.
//! - **`txs`**: Now ignored.  TXS is now always available.
//! - **`extras`**: Now ignored.  See above for `i128/u128` features.
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

// The abstract interface for serialization formats
pub mod serialize;

// CXS serialization
pub mod cxs;

// TXS serialization
pub mod txs;

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

#[cfg(feature = "int128")]
mod int128;

pub use xdr_common::*;
