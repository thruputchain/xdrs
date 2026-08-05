//! Utilities for working with `MaybeUninit` types.
//!
//! This module provides helper functions for safely working with uninitialized memory,
//! used internally by the serialization code.

use core::mem::MaybeUninit as MU;

/// Gets a shared reference to the contained value.
///
/// # Safety
///
/// Calling this when the content is not yet fully initialized causes undefined
/// behavior: it is up to the caller to guarantee that every `MaybeUninit<T>` in
/// the slice really is in an initialized state.
///
#[inline(always)]
pub const unsafe fn assume_init_ref<T>(slice: &[MU<T>]) -> &[T] {
    unsafe {
        // const since 1.59.0
        slice.assume_init_ref()
    }
}

/// Gets a mutable (unique) reference to the contained value.
///
/// # Safety
///
/// Calling this when the content is not yet fully initialized causes undefined
/// behavior: it is up to the caller to guarantee that every `MaybeUninit<T>` in the
/// slice really is in an initialized state. For instance, `.assume_init_mut()` cannot
/// be used to initialize a `MaybeUninit` slice.
///
#[inline(always)]
pub const unsafe fn assume_init_mut<T>(slice: &mut [MU<T>]) -> &mut [T] {
    unsafe {
        // const since 1.84.0
        slice.assume_init_mut()
    }
}

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

/// Copies the elements from `src` to `uninit_dst`,
/// returning a mutable reference to the now initialized contents.
///
/// Panics if the two slices don't have the same length.
///
/// # Safety
///
/// Not actually unsafe anymore.
///
#[inline(always)]
pub const unsafe fn write_copy_of_slice<'a, T>(this: &'a mut [MU<T>], src: &[T]) -> &'a mut [T]
where
    T: Copy,
{
    // const since 0.1.93
    this.write_copy_of_slice(src)
}
