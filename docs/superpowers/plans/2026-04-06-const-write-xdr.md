# ConstWriteXDR Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enable compile-time XDR encoding via `#[derive(ConstWriteXDR)]` and two const-compatible writers (`cxs::ConstWriter<N>`, `txs::ConstWriter<N>`), so XDR-encoded data can be placed in statics and linker sections.

**Architecture:** Two new const writer structs (one per format) that panic on overflow instead of returning `Result`. A new derive macro generates inherent `const fn` methods on opted-in types, bypassing traits entirely. Each field is encoded via direct method calls on the writer (primitives) or direct method calls on the field's own generated const methods (nested types).

**Tech Stack:** Rust (no_std compatible), proc-macro2/quote/syn/darling (derive macro)

---

### Task 1: TXS ConstWriter

TXS is simpler (fixed-width big-endian, 4-byte padding), so we start here.

**Files:**
- Create: `src/txs/const_writer.rs`
- Modify: `src/txs/mod.rs`

- [ ] **Step 1: Create `src/txs/const_writer.rs` with the struct and `new`/`pos`/`into_bytes`**

```rust
/// A const-compatible writer for TXS (Traditional XDR Serialization).
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
        assert!(self.pos == N, "ConstWriter: bytes written does not equal buffer size N");
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
}
```

- [ ] **Step 2: Add integer write methods**

```rust
impl<const N: usize> ConstWriter<N> {
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
}
```

- [ ] **Step 3: Add opaque write methods**

```rust
impl<const N: usize> ConstWriter<N> {
    /// Writes fixed-length opaque data, padded to a 4-byte boundary.
    pub const fn write_opaque_fixed(&mut self, bytes: &[u8]) {
        self.put_bytes(bytes);
        // Pad to 4-byte boundary
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
        assert!(bytes.len() <= u32::MAX as usize, "opaque length exceeds u32::MAX");
        self.write_u32(bytes.len() as u32);
        self.write_opaque_fixed(bytes);
    }
}
```

- [ ] **Step 4: Wire up `src/txs/mod.rs`**

Add this line after the existing `mod put;` line:

```rust
pub mod const_writer;
```

- [ ] **Step 5: Write a compile-time test**

Create a test at the bottom of `src/txs/const_writer.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // Verify basic TXS const encoding at compile time
    const ENCODED: [u8; 8] = {
        let mut w = ConstWriter::<8>::new();
        w.write_i32(0);
        w.write_u32(42);
        w.into_bytes()
    };

    #[test]
    fn const_encode_ints() {
        // i32(0) = 00000000, u32(42) = 0000002a
        assert_eq!(ENCODED, [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x2a]);
    }

    const OPAQUE_FIXED: [u8; 4] = {
        let mut w = ConstWriter::<4>::new();
        w.write_opaque_fixed(&[0xAA, 0xBB, 0xCC]);
        w.into_bytes()
    };

    #[test]
    fn const_encode_opaque_fixed() {
        // 3 bytes + 1 byte padding
        assert_eq!(OPAQUE_FIXED, [0xAA, 0xBB, 0xCC, 0x00]);
    }

    const OPAQUE_VAR: [u8; 8] = {
        let mut w = ConstWriter::<8>::new();
        w.write_opaque_variable(b"hi");
        w.into_bytes()
    };

    #[test]
    fn const_encode_opaque_variable() {
        // length(2) as u32 = 00000002, "hi" = 6869, pad = 0000
        assert_eq!(OPAQUE_VAR, [0x00, 0x00, 0x00, 0x02, 0x68, 0x69, 0x00, 0x00]);
    }

    const BOOL_TRUE: [u8; 4] = {
        let mut w = ConstWriter::<4>::new();
        w.write_bool(true);
        w.into_bytes()
    };

    #[test]
    fn const_encode_bool() {
        assert_eq!(BOOL_TRUE, [0x00, 0x00, 0x00, 0x01]);
    }
}
```

- [ ] **Step 6: Run tests**

Run: `cargo test -p xdrs txs::const_writer`
Expected: All 4 tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/txs/const_writer.rs src/txs/mod.rs
git commit -m "feat: add txs::ConstWriter for compile-time TXS encoding"
```

---

### Task 2: CXS ConstWriter

CXS uses variable-width integer encoding with cascading size checks. The logic mirrors `src/cxs/put.rs` exactly.

**Files:**
- Create: `src/cxs/const_writer.rs`
- Modify: `src/cxs/mod.rs`

- [ ] **Step 1: Create `src/cxs/const_writer.rs` with struct, helpers, and opaque methods**

```rust
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
        assert!(self.pos == N, "ConstWriter: bytes written does not equal buffer size N");
        self.buf
    }

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

    /// Writes fixed-length opaque data (raw bytes, no padding).
    pub const fn write_opaque_fixed(&mut self, bytes: &[u8]) {
        self.put_bytes(bytes);
    }

    /// Writes variable-length opaque data: CXS-encoded u32 length prefix,
    /// then raw bytes.
    pub const fn write_opaque_variable(&mut self, bytes: &[u8]) {
        assert!(bytes.len() <= u32::MAX as usize, "opaque length exceeds u32::MAX");
        self.write_u32(bytes.len() as u32);
        self.put_bytes(bytes);
    }
}
```

- [ ] **Step 2: Add CXS integer encoding methods**

These mirror the cascading logic in `src/cxs/put.rs` exactly.

```rust
impl<const N: usize> ConstWriter<N> {
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
```

- [ ] **Step 3: Wire up `src/cxs/mod.rs`**

Add this line after the existing `mod put;` line:

```rust
pub mod const_writer;
```

- [ ] **Step 4: Write compile-time tests**

Add at the bottom of `src/cxs/const_writer.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // Small values fit in header byte
    const SMALL_INTS: [u8; 4] = {
        let mut w = ConstWriter::<4>::new();
        w.write_i32(0);   // 0x00
        w.write_i32(31);  // 0x1f
        w.write_i32(-1);  // 0x3e
        w.write_i32(1);   // 0x01
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
        w.write_i32(1);   // 0x01
        w.into_bytes()
    };

    #[test]
    fn const_encode_two_byte() {
        assert_eq!(TWO_BYTE, [0x3f, 0xe0, 0x01]);
    }

    // 4-byte encoding for values outside i16 range
    const FOUR_BYTE: [u8; 5] = {
        let mut w = ConstWriter::<5>::new();
        w.write_i32(i32::MAX); // 0xbf + le bytes
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
        assert_eq!(I64_MAX, [0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x7f]);
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

    // Bool
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
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p xdrs cxs::const_writer`
Expected: All tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/cxs/const_writer.rs src/cxs/mod.rs
git commit -m "feat: add cxs::ConstWriter for compile-time CXS encoding"
```

---

### Task 3: Cross-validate const writers against runtime encoders

Verify that the const writers produce identical output to the existing runtime `encode_into` for a range of values. This catches any transcription errors in the encoding logic.

**Files:**
- Create: `tests/const_writer.rs`

- [ ] **Step 1: Create `tests/const_writer.rs`**

```rust
//! Tests that const writers produce the same output as runtime encoders.

mod common;
use common::*;
use core::mem::MaybeUninit as MU;

/// Helper: runtime-encode a value with CXS, return the bytes.
fn cxs_runtime_encode<T: xdrs::WriteXDR>(val: &T) -> Vec<u8> {
    xdrs::cxs::encode(val).unwrap()
}

/// Helper: runtime-encode a value with TXS, return the bytes.
fn txs_runtime_encode<T: xdrs::WriteXDR>(val: &T) -> Vec<u8> {
    xdrs::txs::encode(val).unwrap()
}

// -- TXS cross-validation --

const TXS_I32_NEG1: [u8; 4] = {
    let mut w = xdrs::txs::ConstWriter::<4>::new();
    w.write_i32(-1);
    w.into_bytes()
};

const TXS_U64_DEADBEEF: [u8; 8] = {
    let mut w = xdrs::txs::ConstWriter::<8>::new();
    w.write_u64(0xdead1234beef5678);
    w.into_bytes()
};

const TXS_OPAQUE_FIXED_3: [u8; 4] = {
    let mut w = xdrs::txs::ConstWriter::<4>::new();
    w.write_opaque_fixed(&[0xAA, 0xBB, 0xCC]);
    w.into_bytes()
};

#[test]
fn txs_cross_validate_i32() {
    assert_eq!(&TXS_I32_NEG1, txs_runtime_encode(&-1i32).as_slice());
}

#[test]
fn txs_cross_validate_u64() {
    assert_eq!(&TXS_U64_DEADBEEF, txs_runtime_encode(&0xdead1234beef5678u64).as_slice());
}

#[test]
fn txs_cross_validate_opaque_fixed() {
    let bytes: [u8; 3] = [0xAA, 0xBB, 0xCC];
    assert_eq!(&TXS_OPAQUE_FIXED_3, txs_runtime_encode(&bytes).as_slice());
}

// -- CXS cross-validation --

const CXS_I32_0: [u8; 1] = {
    let mut w = xdrs::cxs::ConstWriter::<1>::new();
    w.write_i32(0);
    w.into_bytes()
};

const CXS_I32_MAX: [u8; 5] = {
    let mut w = xdrs::cxs::ConstWriter::<5>::new();
    w.write_i32(i32::MAX);
    w.into_bytes()
};

const CXS_U32_DEADBEEF: [u8; 5] = {
    let mut w = xdrs::cxs::ConstWriter::<5>::new();
    w.write_u32(0xdeadbeef);
    w.into_bytes()
};

const CXS_I64_MAX: [u8; 9] = {
    let mut w = xdrs::cxs::ConstWriter::<9>::new();
    w.write_i64(i64::MAX);
    w.into_bytes()
};

const CXS_U64_DB: [u8; 9] = {
    let mut w = xdrs::cxs::ConstWriter::<9>::new();
    w.write_u64(0xdead1234beef5678);
    w.into_bytes()
};

#[test]
fn cxs_cross_validate_i32_zero() {
    assert_eq!(&CXS_I32_0, cxs_runtime_encode(&0i32).as_slice());
}

#[test]
fn cxs_cross_validate_i32_max() {
    assert_eq!(&CXS_I32_MAX, cxs_runtime_encode(&i32::MAX).as_slice());
}

#[test]
fn cxs_cross_validate_u32_deadbeef() {
    assert_eq!(&CXS_U32_DEADBEEF, cxs_runtime_encode(&0xdeadbeefu32).as_slice());
}

#[test]
fn cxs_cross_validate_i64_max() {
    assert_eq!(&CXS_I64_MAX, cxs_runtime_encode(&i64::MAX).as_slice());
}

#[test]
fn cxs_cross_validate_u64_db() {
    assert_eq!(&CXS_U64_DB, cxs_runtime_encode(&0xdead1234beef5678u64).as_slice());
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test --all-features --test const_writer`
Expected: All tests pass — const writer output matches runtime encoder output byte-for-byte.

- [ ] **Step 3: Commit**

```bash
git add tests/const_writer.rs
git commit -m "test: cross-validate const writers against runtime encoders"
```

---

### Task 4: ConstWriteXDR derive macro — structs

Add the `ConstWriteXDR` derive macro to `xdr-derive`. Start with struct support only.

**Files:**
- Create: `xdr-derive/src/generate_const.rs`
- Modify: `xdr-derive/src/lib.rs`

- [ ] **Step 1: Create `xdr-derive/src/generate_const.rs`**

```rust
use crate::util::*;
use darling::*;
use proc_macro2::TokenStream;
use quote::quote;
use syn::Result;

#[derive(Clone, Debug, Default, FromDeriveInput, FromField)]
#[darling(attributes(xdrs), default)]
struct Attrs {
    pub into_type: Option<syn::Type>,
    pub via: Option<syn::Type>,

    #[darling(rename = "crate")]
    pub krate: Option<syn::Path>,
}

impl Attrs {
    fn crate_path(&self) -> syn::Path {
        self.krate
            .clone()
            .unwrap_or_else(|| syn::parse_quote!(::xdrs))
    }

    fn check_no_conversion<S: syn::spanned::Spanned>(&self, item: &S) -> Result<()> {
        if self.into_type.is_some() || self.via.is_some() {
            err(
                item,
                "ConstWriteXDR does not support `into_type` or `via` attributes, \
                 as the required conversions are not const-compatible.",
            )?;
        }
        Ok(())
    }
}

pub fn const_write_xdr(input: &syn::DeriveInput) -> Result<TokenStream> {
    let attrs = Attrs::from_derive_input(input)?;
    attrs.check_no_conversion(input)?;
    let krate = attrs.crate_path();

    let impls = const_write_data(input)?;

    Ok(quote! {
        const _: () = {
            use #krate::cxs;
            use #krate::txs;

            #impls
        };
    })
}

fn const_write_data(input: &syn::DeriveInput) -> Result<TokenStream> {
    if !input.generics.params.is_empty() {
        return err(
            input,
            "ConstWriteXDR does not support generic types.",
        );
    }

    match &input.data {
        syn::Data::Struct(item) => const_write_struct(input, item),
        syn::Data::Enum(item) => const_write_enum(input, item),
        _ => err(input, "ConstWriteXDR can only be derived for structs and enums"),
    }
}

fn const_write_struct(input: &syn::DeriveInput, item: &syn::DataStruct) -> Result<TokenStream> {
    let ty_ident = &input.ident;

    let cxs_fields = item
        .fields
        .iter()
        .enumerate()
        .map(|(i, f)| const_write_field(i, f, Format::Cxs))
        .collect::<Result<Vec<_>>>()?;

    let txs_fields = item
        .fields
        .iter()
        .enumerate()
        .map(|(i, f)| const_write_field(i, f, Format::Txs))
        .collect::<Result<Vec<_>>>()?;

    Ok(quote! {
        impl #ty_ident {
            pub const fn const_write_cxs<const N: usize>(&self, w: &mut cxs::ConstWriter<N>) {
                #(#cxs_fields)*
            }

            pub const fn const_write_txs<const N: usize>(&self, w: &mut txs::ConstWriter<N>) {
                #(#txs_fields)*
            }
        }
    })
}

#[derive(Copy, Clone)]
enum Format {
    Cxs,
    Txs,
}

fn const_write_field(index: usize, field: &syn::Field, fmt: Format) -> Result<TokenStream> {
    let attrs = Attrs::from_field(field)?;
    attrs.check_no_conversion(field)?;

    let fa = FieldAccess::new(index, field);
    let ty = &field.ty;

    Ok(const_write_field_expr(&fa, ty, fmt))
}

fn const_write_field_expr(fa: &FieldAccess<'_>, ty: &syn::Type, fmt: Format) -> TokenStream {
    let method = match fmt {
        Format::Cxs => quote! { const_write_cxs },
        Format::Txs => quote! { const_write_txs },
    };

    // Check if the type is a known primitive that maps to a writer method
    if let Some(writer_method) = primitive_write_method(ty) {
        return quote! { w.#writer_method(self.#fa); };
    }

    // Check for i128/u128 — inline shift-and-split
    if is_i128(ty) {
        return match fmt {
            Format::Cxs => quote! {
                w.write_i64((self.#fa >> 64) as i64);
                w.write_u64((self.#fa & 0xffffffffffffffff) as u64);
            },
            Format::Txs => quote! {
                w.write_i64((self.#fa >> 64) as i64);
                w.write_u64((self.#fa & 0xffffffffffffffff) as u64);
            },
        };
    }
    if is_u128(ty) {
        return match fmt {
            Format::Cxs => quote! {
                w.write_u64((self.#fa >> 64) as u64);
                w.write_u64((self.#fa & 0xffffffffffffffff) as u64);
            },
            Format::Txs => quote! {
                w.write_u64((self.#fa >> 64) as u64);
                w.write_u64((self.#fa & 0xffffffffffffffff) as u64);
            },
        };
    }

    // Check for [u8; N] — opaque fixed
    if is_u8_array(ty) {
        return quote! { w.write_opaque_fixed(&self.#fa); };
    }

    // Check for &[u8] — opaque variable
    if is_u8_slice_ref(ty) {
        return quote! { w.write_opaque_variable(self.#fa); };
    }

    // Check for &str — opaque variable via as_bytes
    if is_str_ref(ty) {
        return quote! { w.write_opaque_variable(self.#fa.as_bytes()); };
    }

    // Default: assume the field type also derives ConstWriteXDR
    quote! { self.#fa.#method(w); }
}

fn primitive_write_method(ty: &syn::Type) -> Option<TokenStream> {
    let path = match ty {
        syn::Type::Path(p) => p,
        _ => return None,
    };

    let ident = path.path.get_ident()?;
    let s = ident.to_string();

    match s.as_str() {
        "i32" => Some(quote! { write_i32 }),
        "u32" => Some(quote! { write_u32 }),
        "i64" => Some(quote! { write_i64 }),
        "u64" => Some(quote! { write_u64 }),
        "i16" => Some(quote! { write_i16 }),
        "u16" => Some(quote! { write_u16 }),
        "bool" => Some(quote! { write_bool }),
        _ => None,
    }
}

fn is_i128(ty: &syn::Type) -> bool {
    if let syn::Type::Path(p) = ty {
        if let Some(ident) = p.path.get_ident() {
            return ident == "i128";
        }
    }
    false
}

fn is_u128(ty: &syn::Type) -> bool {
    if let syn::Type::Path(p) = ty {
        if let Some(ident) = p.path.get_ident() {
            return ident == "u128";
        }
    }
    false
}

fn is_u8_array(ty: &syn::Type) -> bool {
    if let syn::Type::Array(arr) = ty {
        if let syn::Type::Path(p) = &*arr.elem {
            if let Some(ident) = p.path.get_ident() {
                return ident == "u8";
            }
        }
    }
    false
}

fn is_u8_slice_ref(ty: &syn::Type) -> bool {
    if let syn::Type::Reference(r) = ty {
        if let syn::Type::Slice(s) = &*r.elem {
            if let syn::Type::Path(p) = &*s.elem {
                if let Some(ident) = p.path.get_ident() {
                    return ident == "u8";
                }
            }
        }
    }
    false
}

fn is_str_ref(ty: &syn::Type) -> bool {
    if let syn::Type::Reference(r) = ty {
        if let syn::Type::Path(p) = &*r.elem {
            if let Some(ident) = p.path.get_ident() {
                return ident == "str";
            }
        }
    }
    false
}

// Enum support — placeholder for Task 5
fn const_write_enum(_input: &syn::DeriveInput, _item: &syn::DataEnum) -> Result<TokenStream> {
    todo!("enum support added in Task 5")
}
```

- [ ] **Step 2: Add the derive entry point to `xdr-derive/src/lib.rs`**

Add at the top of the file, after `mod generate;`:

```rust
mod generate_const;
```

Add at the bottom, after the `derive_xdr` function:

```rust
/// Derives const-compatible XDR encoding methods for compile-time use.
///
/// Generates two inherent `const fn` methods on the type:
/// - `const_write_cxs<const N: usize>(&self, w: &mut cxs::ConstWriter<N>)`
/// - `const_write_txs<const N: usize>(&self, w: &mut txs::ConstWriter<N>)`
///
/// These methods can be used in `const` blocks to encode values at compile time.
///
/// # Limitations
///
/// - Does not support `#[xdrs(into_type)]` or `#[xdrs(via)]` attributes
/// - Does not support generic type parameters
/// - Field types must be primitives, byte arrays, or other types that derive `ConstWriteXDR`
///
#[proc_macro_derive(ConstWriteXDR, attributes(xdrs))]
pub fn derive_const_write_xdr(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);

    generate_const::const_write_xdr(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}
```

- [ ] **Step 3: Re-export the derive macro from `src/lib.rs`**

The existing line `pub use xdr_derive::*;` (line 110 of `src/lib.rs`, behind `#[cfg(feature = "derive")]`) already re-exports everything from `xdr_derive`, so `ConstWriteXDR` will be available as `xdrs::ConstWriteXDR` automatically. No change needed.

Verify this is the case by checking line 110:
```rust
pub use xdr_derive::*;
```

- [ ] **Step 4: Test the derive macro on a struct**

Add to `tests/const_writer.rs`:

```rust
// -- Derive macro tests --

#[derive(xdrs::ConstWriteXDR, xdrs::WriteXDR)]
struct Foo {
    bytes: [u8; 3],
    blerg: i32,
}

const FOO: Foo = Foo {
    bytes: [0xAA, 0xBB, 0xCC],
    blerg: 7,
};

const FOO_CXS: [u8; 4] = {
    let mut w = xdrs::cxs::ConstWriter::<4>::new();
    FOO.const_write_cxs(&mut w);
    w.into_bytes()
};

const FOO_TXS: [u8; 8] = {
    let mut w = xdrs::txs::ConstWriter::<8>::new();
    FOO.const_write_txs(&mut w);
    w.into_bytes()
};

#[test]
fn derive_struct_cxs() {
    // Same encoding as tests/structs.rs::struct_foo
    assert_eq!(&FOO_CXS, hex!("aabbcc07").as_slice());
    assert_eq!(&FOO_CXS, cxs_runtime_encode(&FOO).as_slice());
}

#[test]
fn derive_struct_txs() {
    assert_eq!(&FOO_TXS, hex!("aabbcc0000000007").as_slice());
    assert_eq!(&FOO_TXS, txs_runtime_encode(&FOO).as_slice());
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test --all-features --test const_writer`
Expected: All tests pass, including the new derive tests.

- [ ] **Step 6: Commit**

```bash
git add xdr-derive/src/generate_const.rs xdr-derive/src/lib.rs tests/const_writer.rs
git commit -m "feat: add ConstWriteXDR derive macro for structs"
```

---

### Task 5: ConstWriteXDR derive macro — enums

Add enum support to the `ConstWriteXDR` derive macro.

**Files:**
- Modify: `xdr-derive/src/generate_const.rs`
- Modify: `tests/const_writer.rs`

- [ ] **Step 1: Replace the `const_write_enum` placeholder in `generate_const.rs`**

Replace the `todo!()` placeholder with the full implementation:

```rust
fn const_write_enum(input: &syn::DeriveInput, item: &syn::DataEnum) -> Result<TokenStream> {
    check_enum_repr(input)?;
    check_variants(input, item)?;

    let ty_ident = &input.ident;

    let cxs_arms = item
        .variants
        .iter()
        .enumerate()
        .map(|(i, v)| const_write_variant(i, v, input, Format::Cxs))
        .collect::<Vec<_>>();

    let txs_arms = item
        .variants
        .iter()
        .enumerate()
        .map(|(i, v)| const_write_variant(i, v, input, Format::Txs))
        .collect::<Vec<_>>();

    Ok(quote! {
        impl #ty_ident {
            pub const fn const_write_cxs<const N: usize>(&self, w: &mut cxs::ConstWriter<N>) {
                match self {
                    #(#cxs_arms)*
                }
            }

            pub const fn const_write_txs<const N: usize>(&self, w: &mut txs::ConstWriter<N>) {
                match self {
                    #(#txs_arms)*
                }
            }
        }
    })
}

fn const_write_variant(
    index: usize,
    v: &syn::Variant,
    input: &syn::DeriveInput,
    fmt: Format,
) -> TokenStream {
    let is_default_arm = variant_is_default(v);
    let var_ident = &v.ident;
    let ty_ident = &input.ident;
    let var_bindings = const_var_bindings(&v.fields);

    let variant_idx = index as i32;
    let discriminant = if let Some((_, d)) = v.discriminant.as_ref() {
        quote! { #d }
    } else if is_default_arm {
        quote! { *_self_0 }
    } else {
        quote! { #variant_idx }
    };

    let write_discrim = quote! { w.write_i32(#discriminant); };

    let write_fields = if is_default_arm {
        quote! {}
    } else {
        let field_writes: Vec<_> = v
            .fields
            .iter()
            .enumerate()
            .map(|(i, field)| {
                let bind = tmp_binding(i);
                let ty = &field.ty;
                const_write_binding_expr(&bind, ty, fmt)
            })
            .collect();
        quote! { #(#field_writes)* }
    };

    quote! {
        #ty_ident::#var_ident #var_bindings => {
            #write_discrim
            #write_fields
        }
    }
}

fn const_write_binding_expr(
    bind: &syn::Ident,
    ty: &syn::Type,
    fmt: Format,
) -> TokenStream {
    let method = match fmt {
        Format::Cxs => quote! { const_write_cxs },
        Format::Txs => quote! { const_write_txs },
    };

    if let Some(writer_method) = primitive_write_method(ty) {
        return quote! { w.#writer_method(*#bind); };
    }

    if is_i128(ty) {
        return quote! {
            w.write_i64((*#bind >> 64) as i64);
            w.write_u64((*#bind & 0xffffffffffffffff) as u64);
        };
    }
    if is_u128(ty) {
        return quote! {
            w.write_u64((*#bind >> 64) as u64);
            w.write_u64((*#bind & 0xffffffffffffffff) as u64);
        };
    }

    if is_u8_array(ty) {
        return quote! { w.write_opaque_fixed(#bind); };
    }

    if is_u8_slice_ref(ty) {
        return quote! { w.write_opaque_variable(#bind); };
    }

    if is_str_ref(ty) {
        return quote! { w.write_opaque_variable(#bind.as_bytes()); };
    }

    quote! { #bind.#method(w); }
}

fn const_var_bindings(fields: &syn::Fields) -> TokenStream {
    match fields {
        syn::Fields::Named(fields) => {
            let bindings = fields.named.iter().enumerate().map(|(i, field)| {
                let bind = tmp_binding(i);
                let ident = field.ident.as_ref().unwrap();
                quote! { #ident: #bind, }
            });
            quote! { { #(#bindings)* } }
        }
        syn::Fields::Unnamed(fields) => {
            let bindings = fields.unnamed.iter().enumerate().map(|(i, _)| {
                let bind = tmp_binding(i);
                quote! { #bind, }
            });
            quote! { ( #(#bindings)* ) }
        }
        syn::Fields::Unit => quote! {},
    }
}
```

- [ ] **Step 2: Add enum tests to `tests/const_writer.rs`**

```rust
#[derive(xdrs::ConstWriteXDR, xdrs::WriteXDR)]
#[repr(i32)]
enum Status {
    On,
    Off,
}

const STATUS_ON_CXS: [u8; 1] = {
    let mut w = xdrs::cxs::ConstWriter::<1>::new();
    Status::On.const_write_cxs(&mut w);
    w.into_bytes()
};

const STATUS_ON_TXS: [u8; 4] = {
    let mut w = xdrs::txs::ConstWriter::<4>::new();
    Status::On.const_write_txs(&mut w);
    w.into_bytes()
};

const STATUS_OFF_CXS: [u8; 1] = {
    let mut w = xdrs::cxs::ConstWriter::<1>::new();
    Status::Off.const_write_cxs(&mut w);
    w.into_bytes()
};

#[test]
fn derive_enum_bare() {
    assert_eq!(&STATUS_ON_CXS, hex!("00").as_slice());
    assert_eq!(&STATUS_ON_TXS, hex!("00000000").as_slice());
    assert_eq!(&STATUS_OFF_CXS, hex!("01").as_slice());
    assert_eq!(&STATUS_ON_CXS, cxs_runtime_encode(&Status::On).as_slice());
    assert_eq!(&STATUS_ON_TXS, txs_runtime_encode(&Status::On).as_slice());
}

// Enum with fields
#[derive(xdrs::ConstWriteXDR, xdrs::WriteXDR)]
#[repr(i32)]
enum Shape {
    Circle { radius: u32 } = 0,
    Rect { w: u32, h: u32 } = 1,
    Empty = 2,
}

const CIRCLE_CXS: [u8; 2] = {
    let mut w = xdrs::cxs::ConstWriter::<2>::new();
    (Shape::Circle { radius: 5 }).const_write_cxs(&mut w);
    w.into_bytes()
};

const RECT_TXS: [u8; 12] = {
    let mut w = xdrs::txs::ConstWriter::<12>::new();
    (Shape::Rect { w: 10, h: 20 }).const_write_txs(&mut w);
    w.into_bytes()
};

#[test]
fn derive_enum_with_fields() {
    let circle = Shape::Circle { radius: 5 };
    assert_eq!(&CIRCLE_CXS, cxs_runtime_encode(&circle).as_slice());

    let rect = Shape::Rect { w: 10, h: 20 };
    assert_eq!(&RECT_TXS, txs_runtime_encode(&rect).as_slice());
}

// Nested: struct containing enum
#[derive(xdrs::ConstWriteXDR, xdrs::WriteXDR)]
struct Config {
    version: u32,
    status: Status,
}

const CONFIG: Config = Config {
    version: 1,
    status: Status::On,
};

const CONFIG_CXS: [u8; 2] = {
    let mut w = xdrs::cxs::ConstWriter::<2>::new();
    CONFIG.const_write_cxs(&mut w);
    w.into_bytes()
};

const CONFIG_TXS: [u8; 8] = {
    let mut w = xdrs::txs::ConstWriter::<8>::new();
    CONFIG.const_write_txs(&mut w);
    w.into_bytes()
};

#[test]
fn derive_nested_struct_enum() {
    assert_eq!(&CONFIG_CXS, cxs_runtime_encode(&CONFIG).as_slice());
    assert_eq!(&CONFIG_TXS, txs_runtime_encode(&CONFIG).as_slice());
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test --all-features --test const_writer`
Expected: All tests pass.

- [ ] **Step 4: Commit**

```bash
git add xdr-derive/src/generate_const.rs tests/const_writer.rs
git commit -m "feat: add enum support to ConstWriteXDR derive macro"
```

---

### Task 6: Add `const_writer` test to Cargo.toml and run full CI

The new test file needs `alloc` and `derive` features. Add a test target entry.

**Files:**
- Modify: `Cargo.toml` (root)

- [ ] **Step 1: Add the test target to `Cargo.toml`**

Add after the existing `[[test]]` entries (after the `pointer` test block around line 100):

```toml
[[test]]
name = "const_writer"
required-features = ["alloc", "derive"]
```

- [ ] **Step 2: Run the full test suite**

Run: `cargo test --workspace --all-features`
Expected: All tests pass, including pre-existing tests (no regressions).

- [ ] **Step 3: Run CI script**

Run: `bash ci.sh`
Expected: Clean pass.

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml
git commit -m "chore: add const_writer test target to Cargo.toml"
```
