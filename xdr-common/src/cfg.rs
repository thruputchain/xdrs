// Copied from https://github.com/stellar/xdrgen/blob/master/lib/xdrgen/generators/rust/src/types.rs

#![allow(unused_imports)]
#![allow(dead_code)]

#[cfg(all(feature = "serde", feature = "alloc"))]
pub(crate) use serde::de::{Deserialize, Deserializer, Error as DeError, SeqAccess, Visitor};
#[cfg(all(feature = "serde", feature = "alloc"))]
pub(crate) use serde::ser::{Serialize, Serializer};

// When feature std is turned off, but feature alloc is turned on import the
// alloc crate and use its Box and Vec types.
#[cfg(feature = "alloc")]
extern crate alloc;
#[cfg(feature = "alloc")]
pub(crate) use alloc::{
    borrow::ToOwned,
    string::{FromUtf8Error, String},
};

/// A pointer type that uniquely owns a heap allocation of type `T`.
///
/// # Feature Variants
///
/// - **With `alloc` feature**: Standard heap-allocated `Box<T>` from the alloc crate
/// - **Without `alloc` feature**: Type alias for `&'static T`, requires static data
#[cfg_attr(docsrs, doc(cfg(all())))]
#[cfg(feature = "alloc")]
pub type Box<T> = alloc::boxed::Box<T>;

#[cfg_attr(docsrs, doc(hidden))]
#[cfg(not(feature = "alloc"))]
pub type Box<T> = &'static T;

/// A contiguous growable array type.
///
/// # Feature Variants
///
/// - **With `alloc` feature**: Standard heap-allocated `Vec<T>` from the alloc crate
/// - **Without `alloc` feature**: Type alias for `&'static [T]`, requires static data
#[cfg_attr(docsrs, doc(cfg(all())))]
#[cfg(feature = "alloc")]
pub type Vec<T> = alloc::vec::Vec<T>;

#[cfg_attr(docsrs, doc(hidden))]
#[cfg(not(feature = "alloc"))]
pub type Vec<T> = &'static [T];
