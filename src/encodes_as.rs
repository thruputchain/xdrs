//! Declaring that one type serializes like another.
//!
//! [`EncodesAs<X>`] is the Rust counterpart of x23's `serialize_equiv`: it
//! lets an API whose wire type is `X` accept any value that encodes to the
//! same bytes, so a caller can serialize from borrowed or shared storage
//! instead of first building an owned `X`.

use crate::write_xdr::WriteXDR;
use xdr_common::{BytesM, StaticIfNoAlloc, StringM, VecM};

/// Types whose XDR encoding can stand in for an `X`.
///
/// `T: EncodesAs<X>` promises that [`WriteXDR::write_xdr`] on a `T` writes
/// exactly the bytes that `X` would write for the corresponding value.
/// There is no separate write method: code bounded on `EncodesAs<X>` encodes
/// through the `WriteXDR` supertrait. Nothing checks the promise at runtime,
/// and in particular `[U]` standing in for a `VecM<X, N>` does not check
/// that its length is at most `N`; an oversized value encodes, and the
/// decoder on the other end rejects it.
///
/// The implementations are:
///
/// - **Identity**: every type this crate implements `WriteXDR` for, except
///   `&T`, `&mut T`, `Box<T>` and `Arc<T>`, is `EncodesAs<Self>`, as is every type with
///   `#[derive(WriteXDR)]`. A hand-written `WriteXDR` impl should add
///   `impl EncodesAs<Self> for T {}` alongside it.
/// - **Pointers**: `&U`, `Box<U>` and `Arc<U>` are `EncodesAs<X>` whenever
///   `U: EncodesAs<X>`.
/// - **Slices**: `[U]` is `EncodesAs<VecM<X, N>>` whenever `U: EncodesAs<X>`.
/// - **Chunks**: [`ChunkedBytes<_, N>`](crate::ChunkedBytes) is
///   `EncodesAs<BytesM<N>>`, and [`ChunkedVec<_, N>`](crate::ChunkedVec) is
///   `EncodesAs<VecM<X, N>>` whenever its elements are `EncodesAs<X>`.
///
/// There is no blanket identity `impl<X: WriteXDR> EncodesAs<X> for X`:
/// because `&U` and `Box<U>` are themselves `WriteXDR`, it would overlap the
/// pointer impls, which coherence rejects.
///
/// # Example
///
/// An RPC client that takes its argument as `&(impl EncodesAs<X> + ?Sized)`
/// accepts both the owned wire type and a slice of shared elements:
///
/// ```rust
/// # #[cfg(feature = "alloc")] {
/// use std::sync::Arc;
/// use xdrs::{EncodesAs, VecM, txs};
///
/// fn send(batch: &(impl EncodesAs<VecM<u32, 8>> + ?Sized)) -> Vec<u8> {
///     txs::encode(batch).unwrap()
/// }
///
/// let owned: VecM<u32, 8> = vec![1, 2].try_into().unwrap();
/// let shared: Vec<Arc<u32>> = vec![Arc::new(1), Arc::new(2)];
/// assert_eq!(send(&owned), send(&shared[..]));
/// # }
/// ```
pub trait EncodesAs<X: ?Sized>: WriteXDR {}

impl<X: ?Sized, U: EncodesAs<X> + ?Sized> EncodesAs<X> for &U {}

#[cfg(feature = "alloc")]
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
impl<X: ?Sized, U: EncodesAs<X>> EncodesAs<X> for alloc::boxed::Box<U> {}

#[cfg(not(feature = "alloc"))]
impl<X: ?Sized, U: EncodesAs<X>> EncodesAs<X> for xdr_common::Box<U> {}

#[cfg(all(feature = "alloc", target_has_atomic = "ptr"))]
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
impl<X: ?Sized, U: EncodesAs<X> + ?Sized> EncodesAs<X> for alloc::sync::Arc<U> {}

impl<X: StaticIfNoAlloc, U: EncodesAs<X>, const N: u32> EncodesAs<VecM<X, N>> for [U] {}

// Identity impls. These must track the `WriteXDR` impls in `write_xdr`,
// `int128` and `heapless`, leaving out the pointer types forwarded above.

macro_rules! encodes_as_self {
    ( $( $t:ty ),* $(,)? ) => { $(
        impl EncodesAs<$t> for $t {}
    )* };
}

encodes_as_self! {
    i16, i32, i64, u16, u32, u64, bool, f32, f64, str, (), core::convert::Infallible,
}

#[cfg(feature = "int128")]
encodes_as_self! { i128, u128 }

#[cfg(feature = "alloc")]
encodes_as_self! { alloc::string::String }

// Generic types whose `WriteXDR` impls have bounds; `where Self: WriteXDR`
// keeps those bounds in one place, and also lets one impl cover the opaque
// (`u8`) and element-wise variants of `[T]`, `[T; N]` and the vector types.
macro_rules! encodes_as_self_where_write_xdr {
    ( $( [$($g:tt)*] $t:ty ),* $(,)? ) => { $(
        impl<$($g)*> EncodesAs<$t> for $t where $t: WriteXDR {}
    )* };
}

encodes_as_self_where_write_xdr! {
    [T] [T],
    [T, const N: usize] [T; N],
    [T] Option<T>,
    [T, E] Result<T, E>,
    [T] core::marker::PhantomData<T>,
    [const MAX: u32] BytesM<MAX>,
    [const MAX: u32] StringM<MAX>,
    [T: StaticIfNoAlloc, const MAX: u32] VecM<T, MAX>,
}

#[cfg(feature = "alloc")]
encodes_as_self_where_write_xdr! { [T] alloc::vec::Vec<T> }

#[cfg(feature = "heapless")]
encodes_as_self_where_write_xdr! {
    [T, const N: usize] ::heapless::Vec<T, N>,
    [const N: usize] ::heapless::String<N>,
}

macro_rules! encodes_as_self_tuples {
    () => {};
    ( $t0:ident $(, $t:ident)* ) => {
        encodes_as_self_where_write_xdr! { [$t0 $(, $t)*] ($t0, $($t,)*) }
        encodes_as_self_tuples! { $($t),* }
    };
}

encodes_as_self_tuples! { T0, T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15 }
