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
/// - **Without `alloc` feature**: Transparent newtype holding `T` inline
#[cfg_attr(docsrs, doc(cfg(all())))]
#[cfg(feature = "alloc")]
pub type Box<T> = alloc::boxed::Box<T>;

/// No-`alloc` stand-in for `Box<T>`: a transparent newtype holding `T`
/// inline.
///
/// Without an allocator nothing can be heap-boxed, and a `&'static T` cannot
/// be decoded into from a non-`'static` buffer, so a boxed field would make
/// the derived `ReadXDR<'r>` of its containing type fail to compile. Holding
/// the value inline keeps encoding, decoding and size bounds working, and
/// keeping `Box<T>` a distinct type from `T` keeps `Box::new`, `Deref`,
/// `AsRef<T>` and impl coherence identical to the `alloc` `Box`.
/// Recursive types still need `alloc`: through an inline `Box` they have
/// infinite size.
#[cfg_attr(docsrs, doc(hidden))]
#[cfg(not(feature = "alloc"))]
#[derive(Clone, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct Box<T>(T);

#[cfg(not(feature = "alloc"))]
impl<T> Box<T> {
    /// Wraps `x` (inline; nothing is allocated).
    #[inline]
    pub const fn new(x: T) -> Self {
        Self(x)
    }
    /// Unwraps the value.
    #[inline]
    pub fn into_inner(b: Self) -> T {
        b.0
    }
}

#[cfg(not(feature = "alloc"))]
impl<T> core::ops::Deref for Box<T> {
    type Target = T;
    #[inline]
    fn deref(&self) -> &T {
        &self.0
    }
}
#[cfg(not(feature = "alloc"))]
impl<T> core::ops::DerefMut for Box<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}
#[cfg(not(feature = "alloc"))]
impl<T> AsRef<T> for Box<T> {
    #[inline]
    fn as_ref(&self) -> &T {
        &self.0
    }
}
#[cfg(not(feature = "alloc"))]
impl<T> AsMut<T> for Box<T> {
    #[inline]
    fn as_mut(&mut self) -> &mut T {
        &mut self.0
    }
}
#[cfg(not(feature = "alloc"))]
impl<T> core::borrow::Borrow<T> for Box<T> {
    #[inline]
    fn borrow(&self) -> &T {
        &self.0
    }
}
#[cfg(not(feature = "alloc"))]
impl<T> From<T> for Box<T> {
    #[inline]
    fn from(x: T) -> Self {
        Self(x)
    }
}
#[cfg(not(feature = "alloc"))]
impl<T: core::fmt::Debug> core::fmt::Debug for Box<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(f)
    }
}
#[cfg(not(feature = "alloc"))]
impl<T: core::fmt::Display> core::fmt::Display for Box<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(f)
    }
}

/// Borrows the value behind a [`Box`] in a `const` context.
///
/// `const fn` code cannot go through `Deref`, so the `ConstWriteXDR` derive
/// uses this to reach a boxed field. With `alloc` it is the heap `Box`'s
/// (const) deref; without `alloc` it borrows the inline value.
#[cfg(feature = "alloc")]
#[inline]
pub const fn box_as_ref<T>(b: &Box<T>) -> &T {
    b
}

/// Borrows the value behind a [`Box`] in a `const` context.
///
/// `const fn` code cannot go through `Deref`, so the `ConstWriteXDR` derive
/// uses this to reach a boxed field. With `alloc` it is the heap `Box`'s
/// (const) deref; without `alloc` it borrows the inline value.
#[cfg(not(feature = "alloc"))]
#[inline]
pub const fn box_as_ref<T>(b: &Box<T>) -> &T {
    &b.0
}

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
