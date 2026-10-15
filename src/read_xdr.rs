//! Reading XDR data into Rust types.
//!
//! This module provides the [`ReadXDR`] trait and implementations for
//! converting [XDR][RFC-4506]-encoded data (as defined in RFC 4506)
//! into Rust values.
//!
//! # Stack usage
//!
//! Decoding does not enforce a recursion depth limit. Types that are
//! recursive through a heap-allocated container (e.g.
//! `Vec<Node>` where `Node` contains another `Vec<Node>`) will
//! recurse once per nesting level in the input, and a sufficiently
//! deep input can exhaust the call stack.
//!
//! Similarly, decoding a large fixed-size array `[T; N]` or a
//! `heapless::Vec<T, N>` with a large `N` requires corresponding
//! stack space, because those types are stack-allocated by
//! definition.
//!
//! The recommended approach for untrusted input is to define types
//! with static nesting depth (e.g. a fixed chain of structs instead
//! of a recursive `Vec<Node>`) so that stack usage is bounded at
//! compile time. When recursive types are unavoidable, callers
//! should apply their own resource limits (e.g. capping input length
//! or running the decoder on a thread with a sized stack) rather
//! than relying on the decoder to bound stack usage.
//!
//! [RFC-4506]: https://datatracker.ietf.org/doc/html/rfc4506

use crate::mu;
use crate::serialize::reader::*;
use crate::valid_u32::*;
use core::convert::Infallible;
use core::mem::MaybeUninit as MU;

#[cfg(feature = "alloc")]
use xdr_common::MAX_PREALLOC_BYTES;

struct PartialInitGuard<'a, T> {
    slice: &'a mut [MU<T>],
    initialized: usize,
}

impl<T> Drop for PartialInitGuard<'_, T> {
    fn drop(&mut self) {
        // `take` rather than indexing `[..self.initialized]`: the bound holds
        // by construction, but it is not visible here, so the index would keep
        // a bounds check and a panicking branch in every array and VecM decode.
        for elt in self.slice.iter_mut().take(self.initialized) {
            // SAFETY: elements up to `self.initialized` were successfully
            // written via `MaybeUninit::write` before the count was incremented.
            unsafe {
                elt.assume_init_drop();
            }
        }
    }
}

/// Reads an XDR value of type `T` from the reader `r`.
///
/// This is a convenience function that calls [`ReadXDR::read_xdr`] on the type.
///
/// # Example
///
/// ```rust
/// use xdrs::cxs;
/// // Valid CXS-encoded u32 value of 0 (just one byte)
/// let data = [0u8];
/// let _value: u32 = cxs::decode(&data)?;
/// # Ok::<(), xdrs::ReadError>(())
/// ```
#[inline]
pub fn read_xdr<'r, R, T>(r: &mut R) -> ReadResult<T>
where
    R: Reader<'r>,
    T: ReadXDR<'r>,
{
    <T as ReadXDR>::read_xdr(r)
}

/// Types that can be constructed by reading XDR-encoded data, possibly
/// sharing data from the input.
///
/// The lifetime parameter `'r` represents the lifetime of data borrowed
/// from the reader.
///
/// Borrowed types implement this for every lifetime the reader outlives:
/// `&'a [u8]`, `&'a [u8; N]` and `&'a str` are `ReadXDR<'r>` whenever
/// `'r: 'a`, not only when `'a` is `'r` itself. So a decoded borrow can be
/// placed somewhere whose type is already fixed to a shorter lifetime, such
/// as a slot behind `&mut` or a generic parameter inferred from one, without
/// relying on the use site to shorten it.
///
/// # Derived impls and struct lifetimes
///
/// `#[derive(ReadXDR)]` on a type `Foo<'a, 'b, ..>` emits an impl of the
/// form `unsafe impl<'_r: 'a + 'b + .., 'a, 'b, ..> ReadXDR<'_r> for Foo<'a, 'b, ..>`.
/// That is: **every lifetime parameter on the type must be `<=` the lifetime
/// of the reader buffer**, regardless of whether that parameter is actually
/// used to borrow from the buffer. In particular, a phantom lifetime such as
/// `PhantomData<&'a ()>` is constrained alongside any "real" borrow.
///
/// Practical implication: you cannot decode `Foo<'static>` from a buffer
/// whose lifetime isn't `'static`. If a type has a lifetime parameter that
/// doesn't actually borrow from the buffer (e.g. it's purely a phantom
/// marker), let the lifetime be inferred from the buffer rather than
/// naming it explicitly, or implement `ReadXDR` by hand.
///
/// Custom borrowed field types need the same bounds as the built-in borrows:
/// implement `ReadXDR<'r>` for `Borrowed<'a>` whenever `'r: 'a`. An impl only
/// for `ReadXDR<'a> for Borrowed<'a>` can work by value through lifetime
/// coercion, but cannot fill a field whose type is fixed behind `&mut`.
///
/// ```rust
/// use xdrs::{ReadXDR, ReadResult};
/// use xdrs::serialize::reader::Reader;
///
/// struct Borrowed<'a>(&'a [u8]);
///
/// // SAFETY: the default in-place method writes the value returned here.
/// unsafe impl<'r: 'a, 'a> ReadXDR<'r> for Borrowed<'a> {
///     fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<Self> {
///         Ok(Self(r.read_opaque_variable(u32::MAX)?))
///     }
/// }
/// ```
///
/// # Safety
///
/// On success, [`ReadXDR::read_xdr_in_place`] must fully initialize its
/// destination with a valid `Self` and return a reference to that value in
/// that same destination. Callers rely on this guarantee when assuming the
/// storage is initialized and when dropping it, without a release-mode check.
/// The provided default implementation satisfies this contract; implementations
/// that only define `read_xdr` do not need any additional unsafe operations.
///
/// On an error or panic, callers will not drop the destination. An override
/// is responsible for cleaning up any fields it initialized along the way.
///
/// # Example
///
/// ```rust
/// use xdrs::{ReadXDR, cxs};
///
/// let data = [0u8]; // valid CXS encoding for the unsigned integer 0
/// let _value: u32 = cxs::decode(&data)?;
/// # Ok::<(), xdrs::ReadError>(())
/// ```
pub unsafe trait ReadXDR<'r>: Sized {
    /// Reads an XDR value from the reader.
    ///
    /// This method should read the appropriate amount of data from the reader
    /// and construct a value of type `Self`.
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<Self>;

    /// Reads an XDR value directly into `slot`, without first building it
    /// on the stack.
    ///
    /// On success, `slot` is fully initialized and the returned reference
    /// must point to it. On error, nothing in `slot` may be left that needs
    /// dropping: an implementation that writes part of the value before
    /// failing must drop what it wrote.
    ///
    /// The default implementation decodes with [`ReadXDR::read_xdr`] and
    /// moves the result into `slot`. Overriding it pays off for large
    /// values, such as structs with big fixed-size arrays, that would
    /// otherwise be built in a stack frame and then copied. Callers should
    /// go through [`read_xdr_in_place`], which checks the returned pointer
    /// when debug assertions are enabled.
    #[inline]
    fn read_xdr_in_place<'s, R: Reader<'r>>(
        r: &mut R,
        slot: &'s mut MU<Self>,
    ) -> ReadResult<&'s mut Self> {
        Ok(slot.write(Self::read_xdr(r)?))
    }
}

/// Reads an XDR value of type `T` from the reader `r` directly into `slot`.
///
/// On success, `slot` is initialized and the returned reference points to
/// it. On error, `slot` is left uninitialized with nothing to drop.
///
/// # Panics
///
/// With debug assertions enabled, panics if `T`'s
/// [`ReadXDR::read_xdr_in_place`] returns a reference that does not point
/// into `slot`. This can only happen with an incorrect manual
/// unsafe implementation. This diagnostic is debug-only; release builds
/// rely on the [`ReadXDR`] safety contract.
#[inline]
pub fn read_xdr_in_place<'r, 's, R, T>(r: &mut R, slot: &'s mut MU<T>) -> ReadResult<&'s mut T>
where
    R: Reader<'r>,
    T: ReadXDR<'r>,
{
    let expected: *const T = slot.as_ptr();
    let out = T::read_xdr_in_place(r, slot)?;
    debug_assert!(
        core::ptr::eq(&*out, expected),
        "ReadXDR::read_xdr_in_place returned a reference outside its slot"
    );
    Ok(out)
}

/// Support code for `#[derive(ReadXDR)]`. Not public API.
#[doc(hidden)]
pub mod in_place {
    use super::*;

    /// Drops one already-decoded field if a later field fails to decode.
    ///
    /// Generated code creates one guard per field as it goes and forgets
    /// them all once every field is written. Guards drop in reverse order on
    /// an early return, which unwinds exactly the fields written so far.
    pub struct FieldGuard<T>(*mut T);

    impl<T> Drop for FieldGuard<T> {
        #[inline]
        fn drop(&mut self) {
            // SAFETY: a guard is only created for a field that was fully
            // initialized, and only dropped (rather than forgotten) when the
            // enclosing value is abandoned, so the field is dropped once.
            unsafe { core::ptr::drop_in_place(self.0) }
        }
    }

    /// Decodes a field of type `F` into the field at `field`.
    ///
    /// # Safety
    ///
    /// `field` must be valid for writes, properly aligned, not aliased for
    /// the duration of the call, and must not hold a value that needs
    /// dropping. It must remain valid and initialized until the returned
    /// guard is dropped or forgotten; the guard owns the right to drop it.
    #[cfg_attr(not(target_arch = "riscv32"), inline(always))]
    pub unsafe fn read_field<'r, R, F>(r: &mut R, field: *mut F) -> ReadResult<FieldGuard<F>>
    where
        R: Reader<'r>,
        F: ReadXDR<'r>,
    {
        // SAFETY: `MaybeUninit<F>` has the same layout as `F`, and the
        // caller guarantees `field` is valid, aligned and unaliased.
        let slot = unsafe { &mut *field.cast::<MU<F>>() };
        read_xdr_in_place(r, slot)?;
        Ok(FieldGuard(field))
    }

    /// Moves an already-built value into the field at `field`.
    ///
    /// Used for fields decoded through `#[xdr(from = ..)]` or `via`, which
    /// must be converted before they can be stored.
    ///
    /// # Safety
    ///
    /// Same as [`read_field`].
    #[cfg_attr(not(target_arch = "riscv32"), inline(always))]
    pub unsafe fn write_field<F>(field: *mut F, value: F) -> FieldGuard<F> {
        // SAFETY: forwarded caller contract.
        unsafe { field.write(value) };
        FieldGuard(field)
    }
}

/// Types that can be constructed by reading XDR data
/// of any lifetime.
///
/// This trait is automatically implemented for any type that implements
/// [`ReadXDR`] for all lifetimes. It's useful for types that don't borrow
/// data from the reader.
pub trait ReadXDROwned: for<'r> ReadXDR<'r> {}

impl<T> ReadXDROwned for T where T: for<'r> ReadXDR<'r> {}

// SAFETY: unless explicitly overriding read_xdr_in_place below, these impls
// use the default, which writes the valid value returned by read_xdr.
macro_rules! construct_int_impls {
    { $( $read_method:ident for $int_ty:ty ),* $(,)? } => { $(

        unsafe impl<'r> ReadXDR<'r> for $int_ty {
            #[inline]
            fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<$int_ty> {
                r.$read_method()
            }
        }

    )* };
}

construct_int_impls! {
    read_i64 for i64,
    read_i32 for i32,
    read_i16 for i16,

    read_u64 for u64,
    read_u32 for u32,
    read_u16 for u16,

    read_bool for bool,
    read_f32 for f32,
    read_f64 for f64,
}

// SAFETY: success initializes every element; the guard cleans up partial reads.
unsafe impl<'r, T, const N: usize> ReadXDR<'r> for [T; N]
where
    T: ReadXDR<'r>,
{
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<Self> {
        let () = ValidU32::<N>::OK;

        let mut arr: [MU<T>; N] = [const { MU::uninit() }; N];
        let mut guard = PartialInitGuard {
            slice: &mut arr,
            initialized: 0,
        };

        for elt in guard.slice.iter_mut() {
            elt.write(read_xdr(r)?);
            guard.initialized += 1;
        }

        core::mem::forget(guard);

        // SAFETY: all elements have been initialized
        let arr = unsafe { mu::array_assume_init(arr) };

        Ok(arr)
    }

    fn read_xdr_in_place<'s, R: Reader<'r>>(
        r: &mut R,
        slot: &'s mut MU<Self>,
    ) -> ReadResult<&'s mut Self> {
        let () = ValidU32::<N>::OK;

        // SAFETY: `MaybeUninit<[T; N]>` has the same layout as
        // `[MaybeUninit<T>; N]`, and `slot` is borrowed for the whole call.
        let elems = unsafe { &mut *slot.as_mut_ptr().cast::<[MU<T>; N]>() };
        let mut guard = PartialInitGuard {
            slice: elems,
            initialized: 0,
        };

        for elt in guard.slice.iter_mut() {
            read_xdr_in_place(r, elt)?;
            guard.initialized += 1;
        }

        core::mem::forget(guard);

        // SAFETY: all elements have been initialized
        Ok(unsafe { slot.assume_init_mut() })
    }
}

// Borrowed types are decodable for every lifetime the reader buffer
// outlives, not only the reader's own: the borrow comes out of the reader as
// `&'r _` and is shortened to `'a` here. This matters wherever the decoded
// type is fixed by an invariant position (a generic parameter, a slot
// behind `&mut`), where the shortening cannot happen at the use site.
// SAFETY: the default in-place method writes the value returned by read_xdr.
unsafe impl<'r: 'a, 'a> ReadXDR<'r> for &'a [u8] {
    #[inline]
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<Self> {
        let bytes: &'r [u8] = r.read_opaque_variable(u32::MAX)?;
        Ok(bytes)
    }
}

// SAFETY: the default in-place method writes the value returned by read_xdr.
unsafe impl<'r: 'a, 'a, const N: usize> ReadXDR<'r> for &'a [u8; N] {
    #[inline]
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<Self> {
        let () = ValidU32::<N>::OK;
        let len = N as u32;

        // SAFETY: read_opaque_fixed returns a slice of exactly `len` bytes,
        // and len == N as u32, so the slice length matches the array size.
        let bytes: &'r [u8; N] = unsafe {
            r.read_opaque_fixed(len as usize)?
                .try_into()
                .unwrap_unchecked()
        };
        Ok(bytes)
    }
}

// SAFETY: Reader initializes exactly N bytes on a successful fixed-byte read.
unsafe impl<'r, const N: usize> ReadXDR<'r> for [u8; N] {
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<Self> {
        let () = ValidU32::<N>::OK;

        let mut arr: [MU<u8>; N] = [MU::uninit(); N];

        // Panic-safe: arr has exactly N elements and len is N as u32.
        r.read_opaque_fixed_into(&mut arr, N as u32)?;

        // SAFETY: read_opaque_fixed_into initialized all N elements.
        let arr = unsafe { mu::array_assume_init(arr) };

        Ok(arr)
    }

    fn read_xdr_in_place<'s, R: Reader<'r>>(
        r: &mut R,
        slot: &'s mut MU<Self>,
    ) -> ReadResult<&'s mut Self> {
        let () = ValidU32::<N>::OK;

        // SAFETY: `MaybeUninit<[u8; N]>` has the same layout as
        // `[MaybeUninit<u8>; N]`, and `slot` is borrowed for the whole call.
        let bytes = unsafe { &mut *slot.as_mut_ptr().cast::<[MU<u8>; N]>() };

        // Panic-safe: bytes has exactly N elements and len is N as u32.
        r.read_opaque_fixed_into(bytes, N as u32)?;

        // SAFETY: read_opaque_fixed_into initialized all N elements.
        Ok(unsafe { slot.assume_init_mut() })
    }
}

// SAFETY: the default in-place method writes the value returned by read_xdr.
unsafe impl<'r: 'a, 'a> ReadXDR<'r> for &'a str {
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<Self> {
        let bytes: &'r [u8] = r.read_opaque_variable(u32::MAX)?;
        let s: &'r str = str::from_utf8(bytes).map_err(|_| ReadError::bad_utf8())?;
        Ok(s)
    }
}

#[cfg(feature = "alloc")]
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
// SAFETY: the default in-place method writes the value returned by read_xdr.
unsafe impl<'r> ReadXDR<'r> for alloc::string::String {
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<Self> {
        let bytes = r.read_opaque_variable(u32::MAX)?;
        let s = str::from_utf8(bytes).map_err(|_| ReadError::bad_utf8())?;
        Ok(alloc::string::String::from(s))
    }
}

// SAFETY: the default in-place method writes the value returned by read_xdr.
unsafe impl<'r> ReadXDR<'r> for () {
    fn read_xdr<R: Reader<'r>>(_r: &mut R) -> ReadResult<Self> {
        Ok(())
    }
}

// SAFETY: the default in-place method writes the value returned by read_xdr.
unsafe impl<'r> ReadXDR<'r> for Infallible {
    fn read_xdr<R: Reader<'r>>(_r: &mut R) -> ReadResult<Self> {
        unreachable!()
    }
}

macro_rules! read_tuple {
    { $( $index:tt : $t:ident ),+ $(,)? } => {
        // SAFETY: the in-place method initializes every field before returning
        // success, and guards drop completed fields on error or panic.
        unsafe impl<'r, $($t),+> ReadXDR<'r> for ($($t,)+)
        where
            $( $t: ReadXDR<'r>, )+
        {
            fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<Self> {
                Ok(($(read_xdr::<R, $t>(r)?,)+))
            }

            #[cfg_attr(not(target_arch = "riscv32"), inline(always))]
            fn read_xdr_in_place<'s, R: Reader<'r>>(
                r: &mut R,
                slot: &'s mut MU<Self>,
            ) -> ReadResult<&'s mut Self> {
                let base = slot.as_mut_ptr();
                // SAFETY: tuple fields are aligned, disjoint, and in the
                // caller's exclusively borrowed slot. If a read fails, Rust
                // drops the guards already constructed in this tuple.
                let guards = ($(unsafe {
                    in_place::read_field(r, &raw mut (*base).$index)?
                },)+);
                core::mem::forget(guards);
                // SAFETY: every tuple field was successfully initialized.
                Ok(unsafe { slot.assume_init_mut() })
            }
        }
    };
}

read_tuple!(0: T1);
read_tuple!(0: T1, 1: T2);
read_tuple!(0: T1, 1: T2, 2: T3);
read_tuple!(0: T1, 1: T2, 2: T3, 3: T4);
read_tuple!(0: T1, 1: T2, 2: T3, 3: T4, 4: T5);
read_tuple!(0: T1, 1: T2, 2: T3, 3: T4, 4: T5, 5: T6);
read_tuple!(0: T1, 1: T2, 2: T3, 3: T4, 4: T5, 5: T6, 6: T7);
read_tuple!(0: T1, 1: T2, 2: T3, 3: T4, 4: T5, 5: T6, 6: T7, 7: T8);
read_tuple!(0: T1, 1: T2, 2: T3, 3: T4, 4: T5, 5: T6, 6: T7, 7: T8, 8: T9);
read_tuple!(0: T1, 1: T2, 2: T3, 3: T4, 4: T5, 5: T6, 6: T7, 7: T8, 8: T9, 9: T10);
read_tuple!(0: T1, 1: T2, 2: T3, 3: T4, 4: T5, 5: T6, 6: T7, 7: T8, 8: T9, 9: T10, 10: T11);
read_tuple!(0: T1, 1: T2, 2: T3, 3: T4, 4: T5, 5: T6, 6: T7, 7: T8, 8: T9, 9: T10, 10: T11, 11: T12);
read_tuple!(0: T1, 1: T2, 2: T3, 3: T4, 4: T5, 5: T6, 6: T7, 7: T8, 8: T9, 9: T10, 10: T11, 11: T12, 12: T13);
read_tuple!(0: T1, 1: T2, 2: T3, 3: T4, 4: T5, 5: T6, 6: T7, 7: T8, 8: T9, 9: T10, 10: T11, 11: T12, 12: T13, 13: T14);
read_tuple!(0: T1, 1: T2, 2: T3, 3: T4, 4: T5, 5: T6, 6: T7, 7: T8, 8: T9, 9: T10, 10: T11, 11: T12, 12: T13, 13: T14, 14: T15);
read_tuple!(0: T1, 1: T2, 2: T3, 3: T4, 4: T5, 5: T6, 6: T7, 7: T8, 8: T9, 9: T10, 10: T11, 11: T12, 12: T13, 13: T14, 14: T15, 15: T16);

// SAFETY: both branches write a valid Option into the destination on success.
unsafe impl<'r, T> ReadXDR<'r> for Option<T>
where
    T: ReadXDR<'r>,
{
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<Self> {
        let is_present = r.read_bool()?;
        if is_present {
            Ok(Some(read_xdr(r)?))
        } else {
            Ok(None)
        }
    }

    #[cfg_attr(not(target_arch = "riscv32"), inline(always))]
    fn read_xdr_in_place<'s, R: Reader<'r>>(
        r: &mut R,
        slot: &'s mut MU<Self>,
    ) -> ReadResult<&'s mut Self> {
        if !r.read_bool()? {
            return Ok(slot.write(None));
        }

        // Option's payload layout is not specified for arbitrary T. Decode
        // into one payload temporary, then construct Some normally. This
        // avoids the additional Result<Option<T>> temporary used by the
        // default method, and lets None initialize only its discriminant.
        let mut value = MU::<T>::uninit();
        read_xdr_in_place(r, &mut value)?;
        // SAFETY: read_xdr_in_place initialized the payload on success.
        Ok(slot.write(Some(unsafe { value.assume_init() })))
    }
}

#[cfg(feature = "alloc")]
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
// SAFETY: the default in-place method writes the value returned by read_xdr.
unsafe impl<'r, T> ReadXDR<'r> for xdr_common::Box<T>
where
    T: ReadXDR<'r>,
{
    #[cfg_attr(not(target_arch = "riscv32"), inline(always))]
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<Self> {
        // For at most two machine words, moving the value is cheap. Decode
        // before allocating so scalar reads need not depend on the heap
        // destination, and a failed read needs no allocation or deallocation.
        // This size check is constant for each T and disappears at compile time.
        if core::mem::size_of::<T>() <= 2 * core::mem::size_of::<usize>() {
            let mut value = MU::<T>::uninit();
            read_xdr_in_place(r, &mut value)?;
            // SAFETY: read_xdr_in_place initialized the value on success.
            return Ok(Self::new(unsafe { value.assume_init() }));
        }

        let mut boxed = Self::new_uninit();
        read_xdr_in_place(r, &mut *boxed)?;
        // SAFETY: read_xdr_in_place returned Ok, so the value is initialized.
        Ok(unsafe { boxed.assume_init() })
    }
}

#[cfg(not(feature = "alloc"))]
// SAFETY: the default in-place method writes the value returned by read_xdr.
unsafe impl<'r, T> ReadXDR<'r> for xdr_common::Box<T>
where
    T: ReadXDR<'r>,
{
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<Self> {
        Ok(Self::new(read_xdr(r)?))
    }
}

// SAFETY: the default in-place method writes the value returned by read_xdr.
unsafe impl<'r, T, E> ReadXDR<'r> for core::result::Result<T, E>
where
    T: ReadXDR<'r>,
    E: ReadXDR<'r>,
{
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<Self> {
        let d = r.read_i32()?;

        match d {
            0 => Ok(Result::Ok(read_xdr(r)?)),
            1 => Ok(Result::Err(read_xdr(r)?)),
            _ => Err(ReadError::bad_discriminator(d)),
        }
    }
}

// SAFETY: the default in-place method writes the value returned by read_xdr.
unsafe impl<'r, T> ReadXDR<'r> for core::marker::PhantomData<T> {
    fn read_xdr<R: Reader<'r>>(_r: &mut R) -> ReadResult<Self> {
        Ok(core::marker::PhantomData)
    }
}

/// Decodes `count` elements directly into the spare capacity at the end of
/// `vec`, which must already have room for them.
///
/// On error, the elements decoded so far in this call are dropped and `vec`
/// keeps its previous length, so whatever it already held is dropped with
/// it as usual.
#[cfg(feature = "alloc")]
#[inline]
fn fill_spare<'r, R, T>(r: &mut R, vec: &mut alloc::vec::Vec<T>, count: usize) -> ReadResult<()>
where
    R: Reader<'r>,
    T: ReadXDR<'r>,
{
    let slice = &mut vec.spare_capacity_mut()[..count];
    let mut guard = PartialInitGuard {
        slice,
        initialized: 0,
    };

    for elt in guard.slice.iter_mut() {
        read_xdr_in_place(r, elt)?;
        guard.initialized += 1;
    }

    core::mem::forget(guard);

    // SAFETY: all `count` elements of the spare capacity were initialized.
    unsafe { vec.set_len(vec.len() + count) };
    Ok(())
}

/// Decodes `len` elements into a new vector.
///
/// Capacity for up to [`MAX_PREALLOC_BYTES`] is reserved up front, since
/// `len` comes from the wire and must not drive an unbounded allocation,
/// and those elements are decoded straight into it. After that, grow the
/// vector as needed and decode each batch into its spare capacity.
#[cfg(feature = "alloc")]
// Keep the reader local to the caller so its position updates and overflow
// checks can be optimized across the element-decoding loop. The Vec/VecM
// entry points must also inline for the same reason.
#[cfg_attr(not(target_arch = "riscv32"), inline(always))]
fn read_vec<'r, R, T>(r: &mut R, len: usize) -> ReadResult<alloc::vec::Vec<T>>
where
    R: Reader<'r>,
    T: ReadXDR<'r>,
{
    let cap = len.min(MAX_PREALLOC_BYTES / core::mem::size_of::<T>().max(1));
    let mut vec = alloc::vec::Vec::with_capacity(cap);
    fill_spare(r, &mut vec, cap)?;

    while vec.len() < len {
        vec.reserve(1);
        let count = (len - vec.len()).min(vec.capacity() - vec.len());
        fill_spare(r, &mut vec, count)?;
    }

    Ok(vec)
}

#[cfg(feature = "alloc")]
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
const _: () = {
    use alloc::vec::Vec;

    // SAFETY: the default in-place method writes the value returned by read_xdr.
    unsafe impl<'r> ReadXDR<'r> for Vec<u8> {
        fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<Self> {
            let bytes: &[u8] = r.read_opaque_variable(u32::MAX)?;
            let vec = Vec::from(bytes); // copy the slice
            Ok(vec)
        }
    }

    // SAFETY: the default in-place method writes the value returned by read_xdr.
    unsafe impl<'r, T> ReadXDR<'r> for Vec<T>
    where
        T: ReadXDR<'r>,
    {
        #[cfg_attr(not(target_arch = "riscv32"), inline(always))]
        fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<Self> {
            let len = r.read_u32()? as usize;
            read_vec(r, len)
        }
    }
};

#[cfg(feature = "alloc")]
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
const _: () = {
    use xdr_common::*;

    use crate::ReadXDR;
    use crate::serialize::reader::{ReadError, Reader};

    use alloc::vec::Vec; // can use noalloc machinery in common_types to avoid this

    // SAFETY: the default in-place method writes the value returned by read_xdr.
    unsafe impl<'r, const MAX: u32> ReadXDR<'r> for BytesM<MAX> {
        fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<Self> {
            let bytes = r.read_opaque_variable(MAX)?;
            let vec = Vec::from(bytes);
            Ok(Self(vec))
        }
    }

    // SAFETY: the default in-place method writes the value returned by read_xdr.
    unsafe impl<'r, const MAX: u32> ReadXDR<'r> for StringM<MAX> {
        fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<Self> {
            let bytes = r.read_opaque_variable(MAX)?;
            let vec = Vec::from(bytes);
            Ok(Self(vec))
        }
    }

    // SAFETY: the default in-place method writes the value returned by read_xdr.
    unsafe impl<'r, const MAX: u32, T> ReadXDR<'r> for VecM<T, MAX>
    where
        T: ReadXDR<'r>,
    {
        #[cfg_attr(not(target_arch = "riscv32"), inline(always))]
        fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<Self> {
            let len = r.read_u32()? as usize;
            if len > MAX as usize {
                return Err(ReadError::max_length_exceeded());
            }

            Ok(Self(read_vec(r, len)?))
        }
    }

    // make VecM<u8> work like BytesM;
    // SAFETY: the default in-place method writes the value returned by read_xdr.
    unsafe impl<'r, const MAX: u32> ReadXDR<'r> for VecM<u8, MAX> {
        fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<Self> {
            let bytes = r.read_opaque_variable(MAX)?;
            let vec = Vec::from(bytes);
            Ok(Self(vec))
        }
    }
};

#[cfg(not(feature = "alloc"))]
const _: () = {
    use crate::ReadXDR;
    use crate::serialize::reader::Reader;
    use xdr_common::*;

    // SAFETY: the default in-place method writes the value returned by read_xdr.
    unsafe impl<'r, const MAX: u32> ReadXDR<'r> for BytesM<MAX> {
        fn read_xdr<R: Reader<'r>>(_r: &mut R) -> ReadResult<Self> {
            unimplemented!()
        }
    }

    // SAFETY: the default in-place method writes the value returned by read_xdr.
    unsafe impl<'r, const MAX: u32> ReadXDR<'r> for StringM<MAX> {
        fn read_xdr<R: Reader<'r>>(_r: &mut R) -> ReadResult<Self> {
            unimplemented!()
        }
    }

    // SAFETY: the default in-place method writes the value returned by read_xdr.
    unsafe impl<'r, const MAX: u32, T> ReadXDR<'r> for VecM<T, MAX>
    where
        T: ReadXDR<'r> + 'static,
    {
        fn read_xdr<R: Reader<'r>>(_r: &mut R) -> ReadResult<Self> {
            unimplemented!()
        }
    }

    // make VecM<u8> work like BytesM;
    // SAFETY: the default in-place method writes the value returned by read_xdr.
    unsafe impl<'r, const MAX: u32> ReadXDR<'r> for VecM<u8, MAX> {
        fn read_xdr<R: Reader<'r>>(_r: &mut R) -> ReadResult<Self> {
            unimplemented!()
        }
    }
};
