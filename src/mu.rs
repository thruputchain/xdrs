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
/// (This implementation fills in for `<[MaybeUninit<T>]>::assume_init_ref` in
/// the unstable feature `maybe_uninit_slice`.)
//
// (Adapted from standard library.)
#[inline(always)]
pub const unsafe fn assume_init_ref<T>(slice: &[MU<T>]) -> &[T] {
    // SAFETY: casting `slice` to a `*const [T]` is safe since the caller guarantees that
    // `slice` is initialized, and `MaybeUninit` is guaranteed to have the same layout as `T`.
    // The pointer obtained is valid since it refers to memory owned by `slice` which is a
    // reference and thus guaranteed to be valid for reads.
    unsafe { &*(slice as *const [MU<T>] as *const [T]) }
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
/// (This implementation fills in for `<[MaybeUninit<T>]>::assume_init_mut` in
/// the unstable feature `maybe_uninit_slice`.)
//
// (Adapted from standard library.)
#[inline(always)]
pub const unsafe fn assume_init_mut<T>(slice: &mut [MU<T>]) -> &mut [T] {
    // SAFETY: similar to safety notes for `slice_get_ref`, but we have a
    // mutable reference which is also guaranteed to be valid for writes.
    unsafe { &mut *(slice as *mut [MU<T>] as *mut [T]) }
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
/// # Safety
///
/// The two slices must have the same length. This is checked via
/// `debug_assert` in debug builds; in release builds, passing
/// mismatched lengths is undefined behavior.
///
/// (This implementation fills in for `<[MaybeUninit<T>]>::write_copy_of_slice` in
/// the unstable feature `maybe_uninit_write_slice`.)
//
// (Adapted from standard library.)
#[inline(always)]
pub const unsafe fn write_copy_of_slice<'a, T>(
    uninit_dst: &'a mut [MU<T>],
    src: &[T],
) -> &'a mut [T]
where
    T: Copy,
{
    debug_assert!(uninit_dst.len() == src.len());

    // SAFETY: &[T] and &[MaybeUninit<T>] have the same layout
    let uninit_src: &[MU<T>] = unsafe { core::mem::transmute(src) };

    // SAFETY: uninit_dst and uninit_src have the same length (asserted above),
    // and both are valid for their respective lengths. Using copy_from_slice
    // would be equivalent but generates a panic path that the compiler can't
    // eliminate even though the lengths are always equal.
    unsafe {
        core::ptr::copy_nonoverlapping(uninit_src.as_ptr(), uninit_dst.as_mut_ptr(), src.len());
    }

    // SAFETY: Valid elements have just been copied into `self` so it is initialized
    unsafe { assume_init_mut(uninit_dst) }
}
