//! Variable-length data held as separate chunks.
//!
//! [`ChunkedBytes`] and [`ChunkedVec`] encode a sequence of chunks as the
//! XDR value of their concatenation, standing in for a `BytesM` or a `VecM`
//! without first copying the chunks together. They only write: a reader sees
//! one opaque or array with no chunk boundaries to recover, so decode into
//! the wire type.
//!
//! Both hold their chunks in any container that dereferences to a slice of
//! them, such as a `Vec`, a slice reference, a `Box<[_]>` or an `Arc<[_]>`.
//! Equality compares the chunks, not their concatenation. Writing fails with
//! [`WriteError::InvalidArrayLength`] when the chunks together hold more
//! than `MAX` bytes or elements, as it does for an oversized `BytesM<MAX>`
//! or `VecM<T, MAX>`.

use crate::serialize::writer::*;
use crate::serialize::writer_unchecked::*;
use crate::{EncodesAs, WriteXDR};
use core::ops::Deref;
use xdr_common::{BytesM, StaticIfNoAlloc, VecM};

/// Variable-length opaque data held as a sequence of byte chunks.
///
/// `ChunkedBytes<C, MAX>` is [`EncodesAs<BytesM<MAX>>`](EncodesAs). Each
/// chunk is anything that is `AsRef<[u8]>`, such as a `Vec<u8>`, a `&[u8]`
/// or an `Arc<[u8]>`, and is written straight from its buffer, followed by
/// one TXS padding for the whole opaque. See the
/// [module documentation](self).
///
/// # Example
///
/// ```rust
/// # #[cfg(feature = "alloc")] {
/// use xdrs::{BytesM, ChunkedBytes, txs};
///
/// let chunked = ChunkedBytes::<_>(vec![&b"hello, "[..], &b"world"[..]]);
/// let joined = BytesM::<{ u32::MAX }>(b"hello, world".to_vec());
/// assert_eq!(txs::encode(&chunked).unwrap(), txs::encode(&joined).unwrap());
/// # }
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChunkedBytes<C, const MAX: u32 = { u32::MAX }>(pub C);

/// A variable-length array held as a sequence of element chunks.
///
/// `ChunkedVec<C, MAX>` is [`EncodesAs<VecM<X, MAX>>`](EncodesAs) whenever
/// its elements are `EncodesAs<X>`. Each chunk dereferences to a slice of
/// elements, such as a `Vec<T>`, a `&[T]` or an `Arc<[T]>`. See the
/// [module documentation](self).
///
/// # Example
///
/// ```rust
/// # #[cfg(feature = "alloc")] {
/// use xdrs::{ChunkedVec, VecM, txs};
///
/// let chunked = ChunkedVec::<_>(vec![vec![1u32, 2], vec![3]]);
/// let joined = VecM::<u32>(vec![1, 2, 3]);
/// assert_eq!(txs::encode(&chunked).unwrap(), txs::encode(&joined).unwrap());
/// # }
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChunkedVec<C, const MAX: u32 = { u32::MAX }>(pub C);

impl<C, B, const MAX: u32> ChunkedBytes<C, MAX>
where
    C: Deref<Target = [B]>,
    B: AsRef<[u8]>,
{
    /// The total number of bytes across all chunks.
    pub fn len(&self) -> usize {
        self.0.iter().map(|chunk| chunk.as_ref().len()).sum()
    }

    /// Whether every chunk is empty.
    pub fn is_empty(&self) -> bool {
        self.0.iter().all(|chunk| chunk.as_ref().is_empty())
    }
}

impl<C, S, T, const MAX: u32> ChunkedVec<C, MAX>
where
    C: Deref<Target = [S]>,
    S: Deref<Target = [T]>,
{
    /// The total number of elements across all chunks.
    pub fn len(&self) -> usize {
        self.0.iter().map(|chunk| chunk.len()).sum()
    }

    /// Whether every chunk is empty.
    pub fn is_empty(&self) -> bool {
        self.0.iter().all(|chunk| chunk.is_empty())
    }
}

// A variable-length opaque is its length followed by the fixed-length opaque
// of its bytes, so the chunks can be written back to back as the pieces of
// one fixed-length opaque.
impl<C, B, const MAX: u32> WriteXDR for ChunkedBytes<C, MAX>
where
    C: Deref<Target = [B]>,
    B: AsRef<[u8]>,
{
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
        let len = checked_len::<MAX>(self.0.iter().map(|chunk| chunk.as_ref().len()))?;
        w.write_u32(len)?;
        for chunk in self.0.iter() {
            w.write_opaque_unpadded(chunk.as_ref())?;
        }
        w.write_opaque_padding(len as usize)
    }

    unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
        let len = self.len();
        debug_assert!(len <= MAX as usize, "ChunkedBytes exceeds declared bound");
        // SAFETY: the caller guarantees the value respects its declared
        // bound and that the writer has space for the encoded form, which is
        // the length, the chunks and the padding written here.
        unsafe {
            w.write_u32(len as u32);
            for chunk in self.0.iter() {
                w.write_opaque_unpadded(chunk.as_ref());
            }
            w.write_opaque_padding(len);
        }
    }
}

// A variable-length array is its length followed by its elements.
impl<C, S, T, const MAX: u32> WriteXDR for ChunkedVec<C, MAX>
where
    C: Deref<Target = [S]>,
    S: Deref<Target = [T]>,
    T: WriteXDR,
{
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
        let len = checked_len::<MAX>(self.0.iter().map(|chunk| chunk.len()))?;
        w.write_u32(len)?;
        for elt in self.0.iter().flat_map(|chunk| chunk.iter()) {
            elt.write_xdr(w)?;
        }
        Ok(())
    }

    unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
        let len = self.len();
        debug_assert!(len <= MAX as usize, "ChunkedVec exceeds declared bound");
        // SAFETY: the caller guarantees the value respects its declared
        // bound and that the writer has space for the length prefix and
        // every element.
        unsafe {
            w.write_u32(len as u32);
            for elt in self.0.iter().flat_map(|chunk| chunk.iter()) {
                elt.write_xdr_unchecked(w);
            }
        }
    }
}

/// The total of the chunk lengths `lens` as an XDR length, or
/// [`WriteError::InvalidArrayLength`] if it exceeds `MAX`.
///
/// Sums in `u32`: on a 32-bit target, repeated references to one large
/// chunk could overflow a `usize` total.
fn checked_len<const MAX: u32>(mut lens: impl Iterator<Item = usize>) -> WriteResult<u32> {
    lens.try_fold(0u32, |total, len| {
        u32::try_from(len)
            .ok()
            .and_then(|len| total.checked_add(len))
    })
    .filter(|&len| len <= MAX)
    .ok_or(WriteError::InvalidArrayLength)
}

impl<C, B, const MAX: u32> EncodesAs<Self> for ChunkedBytes<C, MAX>
where
    C: Deref<Target = [B]>,
    B: AsRef<[u8]>,
{
}

impl<C, B, const MAX: u32> EncodesAs<BytesM<MAX>> for ChunkedBytes<C, MAX>
where
    C: Deref<Target = [B]>,
    B: AsRef<[u8]>,
{
}

impl<C, S, T, const MAX: u32> EncodesAs<Self> for ChunkedVec<C, MAX>
where
    C: Deref<Target = [S]>,
    S: Deref<Target = [T]>,
    T: WriteXDR,
{
}

impl<C, S, T, X, const MAX: u32> EncodesAs<VecM<X, MAX>> for ChunkedVec<C, MAX>
where
    C: Deref<Target = [S]>,
    S: Deref<Target = [T]>,
    T: EncodesAs<X>,
    X: StaticIfNoAlloc,
{
}

// `ChunkedBytes` shares the bound of `BytesM`. `ChunkedVec` is bounded like
// the element-generic `VecM`: literal `None`, because its elements sit
// behind indirection.
macro_rules! impl_const_max_size {
    ($($format:ident),*) => {$(
        impl<C, const MAX: u32> crate::$format::ConstMaxSize for ChunkedBytes<C, MAX> {
            const MAX_SIZE: Option<usize> =
                crate::$format::const_writer::max_size_opaque_variable(MAX);
        }

        impl<C, const MAX: u32> crate::$format::ConstMaxSize for ChunkedVec<C, MAX> {
            const MAX_SIZE: Option<usize> = None;
        }
    )*};
}

impl_const_max_size!(cxs, txs);
