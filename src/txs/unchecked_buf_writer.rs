//! TXS writer that writes into a pre-allocated buffer without runtime checks.
//!
//! This module provides [`UncheckedBufWriter`], which implements
//! [`WriterUnchecked`](crate::serialize::writer_unchecked::WriterUnchecked)
//! for writing TXS-encoded XDR data into a pre-allocated buffer of
//! `MaybeUninit<u8>` that the caller guarantees is large enough.

use super::put::*;
use crate::mu;
use core::mem::MaybeUninit;

type Mu8 = MaybeUninit<u8>;

/// A writer for TXS-encoded XDR data into a pre-allocated buffer, without
/// runtime bounds checks.
///
/// This writer implements the
/// [`WriterUnchecked`](crate::serialize::writer_unchecked::WriterUnchecked)
/// trait for the TXS format. Unlike
/// [`IntoBufWriter`](crate::txs::into_buf_writer::IntoBufWriter), it never
/// checks that space remains in the buffer: every write method is `unsafe`,
/// and the caller must guarantee up front that the buffer is large enough
/// for everything written (see
/// [`encode_into_unchecked`](crate::txs::encode_into_unchecked)).
///
/// Buffer overflow is caught by `debug_assert!` in debug builds; in release
/// builds it is undefined behavior.
pub struct UncheckedBufWriter<'a> {
    buf: &'a mut [Mu8],
    written: usize,
}

impl<'a> UncheckedBufWriter<'a> {
    /// Creates a new `UncheckedBufWriter` that writes into the given buffer.
    ///
    /// Construction is safe; the unsafety attaches to the write methods,
    /// which require the buffer to have enough space remaining.
    pub fn new(buf: &'a mut [Mu8]) -> Self {
        Self { buf, written: 0 }
    }

    /// Returns the number of bytes written so far.
    pub fn written(&self) -> usize {
        self.written
    }

    /// # Safety
    /// `self.written + len` must not exceed `self.buf.len()`.
    #[inline]
    unsafe fn reserve<'s>(&'s mut self, len: usize) -> &'s mut [Mu8]
    where
        'a: 's,
    {
        let extent = self.written + len;
        debug_assert!(
            extent <= self.buf.len(),
            "UncheckedBufWriter: buffer overflow"
        );

        let start = self.written;
        self.written = extent;

        // SAFETY: the caller guarantees extent <= self.buf.len(), and
        // start <= extent (since len >= 0 and start = old written).
        unsafe { self.buf.get_unchecked_mut(start..extent) }
    }
}

impl PutUnchecked for UncheckedBufWriter<'_> {
    #[inline]
    unsafe fn put_bytes(&mut self, bytes: &[u8]) {
        // SAFETY: the caller guarantees space for bytes.len() more bytes;
        // reserve returns a slice of exactly that length.
        unsafe {
            let buf = self.reserve(bytes.len());
            mu::write_copy_of_slice(buf, bytes);
        }
    }

    #[inline]
    unsafe fn put_opaque_variable(&mut self, bytes: &[u8]) {
        debug_assert!(bytes.len() <= u32::MAX as usize);

        static PAD: [u8; 3] = [0; 3];

        let len = bytes.len();
        let rem = if len.is_multiple_of(4) {
            0
        } else {
            4 - len % 4
        };
        let size = 4 + len + rem;

        // SAFETY: the caller guarantees space for `size` more bytes. The
        // three sub-slices partition the reserved slice (of length
        // `4 + len + rem`) exactly, so every range is in bounds and each
        // copy has matching lengths; rem <= 3 = PAD.len().
        unsafe {
            let buf = self.reserve(size);
            mu::write_copy_of_slice(buf.get_unchecked_mut(0..4), &(len as u32).to_be_bytes());
            mu::write_copy_of_slice(buf.get_unchecked_mut(4..4 + len), bytes);
            mu::write_copy_of_slice(
                buf.get_unchecked_mut(4 + len..size),
                PAD.get_unchecked(0..rem),
            );
        }
    }

    #[inline]
    unsafe fn put_opaque_fixed(&mut self, bytes: &[u8]) {
        debug_assert!(bytes.len() <= u32::MAX as usize);

        static PAD: [u8; 3] = [0; 3];

        let len = bytes.len();
        let rem = if len.is_multiple_of(4) {
            0
        } else {
            4 - len % 4
        };
        let size = len + rem;

        // SAFETY: the caller guarantees space for `size` more bytes; the two
        // sub-slices partition the reserved slice exactly; rem <= 3 = PAD.len().
        unsafe {
            let buf = self.reserve(size);
            mu::write_copy_of_slice(buf.get_unchecked_mut(0..len), bytes);
            mu::write_copy_of_slice(buf.get_unchecked_mut(len..size), PAD.get_unchecked(0..rem));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serialize::writer_unchecked::WriterUnchecked;

    #[test]
    fn ints_and_bool() {
        let mut buf = [MaybeUninit::uninit(); 8];
        let mut w = UncheckedBufWriter::new(&mut buf);
        // SAFETY: 4 + 4 bytes fit in the 8-byte buffer.
        unsafe {
            WriterUnchecked::<TXS>::write_u32(&mut w, 42);
            WriterUnchecked::<TXS>::write_bool(&mut w, true);
        }
        let written = w.written();
        assert_eq!(written, 8);
        // SAFETY: the first `written` bytes were just initialized.
        let bytes = unsafe { mu::assume_init_ref(&buf[..written]) };
        assert_eq!(bytes, [0, 0, 0, 42, 0, 0, 0, 1]);
    }

    #[test]
    fn opaque_variable_pads() {
        let mut buf = [MaybeUninit::uninit(); 8];
        let mut w = UncheckedBufWriter::new(&mut buf);
        // SAFETY: 4-byte length prefix + 2 data bytes + 2 pad bytes fit exactly.
        unsafe { WriterUnchecked::<TXS>::write_opaque_variable(&mut w, b"hi") };
        let written = w.written();
        assert_eq!(written, 8);
        // SAFETY: the first `written` bytes were just initialized.
        let bytes = unsafe { mu::assume_init_ref(&buf[..written]) };
        assert_eq!(bytes, [0, 0, 0, 2, 0x68, 0x69, 0, 0]);
    }

    #[test]
    fn opaque_fixed_pads() {
        let mut buf = [MaybeUninit::uninit(); 4];
        let mut w = UncheckedBufWriter::new(&mut buf);
        // SAFETY: 3 data bytes + 1 pad byte fit exactly.
        unsafe { WriterUnchecked::<TXS>::write_opaque_fixed(&mut w, &[0xAA, 0xBB, 0xCC]) };
        let written = w.written();
        assert_eq!(written, 4);
        // SAFETY: the first `written` bytes were just initialized.
        let bytes = unsafe { mu::assume_init_ref(&buf[..written]) };
        assert_eq!(bytes, [0xAA, 0xBB, 0xCC, 0x00]);
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "UncheckedBufWriter: buffer overflow")]
    fn overflow_debug_asserts() {
        let mut buf = [MaybeUninit::uninit(); 3];
        let mut w = UncheckedBufWriter::new(&mut buf);
        // Deliberate contract violation: caught by the debug_assert before
        // any out-of-bounds write happens.
        unsafe { WriterUnchecked::<TXS>::write_u32(&mut w, 1) };
    }
}
