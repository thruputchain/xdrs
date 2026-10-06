//! Variable-length opaque data held as separate chunks.

use crate::serialize::writer::*;
use crate::serialize::writer_unchecked::*;
use crate::{EncodesAs, WriteXDR};
use alloc::vec::Vec;
use xdr_common::BytesM;

/// Variable-length opaque data held as a sequence of chunks, encoded as the
/// XDR opaque of their concatenation.
///
/// `ChunkedBytes<B, MAX>` is [`EncodesAs<BytesM<MAX>>`](EncodesAs): it can be
/// passed where a `BytesM<MAX>` argument is expected, and its chunks are
/// written in place instead of first being copied into one buffer. It only
/// writes. A reader sees a single opaque and has no chunk boundaries to
/// recover, so decode into `BytesM`.
///
/// Writing fails with [`WriteError::InvalidArrayLength`] when the chunks
/// together hold more than `MAX` bytes, as it does for an oversized
/// `BytesM<MAX>`.
///
/// # Example
///
/// ```rust
/// use xdrs::{BytesM, ChunkedBytes, txs};
///
/// let chunked = ChunkedBytes::<_>(vec![&b"hello, "[..], &b"world"[..]]);
/// let owned = BytesM::<{ u32::MAX }>(b"hello, world".to_vec());
/// assert_eq!(txs::encode(&chunked)?, txs::encode(&owned)?);
/// # Ok::<(), xdrs::WriteError>(())
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChunkedBytes<B, const MAX: u32 = { u32::MAX }>(pub Vec<B>);

impl<B: AsRef<[u8]>, const MAX: u32> ChunkedBytes<B, MAX> {
    /// The total number of bytes across all chunks.
    pub fn len(&self) -> usize {
        self.0.iter().map(|chunk| chunk.as_ref().len()).sum()
    }

    /// Whether every chunk is empty.
    pub fn is_empty(&self) -> bool {
        self.0.iter().all(|chunk| chunk.as_ref().is_empty())
    }
}

impl<B: AsRef<[u8]>, const MAX: u32> WriteXDR for ChunkedBytes<B, MAX> {
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
        if self.len() > MAX as usize {
            return Err(WriteError::InvalidArrayLength);
        }
        w.write_opaque_variable_chunks(&self.0)
    }

    unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
        debug_assert!(
            self.len() <= MAX as usize,
            "ChunkedBytes exceeds declared bound"
        );
        // SAFETY: the caller guarantees the value respects its declared
        // bound and that the writer has space for the encoded form.
        unsafe { w.write_opaque_variable_chunks(&self.0) }
    }
}

impl<B: AsRef<[u8]>, const MAX: u32> EncodesAs<Self> for ChunkedBytes<B, MAX> {}

impl<B: AsRef<[u8]>, const MAX: u32> EncodesAs<BytesM<MAX>> for ChunkedBytes<B, MAX> {}
