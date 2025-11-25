#![no_std]
#![allow(ambiguous_glob_reexports)]

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
pub use xdr_repr;

#[cfg(feature = "derive")]
pub use xdr_derive::*;

#[cfg(feature = "int128")]
pub mod int128;

#[cfg(all(feature = "int128_xdr", not(feature = "int128")))]
pub mod int128_xdr;

#[cfg(all(feature = "int128_xdr", feature = "int128"))]
compile_error!("Features \"int128\" and \"int128_xdr\" are mutually exclusive, but both have been enabled");

#[cfg(feature = "xdr_common")]
pub use xdr_common;

#[cfg(feature = "xdr_common")]
pub use xdr_common::*;
