//! Utilities for working with `MaybeUninit` types.
//!
//! This module provides helper functions for safely working with uninitialized memory,
//! used internally by the serialization code.

use core::mem::MaybeUninit as MU;

/// Extracts the values from an array of `MaybeUninit` containers.
///
/// # Safety
///
/// It is up to the caller to guarantee that all elements of the array are
/// in an initialized state.
///
/// (This implementation fills in for `core::mem::array_assume_init` in
/// the unstable feature `maybe_uninit_array_assume_init`.)
#[inline(always)]
pub const unsafe fn array_assume_init<T, const N: usize>(array: [MU<T>; N]) -> [T; N] {
    // SAFETY:
    // * The caller guarantees that all elements of the array are initialized
    // * `MaybeUninit<T>` and T are guaranteed to have the same layout
    // * `MaybeUninit` does not drop, so there are no double-frees
    // And thus the conversion is safe

    // // This doesn't work because the generics engine can't see that
    // //      sizeof<T> * N  ==  sizeof<MU<T>> * N
    //
    // unsafe {
    //     core::mem::transmute(array)
    // }

    // // The internet alleges that this is a runtime no-op, but it is not const
    //
    // array.map(|x| unsafe { x.assume_init() })

    // // This was the standard-library implementation of `array_assume_init`
    // // before they switched to using transmute_unchecked
    // unsafe {
    //     core::ptr::read(&array as *const _ as *const [T; N])
    // }

    // And this is approximately the same thing:
    unsafe { core::mem::transmute_copy(&array) }
}
