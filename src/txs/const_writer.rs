//! Const-compatible TXS encoding.
//!
//! This module provides [`ConstWriter`], which encodes TXS data into a
//! fixed-size buffer in `const` contexts, along with `size_*` functions
//! that compute the encoded size of each primitive type.

use core::mem::MaybeUninit;

/// A const-compatible writer for TXS (Traditional XDR Serialization).
///
/// Writes into a caller-provided buffer, panicking on overflow (a compile-time
/// error in `const` contexts). The TXS encoding rules live here exactly once.
///
/// Two entry points are provided: [`new`](Self::new) accepts an initialized
/// `&mut [u8]`, while [`new_uninit`](Self::new_uninit) accepts a
/// `&mut [MaybeUninit<u8>]` and avoids the cost of zero-initializing the
/// backing buffer.
///
/// The backing storage is held as `MaybeUninit<u8>`, but the writer only ever
/// stores initialized bytes: `buf[..pos]` is always fully initialized because
/// `pos` advances only over bytes just written.
pub struct ConstWriter<'a> {
    buf: &'a mut [MaybeUninit<u8>],
    pos: usize,
}

impl<'a> ConstWriter<'a> {
    /// Creates a writer over `buf`, starting at position 0.
    pub const fn new(buf: &'a mut [u8]) -> Self {
        // SAFETY: `MaybeUninit<u8>` has the same layout as `u8`, so reinterpreting
        // `&mut [u8]` as `&mut [MaybeUninit<u8>]` (same length metadata) is sound.
        // This direction never de-initializes the caller's buffer: the writer only
        // ever writes fully-initialized bytes into the slice (see `put_byte`/
        // `put_bytes`) and never reads or exposes an uninitialized element, so
        // every byte the caller passed in remains initialized for its own use.
        let buf = unsafe { &mut *(buf as *mut [u8] as *mut [MaybeUninit<u8>]) };
        Self { buf, pos: 0 }
    }

    /// Creates a writer over an uninitialized `buf`, starting at position 0.
    ///
    /// This is the zero-init-free entry point: unlike [`new`](Self::new), it does
    /// not require the caller to zero-initialize the backing buffer first, avoiding
    /// the `memset` of a `[0u8; N]` allocation on hot paths (for example, encoding a
    /// contract abort message).
    pub const fn new_uninit(buf: &'a mut [MaybeUninit<u8>]) -> Self {
        Self { buf, pos: 0 }
    }

    /// Returns the number of bytes written so far.
    pub const fn pos(&self) -> usize {
        self.pos
    }

    /// Returns the bytes written so far (the filled prefix of the buffer).
    pub const fn as_bytes(&self) -> &[u8] {
        let init = self.buf.split_at(self.pos).0;
        // SAFETY: writer invariant — `buf[..pos]` is always initialized because
        // `pos` only advances over bytes just written by `put_byte`/`put_bytes`.
        unsafe { crate::mu::assume_init_ref(init) }
    }

    // -- internal helpers --

    const fn put_byte(&mut self, b: u8) {
        self.buf[self.pos] = MaybeUninit::new(b);
        self.pos += 1;
    }

    const fn put_bytes(&mut self, bytes: &[u8]) {
        let mut i = 0;
        while i < bytes.len() {
            self.buf[self.pos + i] = MaybeUninit::new(bytes[i]);
            i += 1;
        }
        self.pos += bytes.len();
    }

    // -- integer writes --

    /// Writes a signed 32-bit integer in big-endian format (4 bytes).
    pub const fn write_i32(&mut self, n: i32) {
        let b = n.to_be_bytes();
        self.put_bytes(&b);
    }

    /// Writes an unsigned 32-bit integer in big-endian format (4 bytes).
    pub const fn write_u32(&mut self, n: u32) {
        let b = n.to_be_bytes();
        self.put_bytes(&b);
    }

    /// Writes a signed 64-bit integer in big-endian format (8 bytes).
    pub const fn write_i64(&mut self, n: i64) {
        let b = n.to_be_bytes();
        self.put_bytes(&b);
    }

    /// Writes an unsigned 64-bit integer in big-endian format (8 bytes).
    pub const fn write_u64(&mut self, n: u64) {
        let b = n.to_be_bytes();
        self.put_bytes(&b);
    }

    /// Writes a signed 128-bit integer as `(i64, u64)` in big-endian format (16 bytes).
    #[cfg(feature = "int128")]
    #[cfg_attr(docsrs, doc(cfg(feature = "int128")))]
    pub const fn write_i128(&mut self, n: i128) {
        self.write_i64((n >> 64) as i64);
        self.write_u64((n & 0xffffffffffffffff) as u64);
    }

    /// Writes an unsigned 128-bit integer as `(u64, u64)` in big-endian format (16 bytes).
    #[cfg(feature = "int128")]
    #[cfg_attr(docsrs, doc(cfg(feature = "int128")))]
    pub const fn write_u128(&mut self, n: u128) {
        self.write_u64((n >> 64) as u64);
        self.write_u64((n & 0xffffffffffffffff) as u64);
    }

    /// Writes a signed 16-bit integer, promoted to i32 (4 bytes).
    pub const fn write_i16(&mut self, n: i16) {
        self.write_i32(n as i32);
    }

    /// Writes an unsigned 16-bit integer, promoted to u32 (4 bytes).
    pub const fn write_u16(&mut self, n: u16) {
        self.write_u32(n as u32);
    }

    /// Writes a boolean as i32: 1 for true, 0 for false (4 bytes).
    pub const fn write_bool(&mut self, v: bool) {
        self.write_i32(if v { 1 } else { 0 });
    }

    /// Writes a 32-bit IEEE 754 float in big-endian format (4 bytes).
    pub const fn write_f32(&mut self, n: f32) {
        let b = n.to_be_bytes();
        self.put_bytes(&b);
    }

    /// Writes a 64-bit IEEE 754 double in big-endian format (8 bytes).
    pub const fn write_f64(&mut self, n: f64) {
        let b = n.to_be_bytes();
        self.put_bytes(&b);
    }

    // -- opaque writes --

    /// Writes fixed-length opaque bytes (raw bytes, padded to 4-byte boundary).
    #[doc(hidden)]
    pub const fn write_opaque_fixed(&mut self, bytes: &[u8]) {
        assert!(
            bytes.len() <= u32::MAX as usize,
            "opaque length exceeds u32::MAX"
        );
        self.put_bytes(bytes);
        let rem = bytes.len() % 4;
        if rem != 0 {
            let pad = 4 - rem;
            let mut i = 0;
            while i < pad {
                self.put_byte(0);
                i += 1;
            }
        }
    }

    /// Writes variable-length opaque data: 4-byte big-endian length prefix,
    /// then the data padded to a 4-byte boundary.
    pub const fn write_opaque_variable(&mut self, bytes: &[u8]) {
        assert!(
            bytes.len() <= u32::MAX as usize,
            "opaque length exceeds u32::MAX"
        );
        self.write_u32(bytes.len() as u32);
        self.write_opaque_fixed(bytes);
    }
}

// -- Size calculation helpers --
// These are free functions usable in const contexts to compute TXS encoded sizes.

/// Returns the TXS encoded size of a 32-bit integer (always 4).
pub const fn size_i32(_n: i32) -> usize {
    4
}

/// Returns the TXS encoded size of an unsigned 32-bit integer (always 4).
pub const fn size_u32(_n: u32) -> usize {
    4
}

/// Returns the TXS encoded size of a signed 64-bit integer (always 8).
pub const fn size_i64(_n: i64) -> usize {
    8
}

/// Returns the TXS encoded size of an unsigned 64-bit integer (always 8).
pub const fn size_u64(_n: u64) -> usize {
    8
}

/// Returns the TXS encoded size of a signed 128-bit integer (always 16).
#[cfg(feature = "int128")]
#[cfg_attr(docsrs, doc(cfg(feature = "int128")))]
pub const fn size_i128(_n: i128) -> usize {
    16
}

/// Returns the TXS encoded size of an unsigned 128-bit integer (always 16).
#[cfg(feature = "int128")]
#[cfg_attr(docsrs, doc(cfg(feature = "int128")))]
pub const fn size_u128(_n: u128) -> usize {
    16
}

/// Returns the TXS encoded size of a signed 16-bit integer (promoted to i32, always 4).
pub const fn size_i16(_n: i16) -> usize {
    4
}

/// Returns the TXS encoded size of an unsigned 16-bit integer (promoted to u32, always 4).
pub const fn size_u16(_n: u16) -> usize {
    4
}

/// Returns the TXS encoded size of a boolean (encoded as i32, always 4).
pub const fn size_bool(_v: bool) -> usize {
    4
}

/// Returns the TXS encoded size of a 32-bit float (always 4).
pub const fn size_f32(_n: f32) -> usize {
    4
}

/// Returns the TXS encoded size of a 64-bit double (always 8).
pub const fn size_f64(_n: f64) -> usize {
    8
}

/// Returns the TXS encoded size of fixed-length opaque data (padded to 4-byte boundary).
pub const fn size_opaque_fixed(len: usize) -> usize {
    assert!(len <= u32::MAX as usize, "opaque length exceeds u32::MAX");
    let rem = len % 4;
    if rem != 0 { len + 4 - rem } else { len }
}

/// Returns the TXS encoded size of variable-length opaque data
/// (4-byte length prefix + data padded to 4-byte boundary).
pub const fn size_opaque_variable(len: usize) -> usize {
    assert!(len <= u32::MAX as usize, "opaque length exceeds u32::MAX");
    4 + size_opaque_fixed(len)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENCODED: [u8; 8] = {
        let mut buf = [0u8; 8];
        let mut w = ConstWriter::new(&mut buf);
        w.write_i32(0);
        w.write_u32(42);
        assert!(w.pos() == 8);
        buf
    };

    #[test]
    fn const_encode_ints() {
        assert_eq!(ENCODED, [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x2a]);
    }

    const OPAQUE_FIXED: [u8; 4] = {
        let mut buf = [0u8; 4];
        let mut w = ConstWriter::new(&mut buf);
        w.write_opaque_fixed(&[0xAA, 0xBB, 0xCC]);
        assert!(w.pos() == 4);
        buf
    };

    #[test]
    fn const_encode_opaque_fixed() {
        assert_eq!(OPAQUE_FIXED, [0xAA, 0xBB, 0xCC, 0x00]);
    }

    const OPAQUE_VAR: [u8; 8] = {
        let mut buf = [0u8; 8];
        let mut w = ConstWriter::new(&mut buf);
        w.write_opaque_variable(b"hi");
        assert!(w.pos() == 8);
        buf
    };

    #[test]
    fn const_encode_opaque_variable() {
        assert_eq!(OPAQUE_VAR, [0x00, 0x00, 0x00, 0x02, 0x68, 0x69, 0x00, 0x00]);
    }

    const BOOL_TRUE: [u8; 4] = {
        let mut buf = [0u8; 4];
        let mut w = ConstWriter::new(&mut buf);
        w.write_bool(true);
        assert!(w.pos() == 4);
        buf
    };

    #[test]
    fn const_encode_bool() {
        assert_eq!(BOOL_TRUE, [0x00, 0x00, 0x00, 0x01]);
    }

    const AS_BYTES_LEN: usize = {
        let mut buf = [0u8; 8];
        let mut w = ConstWriter::new(&mut buf);
        w.write_u32(300);
        w.as_bytes().len()
    };

    #[test]
    fn as_bytes_partial_fill() {
        let mut buf = [0u8; 8];
        let mut w = ConstWriter::new(&mut buf);
        w.write_u32(300);
        assert_eq!(w.as_bytes(), &[0x00, 0x00, 0x01, 0x2c]);
        assert_eq!(AS_BYTES_LEN, 4);
    }

    #[test]
    fn as_bytes_empty_and_full() {
        let mut buf = [0u8; 4];
        let w = ConstWriter::new(&mut buf);
        assert_eq!(w.as_bytes(), &[] as &[u8]);
        let mut buf = [0u8; 4];
        let mut w = ConstWriter::new(&mut buf);
        w.write_bool(true);
        assert_eq!(w.as_bytes(), &[0x00, 0x00, 0x00, 0x01]);
    }
}
