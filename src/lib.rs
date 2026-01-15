//! XDRS (XDR Serialization) - A Rust library for serializing and deserializing XDR data.
//!
//! This crate provides:
//!
//! - Traits [`ReadXDR`] and [`WriteXDR`], for converting between Rust values
//!   and the [XDR][RFC-4506] (as defined in RFC 4506) data model
//!
//! - Traits [`serialize::reader::Reader`] and [`serialize::writer::Writer`], for converting
//!   between the [XDR][RFC-4506] data model and some serialization format
//!
//! - Implementations of [`serialize::reader::Reader`] and [`serialize::writer::Writer`]:
//!   - [CXS](serialize_cxs) (Compact XDR Serialization) - see module [`serialize_cxs`] for details
//!   - TXS (Traditional XDR Serialization) (see module [`serialize_txs`])
//!
//! The library is `no_std`, but a few extra things can be
//! enabled with feature `alloc`.
//!
//! [RFC-4506]: https://datatracker.ietf.org/doc/html/rfc4506

#![no_std]
#![allow(ambiguous_glob_reexports)]
#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(feature = "alloc")]
extern crate alloc;

// Traits for translating Rust values to and from the XDR data model
pub mod discrim;
pub mod read_xdr;
pub mod write_xdr;

// The abstract interface for serialization formats
pub mod serialize;

// CXS serialization
#[cfg(feature = "cxs")]
#[cfg_attr(docsrs, doc(cfg(feature = "cxs")))]
pub mod serialize_cxs;

// TXS serialization
#[cfg(feature = "txs")]
#[cfg_attr(docsrs, doc(cfg(feature = "txs")))]
pub mod serialize_txs;

/// Utilities for working with `MaybeUninit` types.
///
/// This module provides helper functions for safely working with uninitialized memory,
/// used internally by the serialization code.
pub mod mu; // used by integration tests

/// Compile-time validation that a const generic usize fits in a u32.
pub(crate) mod valid_u32;




pub use crate::read_xdr::*;
pub use crate::write_xdr::*;

pub use crate::discrim::*;

pub use crate::serialize::reader::Error as ReadError;
pub use crate::serialize::reader::Result as ReadResult;
pub use crate::serialize::reader::*;

pub use crate::serialize::writer::Error as WriteError;
pub use crate::serialize::writer::Result as WriteResult;
pub use crate::serialize::writer::*;

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
