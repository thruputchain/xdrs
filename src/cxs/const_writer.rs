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

    /// Writes fixed-length opaque data (raw bytes, no padding).
    pub const fn write_opaque_fixed(&mut self, bytes: &[u8]) {
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
        w.write_u32(4022250974); // 0xdeadbeef
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
}
