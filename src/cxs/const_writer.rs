//! Const-compatible CXS encoding.
//!
//! This module provides [`ConstWriter`], which encodes CXS data into a
//! fixed-size buffer in `const` contexts, along with `size_*` functions
//! that compute the encoded size of each primitive type.

// CXS encoding constants (mirrors cxs::limits and cxs::put)
const V_MULTIBYTE: u8 = 63;
const S_8BYTE: u8 = 3 << 6;
const S_4BYTE: u8 = 2 << 6;
const S_2BYTE: u8 = 1 << 6;
const S_1BYTE: u8 = 0 << 6;
const S_HEADER: u8 = 0 << 6;

const IH_MAX: i8 = 31;
const IH_MIN: i8 = -31;
const UH_MAX: u8 = 62;

/// A slice-backed, infallible CXS cursor.
///
/// The canonical const-compatible CXS writer: writes into a caller-provided
/// byte slice, panicking on overflow (a compile-time error in `const`
/// contexts). [`ConstWriter`] delegates to this type; the CXS width rules
/// live here exactly once.
pub struct Cursor<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    /// Creates a cursor writing into `buf`, starting at position 0.
    pub const fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    /// Returns the number of bytes written so far.
    pub const fn pos(&self) -> usize {
        self.pos
    }

    /// Returns the bytes written so far (the filled prefix of the buffer).
    pub const fn as_bytes(&self) -> &[u8] {
        self.buf.split_at(self.pos).0
    }

    // -- internal helpers --

    const fn put_byte(&mut self, b: u8) {
        self.buf[self.pos] = b;
        self.pos += 1;
    }

    const fn put_bytes(&mut self, bytes: &[u8]) {
        let mut i = 0;
        while i < bytes.len() {
            self.buf[self.pos + i] = bytes[i];
            i += 1;
        }
        self.pos += bytes.len();
    }

    // -- opaque writes --

    #[doc(hidden)]
    pub const fn write_opaque_fixed(&mut self, bytes: &[u8]) {
        assert!(
            bytes.len() <= u32::MAX as usize,
            "opaque length exceeds u32::MAX"
        );
        self.put_bytes(bytes);
    }

    /// Writes variable-length opaque data: CXS-encoded u32 length prefix,
    /// then raw bytes.
    pub const fn write_opaque_variable(&mut self, bytes: &[u8]) {
        assert!(
            bytes.len() <= u32::MAX as usize,
            "opaque length exceeds u32::MAX"
        );
        self.write_u32(bytes.len() as u32);
        self.put_bytes(bytes);
    }

    // -- integer writes --

    /// Writes a boolean as CXS-encoded i32 (0 or 1).
    pub const fn write_bool(&mut self, v: bool) {
        self.write_i32(if v { 1 } else { 0 });
    }

    /// Writes a 32-bit IEEE 754 float as raw little-endian bytes (4 bytes).
    ///
    /// Unlike integers, floats are not compressed with CXS variable-width
    /// encoding — the bit pattern is written as-is. Little-endian byte order
    /// follows CXS convention (not RFC 4506 big-endian).
    pub const fn write_f32(&mut self, n: f32) {
        let b = n.to_le_bytes();
        self.put_bytes(&b);
    }

    /// Writes a 64-bit IEEE 754 double as raw little-endian bytes (8 bytes).
    ///
    /// Unlike integers, floats are not compressed with CXS variable-width
    /// encoding — the bit pattern is written as-is. Little-endian byte order
    /// follows CXS convention (not RFC 4506 big-endian).
    pub const fn write_f64(&mut self, n: f64) {
        let b = n.to_le_bytes();
        self.put_bytes(&b);
    }

    /// Writes a signed 64-bit integer using CXS variable-width encoding.
    pub const fn write_i64(&mut self, n: i64) {
        if n > i32::MAX as i64 || n < (i32::MIN as i64) {
            let h = V_MULTIBYTE | S_8BYTE;
            let b = n.to_le_bytes();
            let combined = [h, b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]];
            self.put_bytes(&combined);
        } else {
            self.write_i32(n as i32);
        }
    }

    /// Writes a signed 32-bit integer using CXS variable-width encoding.
    pub const fn write_i32(&mut self, n: i32) {
        if n > i16::MAX as i32 || n < (i16::MIN as i32) {
            let h = V_MULTIBYTE | S_4BYTE;
            let b = n.to_le_bytes();
            let combined = [h, b[0], b[1], b[2], b[3]];
            self.put_bytes(&combined);
        } else {
            self.write_i16(n as i16);
        }
    }

    /// Writes a signed 16-bit integer using CXS variable-width encoding.
    pub const fn write_i16(&mut self, n: i16) {
        if n > i8::MAX as i16 || n < (i8::MIN as i16) {
            let h = V_MULTIBYTE | S_2BYTE;
            let b = n.to_le_bytes();
            let combined = [h, b[0], b[1]];
            self.put_bytes(&combined);
        } else {
            self.const_write_i8(n as i8);
        }
    }

    const fn const_write_i8(&mut self, n: i8) {
        if n < IH_MIN || n > IH_MAX {
            let h = V_MULTIBYTE | S_1BYTE;
            let combined = [h, n as u8];
            self.put_bytes(&combined);
        } else {
            // 6-bit one's complement in header byte
            let nu: u8 = n.unsigned_abs();
            let v = if n >= 0 {
                nu
            } else {
                (!nu & 0b01_1111) | 0b0010_0000
            };
            let h = v | S_HEADER;
            self.put_byte(h);
        }
    }

    /// Writes a signed 128-bit integer as `(i64, u64)` using CXS variable-width encoding.
    #[cfg(feature = "int128")]
    #[cfg_attr(docsrs, doc(cfg(feature = "int128")))]
    pub const fn write_i128(&mut self, n: i128) {
        self.write_i64((n >> 64) as i64);
        self.write_u64((n & 0xffffffffffffffff) as u64);
    }

    /// Writes an unsigned 128-bit integer as `(u64, u64)` using CXS variable-width encoding.
    #[cfg(feature = "int128")]
    #[cfg_attr(docsrs, doc(cfg(feature = "int128")))]
    pub const fn write_u128(&mut self, n: u128) {
        self.write_u64((n >> 64) as u64);
        self.write_u64((n & 0xffffffffffffffff) as u64);
    }

    /// Writes an unsigned 64-bit integer using CXS variable-width encoding.
    pub const fn write_u64(&mut self, n: u64) {
        if n > u32::MAX as u64 {
            let h = V_MULTIBYTE | S_8BYTE;
            let b = n.to_le_bytes();
            let combined = [h, b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]];
            self.put_bytes(&combined);
        } else {
            self.write_u32(n as u32);
        }
    }

    /// Writes an unsigned 32-bit integer using CXS variable-width encoding.
    pub const fn write_u32(&mut self, n: u32) {
        if n > u16::MAX as u32 {
            let h = V_MULTIBYTE | S_4BYTE;
            let b = n.to_le_bytes();
            let combined = [h, b[0], b[1], b[2], b[3]];
            self.put_bytes(&combined);
        } else {
            self.write_u16(n as u16);
        }
    }

    /// Writes an unsigned 16-bit integer using CXS variable-width encoding.
    pub const fn write_u16(&mut self, n: u16) {
        if n > u8::MAX as u16 {
            let h = V_MULTIBYTE | S_2BYTE;
            let b = n.to_le_bytes();
            let combined = [h, b[0], b[1]];
            self.put_bytes(&combined);
        } else {
            self.const_write_u8(n as u8);
        }
    }

    const fn const_write_u8(&mut self, n: u8) {
        if n > UH_MAX {
            let h = V_MULTIBYTE | S_1BYTE;
            let combined = [h, n];
            self.put_bytes(&combined);
        } else {
            self.put_byte(n);
        }
    }
}

/// A const-compatible writer for CXS (Compact XDR Serialization).
///
/// All methods panic on buffer overflow. In a `const` context, this
/// produces a compile-time error.
///
/// Use [`into_bytes`](Self::into_bytes) to extract the final buffer,
/// which panics if the writer has not filled the buffer exactly.
pub struct ConstWriter<const N: usize> {
    buf: [u8; N],
    pos: usize,
}

impl<const N: usize> Default for ConstWriter<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> ConstWriter<N> {
    /// Creates a new writer with a zeroed buffer.
    pub const fn new() -> Self {
        Self {
            buf: [0u8; N],
            pos: 0,
        }
    }

    /// Returns the number of bytes written so far.
    pub const fn pos(&self) -> usize {
        self.pos
    }

    /// Consumes the writer and returns the buffer.
    ///
    /// # Panics
    ///
    /// Panics if the number of bytes written does not equal `N`.
    pub const fn into_bytes(self) -> [u8; N] {
        assert!(
            self.pos == N,
            "ConstWriter: bytes written does not equal buffer size N"
        );
        self.buf
    }

    /// Returns the bytes written so far (the filled prefix of the buffer).
    ///
    /// Unlike [`into_bytes`](Self::into_bytes), this does not require the
    /// buffer to be exactly full, so it supports value-dependent encoded
    /// lengths that fall short of `N`.
    pub const fn as_bytes(&self) -> &[u8] {
        self.buf.split_at(self.pos).0
    }

    // -- delegating write methods --

    #[doc(hidden)]
    /// Writes fixed-length opaque bytes (raw bytes, no length prefix).
    pub const fn write_opaque_fixed(&mut self, bytes: &[u8]) {
        let mut c = Cursor { buf: &mut self.buf, pos: self.pos };
        c.write_opaque_fixed(bytes);
        self.pos = c.pos;
    }

    /// Writes variable-length opaque data: CXS-encoded u32 length prefix,
    /// then raw bytes.
    pub const fn write_opaque_variable(&mut self, bytes: &[u8]) {
        let mut c = Cursor { buf: &mut self.buf, pos: self.pos };
        c.write_opaque_variable(bytes);
        self.pos = c.pos;
    }

    /// Writes a boolean as CXS-encoded i32 (0 or 1).
    pub const fn write_bool(&mut self, v: bool) {
        let mut c = Cursor { buf: &mut self.buf, pos: self.pos };
        c.write_bool(v);
        self.pos = c.pos;
    }

    /// Writes a 32-bit IEEE 754 float as raw little-endian bytes (4 bytes).
    ///
    /// Unlike integers, floats are not compressed with CXS variable-width
    /// encoding — the bit pattern is written as-is. Little-endian byte order
    /// follows CXS convention (not RFC 4506 big-endian).
    pub const fn write_f32(&mut self, n: f32) {
        let mut c = Cursor { buf: &mut self.buf, pos: self.pos };
        c.write_f32(n);
        self.pos = c.pos;
    }

    /// Writes a 64-bit IEEE 754 double as raw little-endian bytes (8 bytes).
    ///
    /// Unlike integers, floats are not compressed with CXS variable-width
    /// encoding — the bit pattern is written as-is. Little-endian byte order
    /// follows CXS convention (not RFC 4506 big-endian).
    pub const fn write_f64(&mut self, n: f64) {
        let mut c = Cursor { buf: &mut self.buf, pos: self.pos };
        c.write_f64(n);
        self.pos = c.pos;
    }

    /// Writes a signed 64-bit integer using CXS variable-width encoding.
    pub const fn write_i64(&mut self, n: i64) {
        let mut c = Cursor { buf: &mut self.buf, pos: self.pos };
        c.write_i64(n);
        self.pos = c.pos;
    }

    /// Writes a signed 32-bit integer using CXS variable-width encoding.
    pub const fn write_i32(&mut self, n: i32) {
        let mut c = Cursor { buf: &mut self.buf, pos: self.pos };
        c.write_i32(n);
        self.pos = c.pos;
    }

    /// Writes a signed 16-bit integer using CXS variable-width encoding.
    pub const fn write_i16(&mut self, n: i16) {
        let mut c = Cursor { buf: &mut self.buf, pos: self.pos };
        c.write_i16(n);
        self.pos = c.pos;
    }

    /// Writes a signed 128-bit integer as `(i64, u64)` using CXS variable-width encoding.
    #[cfg(feature = "int128")]
    #[cfg_attr(docsrs, doc(cfg(feature = "int128")))]
    pub const fn write_i128(&mut self, n: i128) {
        let mut c = Cursor { buf: &mut self.buf, pos: self.pos };
        c.write_i128(n);
        self.pos = c.pos;
    }

    /// Writes an unsigned 128-bit integer as `(u64, u64)` using CXS variable-width encoding.
    #[cfg(feature = "int128")]
    #[cfg_attr(docsrs, doc(cfg(feature = "int128")))]
    pub const fn write_u128(&mut self, n: u128) {
        let mut c = Cursor { buf: &mut self.buf, pos: self.pos };
        c.write_u128(n);
        self.pos = c.pos;
    }

    /// Writes an unsigned 64-bit integer using CXS variable-width encoding.
    pub const fn write_u64(&mut self, n: u64) {
        let mut c = Cursor { buf: &mut self.buf, pos: self.pos };
        c.write_u64(n);
        self.pos = c.pos;
    }

    /// Writes an unsigned 32-bit integer using CXS variable-width encoding.
    pub const fn write_u32(&mut self, n: u32) {
        let mut c = Cursor { buf: &mut self.buf, pos: self.pos };
        c.write_u32(n);
        self.pos = c.pos;
    }

    /// Writes an unsigned 16-bit integer using CXS variable-width encoding.
    pub const fn write_u16(&mut self, n: u16) {
        let mut c = Cursor { buf: &mut self.buf, pos: self.pos };
        c.write_u16(n);
        self.pos = c.pos;
    }
}

// -- Size calculation helpers --
// These are free functions usable in const contexts to compute CXS encoded sizes.
// Sizes are value-dependent due to variable-width encoding.

/// Returns the CXS encoded size of a boolean (encoded as i32).
pub const fn size_bool(v: bool) -> usize {
    size_i32(if v { 1 } else { 0 })
}

/// Returns the CXS encoded size of a 32-bit float (always 4).
pub const fn size_f32(_n: f32) -> usize {
    4
}

/// Returns the CXS encoded size of a 64-bit double (always 8).
pub const fn size_f64(_n: f64) -> usize {
    8
}

/// Returns the CXS encoded size of a signed 64-bit integer.
pub const fn size_i64(n: i64) -> usize {
    if n > i32::MAX as i64 || n < (i32::MIN as i64) {
        9 // 1 header + 8 data
    } else {
        size_i32(n as i32)
    }
}

/// Returns the CXS encoded size of a signed 32-bit integer.
pub const fn size_i32(n: i32) -> usize {
    if n > i16::MAX as i32 || n < (i16::MIN as i32) {
        5 // 1 header + 4 data
    } else {
        size_i16(n as i16)
    }
}

/// Returns the CXS encoded size of a signed 16-bit integer.
pub const fn size_i16(n: i16) -> usize {
    if n > i8::MAX as i16 || n < (i8::MIN as i16) {
        3 // 1 header + 2 data
    } else {
        size_i8(n as i8)
    }
}

const fn size_i8(n: i8) -> usize {
    if n < IH_MIN || n > IH_MAX {
        2 // 1 header + 1 data
    } else {
        1 // header only
    }
}

/// Returns the CXS encoded size of a signed 128-bit integer.
#[cfg(feature = "int128")]
#[cfg_attr(docsrs, doc(cfg(feature = "int128")))]
pub const fn size_i128(n: i128) -> usize {
    size_i64((n >> 64) as i64) + size_u64((n & 0xffffffffffffffff) as u64)
}

/// Returns the CXS encoded size of an unsigned 128-bit integer.
#[cfg(feature = "int128")]
#[cfg_attr(docsrs, doc(cfg(feature = "int128")))]
pub const fn size_u128(n: u128) -> usize {
    size_u64((n >> 64) as u64) + size_u64((n & 0xffffffffffffffff) as u64)
}

/// Returns the CXS encoded size of an unsigned 64-bit integer.
pub const fn size_u64(n: u64) -> usize {
    if n > u32::MAX as u64 {
        9
    } else {
        size_u32(n as u32)
    }
}

/// Returns the CXS encoded size of an unsigned 32-bit integer.
pub const fn size_u32(n: u32) -> usize {
    if n > u16::MAX as u32 {
        5
    } else {
        size_u16(n as u16)
    }
}

/// Returns the CXS encoded size of an unsigned 16-bit integer.
pub const fn size_u16(n: u16) -> usize {
    if n > u8::MAX as u16 {
        3
    } else {
        size_u8(n as u8)
    }
}

const fn size_u8(n: u8) -> usize {
    if n > UH_MAX { 2 } else { 1 }
}

/// Returns the CXS encoded size of fixed-length opaque data (raw bytes, no padding).
pub const fn size_opaque_fixed(len: usize) -> usize {
    assert!(len <= u32::MAX as usize, "opaque length exceeds u32::MAX");
    len
}

/// Returns the CXS encoded size of variable-length opaque data
/// (CXS-encoded u32 length prefix + raw bytes).
pub const fn size_opaque_variable(len: usize) -> usize {
    assert!(len <= u32::MAX as usize, "opaque length exceeds u32::MAX");
    size_u32(len as u32) + len
}

#[cfg(test)]
mod tests {
    use super::*;

    // Small values fit in header byte
    const SMALL_INTS: [u8; 4] = {
        let mut w = ConstWriter::<4>::new();
        w.write_i32(0); // 0x00
        w.write_i32(31); // 0x1f
        w.write_i32(-1); // 0x3e
        w.write_i32(1); // 0x01
        w.into_bytes()
    };

    #[test]
    fn const_encode_small_ints() {
        assert_eq!(SMALL_INTS, [0x00, 0x1f, 0x3e, 0x01]);
    }

    // Values outside i8 range use 2-byte encoding
    const TWO_BYTE: [u8; 3] = {
        let mut w = ConstWriter::<3>::new();
        w.write_i32(-32); // 0x3f 0xe0
        w.write_i32(1); // 0x01
        w.into_bytes()
    };

    #[test]
    fn const_encode_two_byte() {
        assert_eq!(TWO_BYTE, [0x3f, 0xe0, 0x01]);
    }

    // 4-byte encoding for values outside i16 range
    const FOUR_BYTE: [u8; 5] = {
        let mut w = ConstWriter::<5>::new();
        w.write_i32(i32::MAX);
        w.into_bytes()
    };

    #[test]
    fn const_encode_four_byte() {
        assert_eq!(FOUR_BYTE, [0xbf, 0xff, 0xff, 0xff, 0x7f]);
    }

    // Unsigned: u32 deadbeef
    const U32_DB: [u8; 5] = {
        let mut w = ConstWriter::<5>::new();
        w.write_u32(4022250974); // 0xEFBEADDE (LE bytes spell out 0xdeadbeef)
        w.into_bytes()
    };

    #[test]
    fn const_encode_u32_deadbeef() {
        assert_eq!(U32_DB, [0xbf, 0xde, 0xad, 0xbe, 0xef]);
    }

    // 8-byte encoding for i64
    const I64_MAX: [u8; 9] = {
        let mut w = ConstWriter::<9>::new();
        w.write_i64(i64::MAX);
        w.into_bytes()
    };

    #[test]
    fn const_encode_i64_max() {
        assert_eq!(
            I64_MAX,
            [0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x7f]
        );
    }

    // Opaque variable: CXS encodes length 2 as single byte 0x02, then "hi"
    const OPAQUE: [u8; 3] = {
        let mut w = ConstWriter::<3>::new();
        w.write_opaque_variable(b"hi");
        w.into_bytes()
    };

    #[test]
    fn const_encode_opaque_variable() {
        assert_eq!(OPAQUE, [0x02, 0x68, 0x69]);
    }

    const BOOL_TRUE: [u8; 1] = {
        let mut w = ConstWriter::<1>::new();
        w.write_bool(true);
        w.into_bytes()
    };

    #[test]
    fn const_encode_bool() {
        assert_eq!(BOOL_TRUE, [0x01]);
    }

    // `write_u32(300)`: 300 > u8::MAX → 2-byte encoding: header 0x7f, LE bytes [0x2c, 0x01]
    const AS_BYTES_LEN: usize = {
        let mut w = ConstWriter::<8>::new();
        w.write_u32(300);
        w.as_bytes().len()
    };

    #[test]
    fn as_bytes_partial_fill() {
        let mut w = ConstWriter::<8>::new();
        w.write_u32(300);
        assert_eq!(w.as_bytes(), &[0x7f, 0x2c, 0x01]);
        assert_eq!(AS_BYTES_LEN, 3);
    }

    #[test]
    fn as_bytes_empty_and_full() {
        let w = ConstWriter::<4>::new();
        assert_eq!(w.as_bytes(), &[] as &[u8]);
        let mut w = ConstWriter::<1>::new();
        w.write_bool(true);
        assert_eq!(w.as_bytes(), &[0x01]);
    }
}
