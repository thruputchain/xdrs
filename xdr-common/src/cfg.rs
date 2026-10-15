// Copied from https://github.com/stellar/xdrgen/blob/master/lib/xdrgen/generators/rust/src/types.rs

#![allow(unused_imports)]
#![allow(dead_code)]

#[cfg(all(feature = "serde", feature = "alloc"))]
pub(crate) use serde::de::{
    Deserialize, Deserializer, Error as DeError, IgnoredAny, SeqAccess, Visitor,
};
#[cfg(all(feature = "serde", feature = "alloc"))]
pub(crate) use serde::ser::{Error as SerError, Serialize, Serializer};

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
/// - **Without `alloc` feature**: Type alias for `T`, stored inline
#[cfg_attr(docsrs, doc(cfg(all())))]
#[cfg(feature = "alloc")]
pub type Box<T> = alloc::boxed::Box<T>;

/// No-`alloc` stand-in for `Box<T>`: an alias for `T`, stored inline.
///
/// Without an allocator nothing can be heap-boxed, and a `&'static T` cannot
/// be decoded into from a non-`'static` buffer, so a boxed field would make
/// the derived `ReadXDR<'r>` of its containing type fail to compile. Holding
/// the value inline keeps encoding, decoding and size bounds working.
/// Recursive types still need `alloc`: through an inline `Box` they have
/// infinite size.
#[cfg_attr(docsrs, doc(hidden))]
#[cfg(not(feature = "alloc"))]
pub type Box<T> = T;

/// A contiguous growable array type.
///
/// # Feature Variants
///
/// - **With `alloc` feature**: Standard heap-allocated `Vec<T>` from the alloc crate
/// - **Without `alloc` feature**: Type alias for `&'static [T]`, requires static data
#[cfg_attr(docsrs, doc(cfg(all())))]
#[cfg(feature = "alloc")]
pub type Vec<T> = alloc::vec::Vec<T>;

/// No-`alloc` stand-in for `Vec<T>`: an alias for `&'static [T]`,
/// requiring static data.
#[cfg_attr(docsrs, doc(hidden))]
#[cfg(not(feature = "alloc"))]
pub type Vec<T> = &'static [T];
