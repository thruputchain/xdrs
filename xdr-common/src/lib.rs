//! Common types for XDR (External Data Representation) encoding and decoding.
//!
//! This crate provides length-constrained collection types for XDR serialization:
//! - [`VecM`] - Generic vector with maximum length constraint
//! - [`BytesM`] - Byte vector with maximum length constraint
//! - [`StringM`] - String (as bytes) with maximum length constraint
//!
//! # Features
//!
//! - **`alloc`** - Enables heap allocation support (required for most functionality)
//! - **`std`** - Enables standard library support (implies `alloc`)
//! - **`serde`** - Enables serde serialization/deserialization (implies `alloc`)
//! - **`schemars`** - Enables JSON Schema generation via schemars (implies `alloc`)
//!
//! # no_std Support
//!
//! This crate is `no_std` by default. Enable the `alloc` feature for heap-allocated
//! collections, or `std` for full standard library support.
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

#![cfg_attr(docsrs, feature(doc_cfg))]
#![no_std]
#![warn(missing_docs)]

#[cfg(feature = "std")]
extern crate std;

#[cfg(feature = "alloc")]
extern crate alloc;

mod cfg;
pub use cfg::{Box, Vec};

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
