//! Compile-time validation that a const generic usize fits in a u32.
//!
//! This module provides a type-level mechanism to ensure that const generic
//! parameters of type `usize` fit within the range of `u32`, which is required
//! for XDR serialization.

/// A type-level validator for ensuring a const generic `usize` fits in a `u32`.
///
/// This struct is used as a compile-time assertion. Accessing [`ValidU32::OK`]
/// will cause a compile error if `N` is greater than `u32::MAX as usize`.
pub struct ValidU32<const N: usize>;

impl<const N: usize> ValidU32<N> {
    /// A compile-time assertion that `N` fits in a `u32`.
    ///
    /// Accessing this constant will cause a compile error if `N > u32::MAX as usize`.
    /// This is used throughout the codebase to ensure array sizes and lengths
    /// are valid for XDR serialization.
    pub const OK: () = assert!(N <= u32::MAX as usize, "N must fit in a u32",);
}
