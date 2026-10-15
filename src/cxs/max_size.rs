//! Worst-case CXS size bounds.
//!
//! [`ConstMaxSize`] reports, per type, the worst-case CXS encoded size over
//! all values of the type as an associated const — usable in const contexts
//! (e.g. sizing a static buffer) without a value in hand.
//!
//! Bounds fold through by-value layout only; boundedness does not cross
//! pointers. The blanket `&T` impl is a literal `None` that never names the
//! pointee's bound, and the `Box<T>` and element-generic `VecM<T, MAX>`
//! impls are likewise literal `None` (heap-backed indirection). This is what
//! makes const-evaluation cycles impossible for recursive types: recursion
//! must pass through indirection, and every indirection bounds to `None`
//! without evaluating anything further.
//!
//! `u8` deliberately does not implement [`ConstMaxSize`]: a lone `u8` is not
//! XDR-encodable, and its absence is what keeps the `[u8; N]`/`[T; N]` and
//! `VecM<u8, MAX>`/`VecM<T, MAX>` impl pairs disjoint.

use super::const_writer::{
    max_size_add, max_size_array, max_size_bool, max_size_f32, max_size_f64, max_size_i16,
    max_size_i32, max_size_i64, max_size_max, max_size_opaque_fixed, max_size_opaque_variable,
    max_size_u16, max_size_u32, max_size_u64, size_i32,
};
#[cfg(feature = "int128")]
use super::const_writer::{max_size_i128, max_size_u128};
use core::convert::Infallible;
use core::marker::PhantomData;
use xdr_common::{BytesM, StaticIfNoAlloc, StringM, VecM};

/// Worst-case CXS encoded size over all values of a type.
///
/// `MAX_SIZE` is `Some(bound)` when a finite bound exists, or `None` when
/// the type is unbounded, reached through indirection (`&T`, `Box<T>`,
/// `VecM<T, MAX>`), or its bound overflows `usize` on this target.
///
/// Implemented for user types via `#[derive(ConstXDRSize)]`; implement
/// manually for foreign types.
pub trait ConstMaxSize {
    /// Worst-case CXS encoded size in bytes, or `None` if unbounded.
    const MAX_SIZE: Option<usize>;
}

macro_rules! impl_primitive {
    ($($ty:ty => $max_fn:ident),* $(,)?) => {$(
        impl ConstMaxSize for $ty {
            const MAX_SIZE: Option<usize> = Some($max_fn());
        }
    )*};
}

impl_primitive! {
    i16 => max_size_i16,
    u16 => max_size_u16,
    i32 => max_size_i32,
    u32 => max_size_u32,
    i64 => max_size_i64,
    u64 => max_size_u64,
    bool => max_size_bool,
    f32 => max_size_f32,
    f64 => max_size_f64,
}

#[cfg(feature = "int128")]
impl_primitive! {
    i128 => max_size_i128,
    u128 => max_size_u128,
}

impl<const N: usize> ConstMaxSize for [u8; N] {
    const MAX_SIZE: Option<usize> = max_size_opaque_fixed(N);
}

impl<T: ConstMaxSize, const N: usize> ConstMaxSize for [T; N] {
    const MAX_SIZE: Option<usize> = max_size_array(N, T::MAX_SIZE);
}

// Pointer cut: boundedness does not cross a reference. Literal `None`, no
// bound on `T`, so the pointee's bound is never named (or evaluated).
impl<T: ?Sized> ConstMaxSize for &T {
    const MAX_SIZE: Option<usize> = None;
}

// Pointer cut: an owned heap `Box` is indirection — a cycle edge for
// recursive types, even though it XDR-encodes transparently as its pointee.
// Literal `None`, no bound on `T`, so the pointee's bound is never named (or
// evaluated). In no-`alloc` builds `Box<T>` aliases `T` itself and takes
// `T`'s bound; this covers the `alloc` heap `Box`.
#[cfg(feature = "alloc")]
impl<T: ?Sized> ConstMaxSize for alloc::boxed::Box<T> {
    const MAX_SIZE: Option<usize> = None;
}

impl<T: ConstMaxSize> ConstMaxSize for Option<T> {
    const MAX_SIZE: Option<usize> = max_size_add(Some(size_i32(1)), T::MAX_SIZE);
}

impl<T: ConstMaxSize, E: ConstMaxSize> ConstMaxSize for Result<T, E> {
    const MAX_SIZE: Option<usize> =
        max_size_add(Some(size_i32(1)), max_size_max(T::MAX_SIZE, E::MAX_SIZE));
}

impl<T> ConstMaxSize for PhantomData<T> {
    const MAX_SIZE: Option<usize> = Some(0);
}

impl ConstMaxSize for Infallible {
    const MAX_SIZE: Option<usize> = Some(0);
}

impl ConstMaxSize for () {
    const MAX_SIZE: Option<usize> = Some(0);
}

macro_rules! impl_tuple {
    ($($t:ident),+) => {
        impl<$($t: ConstMaxSize),+> ConstMaxSize for ($($t,)+) {
            const MAX_SIZE: Option<usize> = {
                let mut total: Option<usize> = Some(0);
                $( total = max_size_add(total, $t::MAX_SIZE); )+
                total
            };
        }
    };
}

impl_tuple!(A);
impl_tuple!(A, B);
impl_tuple!(A, B, C);
impl_tuple!(A, B, C, D);
impl_tuple!(A, B, C, D, E);
impl_tuple!(A, B, C, D, E, F);
impl_tuple!(A, B, C, D, E, F, G);
impl_tuple!(A, B, C, D, E, F, G, H);
impl_tuple!(A, B, C, D, E, F, G, H, I);
impl_tuple!(A, B, C, D, E, F, G, H, I, J);
impl_tuple!(A, B, C, D, E, F, G, H, I, J, K);
impl_tuple!(A, B, C, D, E, F, G, H, I, J, K, L);

impl<const MAX: u32> ConstMaxSize for BytesM<MAX> {
    const MAX_SIZE: Option<usize> = max_size_opaque_variable(MAX);
}

impl<const MAX: u32> ConstMaxSize for StringM<MAX> {
    const MAX_SIZE: Option<usize> = max_size_opaque_variable(MAX);
}

impl<const MAX: u32> ConstMaxSize for VecM<u8, MAX> {
    const MAX_SIZE: Option<usize> = max_size_opaque_variable(MAX);
}

// Pointer cut: an element-generic `VecM` is heap-backed indirection — a
// cycle edge for recursive types. Literal `None`; the `T: ConstMaxSize`
// bound exists only to keep this impl disjoint from `VecM<u8, MAX>`, and
// `StaticIfNoAlloc` mirrors the bound on `VecM` itself.
impl<T: ConstMaxSize + StaticIfNoAlloc, const MAX: u32> ConstMaxSize for VecM<T, MAX> {
    const MAX_SIZE: Option<usize> = None;
}

#[cfg(test)]
mod tests {
    use super::ConstMaxSize;
    use xdr_common::{BytesM, StringM, VecM};

    const fn bound<T: ConstMaxSize>() -> Option<usize> {
        T::MAX_SIZE
    }

    // Primitives: worst-case CXS widths.
    const _: () = assert!(matches!(bound::<u32>(), Some(5)));
    const _: () = assert!(matches!(bound::<i64>(), Some(9)));
    const _: () = assert!(matches!(bound::<bool>(), Some(1)));
    const _: () = assert!(matches!(bound::<f32>(), Some(4)));
    const _: () = assert!(matches!(bound::<u16>(), Some(3)));

    // Fixed opaque bytes: no padding in CXS.
    const _: () = assert!(matches!(bound::<[u8; 5]>(), Some(5)));
    // Element arrays multiply the element bound.
    const _: () = assert!(matches!(bound::<[u32; 3]>(), Some(15)));

    // Pointer cuts: all references bound to None.
    const _: () = assert!(bound::<&'static u32>().is_none());
    const _: () = assert!(bound::<&'static [u8]>().is_none());
    const _: () = assert!(bound::<&'static str>().is_none());
    const _: () = assert!(bound::<&'static [u32]>().is_none());

    // Owned heap `Box` is also a pointer cut (alloc only; no-alloc `Box`
    // aliases `T` and is not a pointer). None propagates through an enclosing fold.
    #[cfg(feature = "alloc")]
    const _: () = assert!(bound::<alloc::boxed::Box<u32>>().is_none());
    #[cfg(feature = "alloc")]
    const _: () = assert!(bound::<alloc::boxed::Box<[u8; 4]>>().is_none());
    #[cfg(feature = "alloc")]
    const _: () = assert!(bound::<Option<alloc::boxed::Box<u32>>>().is_none());

    // A type whose recursion passes through `Box` folds to `None` with no
    // const-eval cycle — the property the literal `None` protects. Mirrors
    // what `#[derive(ConstXDRSize)]` emits for `struct Rec(Option<Box<Rec>>)`.
    #[cfg(feature = "alloc")]
    struct Rec;
    #[cfg(feature = "alloc")]
    impl ConstMaxSize for Rec {
        const MAX_SIZE: Option<usize> = <Option<alloc::boxed::Box<Rec>> as ConstMaxSize>::MAX_SIZE;
    }
    #[cfg(feature = "alloc")]
    const _: () = assert!(bound::<Rec>().is_none());

    // Option adds the worst-case discriminant; None propagates.
    const _: () = assert!(matches!(bound::<Option<u32>>(), Some(6)));
    const _: () = assert!(bound::<Option<&'static u32>>().is_none());

    // Result takes the max arm plus discriminant.
    const _: () = assert!(matches!(bound::<Result<u32, i64>>(), Some(10)));

    // Tuples fold element bounds. Unit is zero.
    const _: () = assert!(matches!(bound::<(u16, i16)>(), Some(6)));
    const _: () = assert!(matches!(bound::<()>(), Some(0)));

    // Zero-sized markers.
    const _: () = assert!(matches!(bound::<core::marker::PhantomData<u64>>(), Some(0)));
    const _: () = assert!(matches!(bound::<core::convert::Infallible>(), Some(0)));

    // A zero-length array of unbounded elements still encodes zero bytes.
    const _: () = assert!(matches!(bound::<[&'static [u8]; 0]>(), Some(0)));

    // Bounded byte containers: exact. size_u32(16) == 1 in CXS.
    const _: () = assert!(matches!(bound::<BytesM<16>>(), Some(17)));
    const _: () = assert!(matches!(bound::<StringM<8>>(), Some(9)));
    const _: () = assert!(matches!(bound::<VecM<u8, 4>>(), Some(5)));

    // Element-generic VecM is heap indirection: literal None even with MAX.
    const _: () = assert!(bound::<VecM<u32, 5>>().is_none());

    // Unspelled MAX defaults to u32::MAX at the type level.
    #[cfg(target_pointer_width = "64")]
    const _: () = assert!(matches!(
        bound::<BytesM>(),
        Some(n) if n == 5 + u32::MAX as usize
    ));

    #[cfg(feature = "int128")]
    const _: () = assert!(matches!(bound::<i128>(), Some(18)));
    #[cfg(feature = "int128")]
    const _: () = assert!(matches!(bound::<u128>(), Some(18)));

    #[test]
    fn bounds_are_stable() {
        // Const asserts above are the real test; exercise at runtime too.
        assert_eq!(bound::<u32>(), Some(5));
        assert_eq!(bound::<Option<u32>>(), Some(6));
        assert_eq!(bound::<VecM<u32, 5>>(), None);
    }
}
