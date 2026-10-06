//! Variable-length opaque data held as separate chunks.

use crate::serialize::writer::*;
use crate::serialize::writer_unchecked::*;
use crate::{EncodesAs, WriteXDR};
use alloc::vec::Vec;
use core::convert::Infallible;
use xdr_common::BytesM;

/// Variable-length opaque data held as a sequence of chunks, encoded as the
/// XDR opaque of their concatenation.
///
/// `ChunkedBytes<B, MAX>` is [`EncodesAs<BytesM<MAX>>`](EncodesAs): it can be
/// passed where a `BytesM<MAX>` argument is expected, and its chunks are
/// written straight from their buffers instead of first being copied into
/// one. Only the at most three bytes that straddle a chunk boundary are
/// copied. It only writes. A reader sees a single opaque and has no chunk
/// boundaries to recover, so decode into `BytesM`.
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

// A variable-length opaque is its length followed by the fixed-length opaque
// of its bytes. A fixed-length opaque whose length is a multiple of 4 has no
// padding, so the payload can be written as several fixed-length opaques
// provided every one but the last is a multiple of 4 bytes long: then the
// only padding is the last piece's, which is the payload's own.
impl<B: AsRef<[u8]>, const MAX: u32> WriteXDR for ChunkedBytes<B, MAX> {
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
        // Sum in `u32`: on a 32-bit target, repeated references to one
        // large chunk could overflow a `usize` total.
        let len = match total_len(&self.0) {
            Some(len) if len <= MAX => len,
            _ => return Err(WriteError::InvalidArrayLength),
        };
        w.write_u32(len)?;
        for_each_aligned_piece(&self.0, |piece| w.write_opaque_fixed(piece))
    }

    unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
        let len = self.len();
        debug_assert!(len <= MAX as usize, "ChunkedBytes exceeds declared bound");
        // SAFETY: the caller guarantees the value respects its declared
        // bound and that the writer has space for the encoded form, which is
        // the length followed by the pieces written here.
        unsafe { w.write_u32(len as u32) };
        let Ok(()) = for_each_aligned_piece::<_, Infallible>(&self.0, |piece| {
            // SAFETY: as above.
            unsafe { w.write_opaque_fixed(piece) };
            Ok(())
        });
    }
}

impl<B: AsRef<[u8]>, const MAX: u32> crate::cxs::ConstMaxSize for ChunkedBytes<B, MAX> {
    const MAX_SIZE: Option<usize> = crate::cxs::const_writer::max_size_opaque_variable(MAX);
}

impl<B: AsRef<[u8]>, const MAX: u32> crate::txs::ConstMaxSize for ChunkedBytes<B, MAX> {
    const MAX_SIZE: Option<usize> = crate::txs::const_writer::max_size_opaque_variable(MAX);
}

impl<B: AsRef<[u8]>, const MAX: u32> EncodesAs<Self> for ChunkedBytes<B, MAX> {}

impl<B: AsRef<[u8]>, const MAX: u32> EncodesAs<BytesM<MAX>> for ChunkedBytes<B, MAX> {}

/// The total length of `chunks`, or `None` if it does not fit in an XDR
/// length.
fn total_len<B: AsRef<[u8]>>(chunks: &[B]) -> Option<u32> {
    chunks.iter().try_fold(0u32, |total, chunk| {
        u32::try_from(chunk.as_ref().len())
            .ok()
            .and_then(|len| total.checked_add(len))
    })
}

/// Passes the concatenation of `chunks` to `write` as consecutive pieces,
/// every one but the last a multiple of 4 bytes long. Each chunk's 4-byte
/// aligned middle is passed in place; bytes straddling a chunk boundary are
/// gathered into a 4-byte piece first.
fn for_each_aligned_piece<B: AsRef<[u8]>, E>(
    chunks: &[B],
    mut write: impl FnMut(&[u8]) -> Result<(), E>,
) -> Result<(), E> {
    let mut carry = [0u8; 4];
    let mut carried = 0;
    for chunk in chunks {
        let mut rest = chunk.as_ref();
        if carried > 0 {
            let take = (4 - carried).min(rest.len());
            carry[carried..carried + take].copy_from_slice(&rest[..take]);
            carried += take;
            rest = &rest[take..];
            if carried < 4 {
                continue;
            }
            write(&carry)?;
        }
        let aligned = rest.len() - rest.len() % 4;
        if aligned > 0 {
            write(&rest[..aligned])?;
        }
        let tail = &rest[aligned..];
        carry[..tail.len()].copy_from_slice(tail);
        carried = tail.len();
    }
    write(&carry[..carried])
}
