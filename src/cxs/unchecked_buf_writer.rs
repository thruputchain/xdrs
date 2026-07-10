//! CXS writer that writes into a pre-allocated buffer without runtime checks.
//!
//! This module provides [`UncheckedBufWriter`], which implements
//! [`WriterUnchecked`](crate::serialize::writer_unchecked::WriterUnchecked)
//! for writing CXS-encoded XDR data into a pre-allocated buffer of
//! `MaybeUninit<u8>` that the caller guarantees is large enough.

use super::put::*;
use crate::mu;
use core::mem::MaybeUninit;

type Mu8 = MaybeUninit<u8>;

/// A writer for CXS-encoded XDR data into a pre-allocated buffer, without
/// runtime bounds checks.
///
/// This writer implements the
/// [`WriterUnchecked`](crate::serialize::writer_unchecked::WriterUnchecked)
/// trait for the CXS format. Unlike
/// [`IntoBufWriter`](crate::cxs::into_buf_writer::IntoBufWriter), it never
/// checks that space remains in the buffer: every write method is `unsafe`,
/// and the caller must guarantee up front that the buffer is large enough
/// for everything written (see
/// [`encode_into_unchecked`](crate::cxs::encode_into_unchecked)).
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
    unsafe fn put_byte(&mut self, b: u8) {
        // SAFETY: the caller guarantees space for 1 more byte; reserve
        // returns a slice of exactly length 1.
        unsafe {
            let p = self.reserve(1);
            p.get_unchecked_mut(0).write(b);
        }
    }

    #[inline]
    unsafe fn put_bytes(&mut self, bytes: &[u8]) {
        // SAFETY: the caller guarantees space for bytes.len() more bytes;
        // reserve returns a slice of exactly that length.
        unsafe {
            let buf = self.reserve(bytes.len());
            mu::write_copy_of_slice(buf, bytes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serialize::writer_unchecked::WriterUnchecked;

    // Writes `v` with both the checked and unchecked CXS writers and
    // asserts identical output.
    fn check<T: crate::WriteXDR>(v: &T, write_unchecked: impl FnOnce(&mut UncheckedBufWriter)) {
        let mut cbuf = [MaybeUninit::uninit(); 64];
        let clen = crate::cxs::encode_into(v, &mut cbuf).unwrap();

        let mut ubuf = [MaybeUninit::uninit(); 64];
        let mut w = UncheckedBufWriter::new(&mut ubuf);
        write_unchecked(&mut w);
        let ulen = w.written();

        assert_eq!(clen, ulen);
        // SAFETY: both writers initialized their first `len` bytes.
        let (c, u) = unsafe {
            (
                mu::assume_init_ref(&cbuf[..clen]),
                mu::assume_init_ref(&ubuf[..ulen]),
            )
        };
        assert_eq!(c, u);
    }

    #[test]
    fn u32_all_widths() {
        // Header-byte, 1-, 2-, and 4-byte representations.
        for n in [0u32, 62, 63, 255, 256, 65535, 65536, u32::MAX] {
            // SAFETY: the 64-byte buffer covers the worst-case size (5).
            check(&n, |w| unsafe { WriterUnchecked::<CXS>::write_u32(w, n) });
        }
    }

    #[test]
    fn i32_all_widths() {
        for n in [
            0i32,
            31,
            -31,
            32,
            -32,
            127,
            -128,
            128,
            -129,
            i32::MAX,
            i32::MIN,
        ] {
            // SAFETY: the 64-byte buffer covers the worst-case size (5).
            check(&n, |w| unsafe { WriterUnchecked::<CXS>::write_i32(w, n) });
        }
    }

    #[test]
    fn i64_u64_and_16bit() {
        for n in [0i64, i32::MAX as i64, i32::MAX as i64 + 1, i64::MIN] {
            // SAFETY: the 64-byte buffer covers the worst-case size (9).
            check(&n, |w| unsafe { WriterUnchecked::<CXS>::write_i64(w, n) });
        }
        for n in [0u64, u32::MAX as u64, u32::MAX as u64 + 1, u64::MAX] {
            // SAFETY: the 64-byte buffer covers the worst-case size (9).
            check(&n, |w| unsafe { WriterUnchecked::<CXS>::write_u64(w, n) });
        }
        for n in [0u16, 62, 63, 255, 256, u16::MAX] {
            // SAFETY: the 64-byte buffer covers the worst-case size (3).
            check(&n, |w| unsafe { WriterUnchecked::<CXS>::write_u16(w, n) });
        }
        for n in [0i16, 31, -31, 32, i16::MAX, i16::MIN] {
            // SAFETY: the 64-byte buffer covers the worst-case size (3).
            check(&n, |w| unsafe { WriterUnchecked::<CXS>::write_i16(w, n) });
        }
    }

    #[test]
    fn bool_floats_opaque() {
        // SAFETY (each): the 64-byte buffer covers the worst-case size.
        check(&true, |w| unsafe {
            WriterUnchecked::<CXS>::write_bool(w, true)
        });
        check(&1.5f32, |w| unsafe {
            WriterUnchecked::<CXS>::write_f32(w, 1.5)
        });
        check(&1.5f64, |w| unsafe {
            WriterUnchecked::<CXS>::write_f64(w, 1.5)
        });
        let bytes: &[u8] = b"hello";
        check(&bytes, |w| unsafe {
            WriterUnchecked::<CXS>::write_opaque_variable(w, b"hello")
        });
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "UncheckedBufWriter: buffer overflow")]
    fn overflow_debug_asserts() {
        let mut buf: [Mu8; 0] = [];
        let mut w = UncheckedBufWriter::new(&mut buf);
        // Deliberate contract violation: caught by the debug_assert before
        // any out-of-bounds write happens.
        unsafe { WriterUnchecked::<CXS>::write_u32(&mut w, 1) };
    }
}
