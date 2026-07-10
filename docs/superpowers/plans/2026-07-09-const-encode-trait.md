# Cursor + ConstWriteXDR Trait Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an infallible, non-generic, cursor-style encoding surface to xdrs: slice-backed `Cursor` writers, a `ConstWriteXDR` trait with worst-case size consts, and derive support including generic types.

**Architecture:** New `Cursor<'a>` (slice-backed, panicking, `const fn`) per format becomes the canonical infallible writer; `ConstWriter<N>` delegates to it. A new `ConstWriteXDR` trait (non-generic methods, required `MAX_CXS_LEN`/`MAX_TXS_LEN` consts) mirrors the `WriteXDR` impl surface. The `#[derive(ConstWriteXDR)]` macro emits the trait impl (uniform per-field trait calls) alongside the existing inherent `const fn`s (switched to `Cursor`), and now accepts type parameters.

**Tech Stack:** Rust 1.95.0 stable (pinned in rust-toolchain.toml), syn/quote/darling in xdr-derive. No nightly features.

**Spec:** `docs/superpowers/specs/2026-07-09-const-encode-trait-design.md`

## Global Constraints

- Stable Rust only. No `generic_const_exprs`, no `const_trait_impl`.
- Crate is `#![no_std]` + `#![warn(missing_docs)]` with `RUSTFLAGS="--deny warnings"` in CI: **every new pub item needs a doc comment**, and tests must compile with and without `alloc`.
- Existing const-context encoding capability must keep working (additive; the one deliberate breaking change is the derive's inherent-fn signature, Task 8).
- Format parity: everything exists for both `cxs` and `txs`.
- `u8`/`i8` must NEVER implement `ConstWriteXDR` (coherence: `[u8; N]`/`[T; N]` overlap — same trick `WriteXDR` uses).
- Verification battery for byte-equality tests (CXS width boundaries): `0, 1, -1, 31, 32, -31, -32, 62, 63, 127, 128, 255, 256, 32767, 32768, 65535, 65536, i32::MAX, i32::MIN, u32::MAX, i64::MAX, i64::MIN, u64::MAX`.
- Commit after every task (branch `geoff/const-encode-trait`).
- Workspace layout: root manifest is the `xdrs` package + `[workspace]`; members `xdr-common`, `xdr-derive`, `xdr-repr`. Version is `[workspace.package] version = "0.7.1"`; internal deps pin `=0.7.1`.

---

### Task 1: `as_bytes` on both `ConstWriter`s

**Files:**
- Modify: `src/cxs/const_writer.rs` (method + tests)
- Modify: `src/txs/const_writer.rs` (method + tests)

**Interfaces:**
- Produces: `pub const fn as_bytes(&self) -> &[u8]` on `cxs::ConstWriter<N>` and `txs::ConstWriter<N>` — the written prefix `&buf[..pos]`.

- [ ] **Step 1: Write failing tests** — in the `#[cfg(test)] mod tests` of `src/cxs/const_writer.rs`:

```rust
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
```

And in `src/txs/const_writer.rs` tests (TXS u32 is 4-byte BE):

```rust
const AS_BYTES_LEN: usize = {
    let mut w = ConstWriter::<8>::new();
    w.write_u32(300);
    w.as_bytes().len()
};

#[test]
fn as_bytes_partial_fill() {
    let mut w = ConstWriter::<8>::new();
    w.write_u32(300);
    assert_eq!(w.as_bytes(), &[0x00, 0x00, 0x01, 0x2c]);
    assert_eq!(AS_BYTES_LEN, 4);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p xdrs as_bytes`
Expected: FAIL to compile — `as_bytes` not found.

- [ ] **Step 3: Implement** — add to BOTH `impl<const N: usize> ConstWriter<N>` blocks (cxs and txs), right after `into_bytes`:

```rust
/// Returns the bytes written so far (the filled prefix of the buffer).
///
/// Unlike [`into_bytes`](Self::into_bytes), this does not require the
/// buffer to be exactly full, so it supports value-dependent encoded
/// lengths that fall short of `N`.
pub const fn as_bytes(&self) -> &[u8] {
    self.buf.split_at(self.pos).0
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p xdrs as_bytes` → PASS. Then `cargo test -F int128` → PASS (no regressions).

- [ ] **Step 5: Commit**

```bash
git add src/cxs/const_writer.rs src/txs/const_writer.rs
git commit -m "Add ConstWriter::as_bytes for partial-fill access (cxs and txs)"
```

---

### Task 2: `cxs::Cursor` slice-backed const writer

**Files:**
- Modify: `src/cxs/const_writer.rs` (add `Cursor`, make `ConstWriter` delegate)
- Modify: `src/cxs/mod.rs` (export)
- Test: `tests/cursor.rs` (new)

**Interfaces:**
- Produces: `pub struct cxs::Cursor<'a>` with `pub const fn`: `new(&'a mut [u8]) -> Self`, `pos(&self) -> usize`, `as_bytes(&self) -> &[u8]`, and the full write vocabulary `write_bool/write_i16/write_i32/write_i64/write_u16/write_u32/write_u64/write_f32/write_f64/write_opaque_fixed/write_opaque_variable` plus `write_i128/write_u128` under `#[cfg(feature = "int128")]`. All panic on buffer overflow (slice bounds check). Exported as `cxs::Cursor`.
- The CXS width rules live ONLY in `Cursor` after this task; `ConstWriter` write methods delegate.

- [ ] **Step 1: Write failing integration test** — create `tests/cursor.rs`. It must compile without `alloc` (CI runs `cargo test -F int128`), so cross-validate against `cxs::encode_into` (see `src/cxs/utils.rs:63` for the exact signature — it takes `&mut [MaybeUninit<u8>]` and returns the written `&[u8]` in a `WriteResult`; adjust the helper below to the real return shape before assuming):

```rust
use core::mem::MaybeUninit;
use xdrs::cxs;

/// Encode via the fallible reference path; return the bytes in a fixed buffer + len.
fn reference_cxs<T: xdrs::WriteXDR>(v: &T) -> ([u8; 64], usize) {
    let mut mu = [MaybeUninit::<u8>::uninit(); 64];
    let bytes = cxs::encode_into(v, &mut mu).expect("reference encode failed");
    let mut out = [0u8; 64];
    out[..bytes.len()].copy_from_slice(bytes);
    (out, bytes.len())
}

macro_rules! assert_cursor_matches {
    ($write_method:ident, $val:expr) => {{
        let v = $val;
        let (expected, len) = reference_cxs(&v);
        let mut buf = [0u8; 64];
        let mut c = cxs::Cursor::new(&mut buf);
        c.$write_method(v);
        assert_eq!(c.pos(), len, "size mismatch for {:?}", v);
        assert_eq!(c.as_bytes(), &expected[..len], "bytes mismatch for {:?}", v);
    }};
}

#[test]
fn cursor_i32_width_boundaries() {
    for v in [0i32, 1, -1, 31, 32, -31, -32, 62, 63, 127, 128, 255, 256,
              32767, 32768, 65535, 65536, i32::MAX, i32::MIN] {
        assert_cursor_matches!(write_i32, v);
    }
}

#[test]
fn cursor_u32_u64_i64_boundaries() {
    for v in [0u32, 62, 63, 255, 256, 65535, 65536, u32::MAX] {
        assert_cursor_matches!(write_u32, v);
    }
    for v in [0u64, 62, 63, 65536, u32::MAX as u64, u32::MAX as u64 + 1, u64::MAX] {
        assert_cursor_matches!(write_u64, v);
    }
    for v in [0i64, -32, 32768, i32::MAX as i64 + 1, i32::MIN as i64 - 1, i64::MAX, i64::MIN] {
        assert_cursor_matches!(write_i64, v);
    }
}

#[test]
fn cursor_u16_i16_bool_floats() {
    for v in [0u16, 62, 63, 255, 256, u16::MAX] { assert_cursor_matches!(write_u16, v); }
    for v in [0i16, -31, -32, 127, 128, i16::MAX, i16::MIN] { assert_cursor_matches!(write_i16, v); }
    assert_cursor_matches!(write_bool, true);
    assert_cursor_matches!(write_bool, false);
    assert_cursor_matches!(write_f32, 1.5f32);
    assert_cursor_matches!(write_f64, -2.75f64);
}

#[cfg(feature = "int128")]
#[test]
fn cursor_i128_u128() {
    for v in [0i128, -1, i64::MAX as i128 + 1, i128::MAX, i128::MIN] {
        let (expected, len) = reference_cxs(&v);
        let mut buf = [0u8; 64];
        let mut c = cxs::Cursor::new(&mut buf);
        c.write_i128(v);
        assert_eq!((c.pos(), c.as_bytes()), (len, &expected[..len]));
    }
    let mut buf = [0u8; 64];
    let mut c = cxs::Cursor::new(&mut buf);
    c.write_u128(u128::MAX);
    let (expected, len) = reference_cxs(&u128::MAX);
    assert_eq!((c.pos(), c.as_bytes()), (len, &expected[..len]));
}

#[test]
fn cursor_opaque() {
    // variable: length prefix + bytes; fixed: raw bytes
    let data = [0xde, 0xad, 0xbe, 0xef];
    let (expected, len) = reference_cxs(&&data[..]); // &[u8] → opaque variable
    let mut buf = [0u8; 64];
    let mut c = cxs::Cursor::new(&mut buf);
    c.write_opaque_variable(&data);
    assert_eq!((c.pos(), c.as_bytes()), (len, &expected[..len]));

    let mut buf2 = [0u8; 4];
    let mut c2 = cxs::Cursor::new(&mut buf2);
    c2.write_opaque_fixed(&data);
    assert_eq!(c2.as_bytes(), &data);
}

// Const-context usability: build a const array through Cursor.
const CONST_VIA_CURSOR: [u8; 3] = {
    let mut buf = [0u8; 3];
    let mut c = cxs::Cursor::new(&mut buf);
    c.write_i32(-32); // 0x3f 0xe0
    c.write_i32(1);   // 0x01
    assert!(c.pos() == 3);
    buf
};

#[test]
fn cursor_const_context() {
    assert_eq!(CONST_VIA_CURSOR, [0x3f, 0xe0, 0x01]);
}

#[test]
#[should_panic]
fn cursor_overflow_panics() {
    let mut buf = [0u8; 1];
    let mut c = cxs::Cursor::new(&mut buf);
    c.write_u32(u32::MAX); // needs 5 bytes
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --test cursor`
Expected: FAIL to compile — `cxs::Cursor` not found.

- [ ] **Step 3: Implement `Cursor` in `src/cxs/const_writer.rs`.** Mechanical refactor — the format logic moves, it is not rewritten:

1. Add the struct + core methods:

```rust
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
    // ... write methods moved here (next step)
}
```

2. MOVE (verbatim bodies) from `ConstWriter` to `Cursor`: the private helpers `put_byte`, `put_bytes`, `const_write_i8`, `const_write_u8`, and all `pub const fn write_*` methods (`write_opaque_fixed`, `write_opaque_variable`, `write_bool`, `write_f32`, `write_f64`, `write_i64`, `write_i32`, `write_i16`, `write_u64`, `write_u32`, `write_u16`, and the `int128`-gated `write_i128`/`write_u128`). Bodies are unchanged — they only touch `self.buf[..]` and `self.pos`, which have the same shape on `Cursor`. Keep every doc comment and `#[cfg]`/`#[doc(hidden)]` attribute.

3. Rewrite each `ConstWriter` write method as a delegate (same file, so private fields are accessible). Pattern for every write method:

```rust
/// Writes an unsigned 32-bit integer using CXS variable-width encoding.
pub const fn write_u32(&mut self, n: u32) {
    let mut c = Cursor { buf: &mut self.buf, pos: self.pos };
    c.write_u32(n);
    self.pos = c.pos;
}
```

(`&mut self.buf` unsize-coerces to `&mut [u8]` in const fn — verified on this toolchain.) `ConstWriter` keeps only `new`, `pos`, `into_bytes`, `as_bytes`, and the delegating write methods. Delete the now-unused private helpers and the constants block from `ConstWriter`'s side if they moved with the methods (the constants `V_MULTIBYTE`, `S_*`, `IH_*`, `UH_MAX` are file-level — they stay, now used by `Cursor`).

4. Export in `src/cxs/mod.rs`:

```rust
pub use const_writer::{ConstWriter, Cursor};
```

- [ ] **Step 4: Run tests**

Run: `cargo test --test cursor && cargo test -p xdrs && cargo test -F int128`
Expected: all PASS (existing `const_writer` unit tests prove the delegation preserved behavior).

- [ ] **Step 5: Commit**

```bash
git add src/cxs/const_writer.rs src/cxs/mod.rs tests/cursor.rs
git commit -m "Add cxs::Cursor slice-backed const writer; ConstWriter delegates"
```

---

### Task 3: `txs::Cursor` slice-backed const writer

**Files:**
- Modify: `src/txs/const_writer.rs`, `src/txs/mod.rs`
- Test: `tests/cursor.rs` (extend)

**Interfaces:**
- Produces: `pub struct txs::Cursor<'a>` — identical API surface to `cxs::Cursor` (Task 2), TXS rules: fixed-width big-endian (bool/i16/u16/i32/u32/f32 → 4 bytes; i64/u64/f64 → 8; i128/u128 → 16), opaque padded to 4-byte boundary, variable opaque = 4-byte BE length prefix + padded data.

- [ ] **Step 1: Extend `tests/cursor.rs` with TXS tests (failing)** — same macro pattern with a `reference_txs` helper using `xdrs::txs::encode_into`, over the same value battery, plus TXS-specific padding checks:

```rust
fn reference_txs<T: xdrs::WriteXDR>(v: &T) -> ([u8; 64], usize) {
    let mut mu = [MaybeUninit::<u8>::uninit(); 64];
    let bytes = xdrs::txs::encode_into(v, &mut mu).expect("reference encode failed");
    let mut out = [0u8; 64];
    out[..bytes.len()].copy_from_slice(bytes);
    (out, bytes.len())
}

macro_rules! assert_txs_cursor_matches {
    ($write_method:ident, $val:expr) => {{
        let v = $val;
        let (expected, len) = reference_txs(&v);
        let mut buf = [0u8; 64];
        let mut c = xdrs::txs::Cursor::new(&mut buf);
        c.$write_method(v);
        assert_eq!(c.pos(), len);
        assert_eq!(c.as_bytes(), &expected[..len]);
    }};
}

#[test]
fn txs_cursor_all_primitives() {
    for v in [0i32, -1, i32::MAX, i32::MIN] { assert_txs_cursor_matches!(write_i32, v); }
    for v in [0u32, u32::MAX] { assert_txs_cursor_matches!(write_u32, v); }
    for v in [0i64, i64::MAX, i64::MIN] { assert_txs_cursor_matches!(write_i64, v); }
    for v in [0u64, u64::MAX] { assert_txs_cursor_matches!(write_u64, v); }
    for v in [0i16, i16::MIN] { assert_txs_cursor_matches!(write_i16, v); }
    for v in [0u16, u16::MAX] { assert_txs_cursor_matches!(write_u16, v); }
    assert_txs_cursor_matches!(write_bool, true);
    assert_txs_cursor_matches!(write_f32, 1.5f32);
    assert_txs_cursor_matches!(write_f64, -2.75f64);
}

#[test]
fn txs_cursor_opaque_padding() {
    // 5 data bytes → variable = 4 (len) + 5 + 3 (pad) = 12
    let data = [1u8, 2, 3, 4, 5];
    let (expected, len) = reference_txs(&&data[..]);
    assert_eq!(len, 12);
    let mut buf = [0u8; 64];
    let mut c = xdrs::txs::Cursor::new(&mut buf);
    c.write_opaque_variable(&data);
    assert_eq!((c.pos(), c.as_bytes()), (len, &expected[..len]));
}
```

- [ ] **Step 2: Run to verify failure**: `cargo test --test cursor` → FAIL (`txs::Cursor` not found).

- [ ] **Step 3: Implement** — exactly the Task 2 recipe applied to `src/txs/const_writer.rs`: add `Cursor<'a>` (same struct/doc shape, TXS wording), MOVE `put_byte`/`put_bytes` and all `write_*` bodies verbatim, rewrite `ConstWriter` methods as delegates, export `pub use const_writer::{ConstWriter, Cursor};` in `src/txs/mod.rs`.

- [ ] **Step 4: Run**: `cargo test --test cursor && cargo test -p xdrs && cargo test --workspace --all-features` → PASS.

- [ ] **Step 5: Commit**

```bash
git add src/txs/const_writer.rs src/txs/mod.rs tests/cursor.rs
git commit -m "Add txs::Cursor slice-backed const writer; ConstWriter delegates"
```

---

### Task 4: `encoded_max` const arithmetic helpers

**Files:**
- Create: `src/encoded_max.rs`
- Modify: `src/lib.rs` (add `pub mod encoded_max;` next to the other module decls; do NOT glob re-export — names are too generic)

**Interfaces:**
- Produces: `xdrs::encoded_max::{add, mul, max}` — `pub const fn` over `Option<usize>`; `None` propagates (unbounded) and checked overflow yields `None`.

- [ ] **Step 1: Write the module with inline failing tests first** (tests reference fns that don't exist until Step 3 — write the test mod, run, watch it fail to compile):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_propagates_none_and_overflow() {
        assert_eq!(add(Some(2), Some(3)), Some(5));
        assert_eq!(add(None, Some(3)), None);
        assert_eq!(add(Some(2), None), None);
        assert_eq!(add(Some(usize::MAX), Some(1)), None);
    }

    #[test]
    fn mul_propagates() {
        assert_eq!(mul(4, Some(3)), Some(12));
        assert_eq!(mul(4, None), None);
        assert_eq!(mul(0, Some(3)), Some(0));
        assert_eq!(mul(2, Some(usize::MAX)), None);
    }

    #[test]
    fn max_requires_both_bounded() {
        assert_eq!(max(Some(2), Some(3)), Some(3));
        assert_eq!(max(Some(3), Some(2)), Some(3));
        assert_eq!(max(None, Some(2)), None);
        assert_eq!(max(Some(2), None), None);
    }

    const CONST_FOLD: Option<usize> = add(mul(3, Some(9)), Some(5));

    #[test]
    fn usable_in_const() {
        assert_eq!(CONST_FOLD, Some(32));
    }
}
```

- [ ] **Step 2: Run**: `cargo test -p xdrs encoded_max` → FAIL to compile.

- [ ] **Step 3: Implement:**

```rust
//! Const-evaluable arithmetic over optional worst-case encoded sizes.
//!
//! `None` means "unbounded" (no finite worst case). All operations
//! propagate `None`, and checked overflow also yields `None`: an
//! astronomically large bound is as unusable for sizing a buffer as no
//! bound at all, and mapping it to `None` routes such types to the
//! fallible encoding path instead of failing compilation.
//!
//! These functions are the building blocks for
//! [`ConstWriteXDR::MAX_CXS_LEN`](crate::ConstWriteXDR::MAX_CXS_LEN) /
//! [`MAX_TXS_LEN`](crate::ConstWriteXDR::MAX_TXS_LEN) computations,
//! including those generated by `#[derive(ConstWriteXDR)]`.

/// Sum of two optional bounds; `None` if either is unbounded or the sum overflows.
pub const fn add(a: Option<usize>, b: Option<usize>) -> Option<usize> {
    match (a, b) {
        (Some(x), Some(y)) => x.checked_add(y),
        _ => None,
    }
}

/// `n` repetitions of an optional bound; `None` if unbounded or on overflow.
pub const fn mul(n: usize, a: Option<usize>) -> Option<usize> {
    match a {
        Some(x) => x.checked_mul(n),
        None => None,
    }
}

/// Larger of two optional bounds; `None` if either is unbounded.
pub const fn max(a: Option<usize>, b: Option<usize>) -> Option<usize> {
    match (a, b) {
        (Some(x), Some(y)) => Some(if x > y { x } else { y }),
        _ => None,
    }
}
```

(Note: the doc-comment cross-links to `ConstWriteXDR` only resolve after Task 5 — if implementing this task standalone, write the prose without intra-doc links and let Task 5 add them, or land Tasks 4+5 together before `cargo doc` gating.)

- [ ] **Step 4: Run**: `cargo test -p xdrs encoded_max` → PASS.

- [ ] **Step 5: Commit**

```bash
git add src/encoded_max.rs src/lib.rs
git commit -m "Add encoded_max const helpers for Option<usize> size folds"
```

---

### Task 5: `ConstWriteXDR` trait + primitive impls

**Files:**
- Create: `src/const_write_xdr.rs`
- Modify: `src/lib.rs` (`pub mod const_write_xdr;` + `pub use const_write_xdr::*;`)
- Test: `tests/const_write_trait.rs` (new)

**Interfaces:**
- Produces (relied on by every later task):

```rust
pub trait ConstWriteXDR {
    const MAX_CXS_LEN: Option<usize>;
    const MAX_TXS_LEN: Option<usize>;
    fn write_cxs(&self, w: &mut crate::cxs::Cursor<'_>);
    fn write_txs(&self, w: &mut crate::txs::Cursor<'_>);
    fn cxs_size(&self) -> usize;
    fn txs_size(&self) -> usize;
}
```

- Impls for `i16 i32 i64 u16 u32 u64 bool f32 f64` (NOT `u8`/`i8` — coherence with the `[u8; N]` opaque impl, Task 6).
- Name note: `xdrs::ConstWriteXDR` currently resolves to the derive-macro re-export (`lib.rs:122`). Trait (type namespace) and derive macro (macro namespace) legally share the name — the serde `use serde::Serialize` pattern. Both re-exports stay.

- [ ] **Step 1: Write failing test** — `tests/const_write_trait.rs`:

```rust
use core::mem::MaybeUninit;
use xdrs::ConstWriteXDR;

fn reference_cxs<T: xdrs::WriteXDR>(v: &T) -> ([u8; 64], usize) {
    let mut mu = [MaybeUninit::<u8>::uninit(); 64];
    let bytes = xdrs::cxs::encode_into(v, &mut mu).unwrap();
    let mut out = [0u8; 64];
    out[..bytes.len()].copy_from_slice(bytes);
    (out, bytes.len())
}

fn reference_txs<T: xdrs::WriteXDR>(v: &T) -> ([u8; 64], usize) {
    let mut mu = [MaybeUninit::<u8>::uninit(); 64];
    let bytes = xdrs::txs::encode_into(v, &mut mu).unwrap();
    let mut out = [0u8; 64];
    out[..bytes.len()].copy_from_slice(bytes);
    (out, bytes.len())
}

/// Assert: trait encode == WriteXDR encode, exact sizes match, and both
/// sizes are within the declared MAX (when bounded) — for both formats.
fn assert_trait_matches<T: ConstWriteXDR + xdrs::WriteXDR + core::fmt::Debug>(v: &T) {
    let (exp, len) = reference_cxs(v);
    let mut buf = [0u8; 64];
    let mut c = xdrs::cxs::Cursor::new(&mut buf);
    v.write_cxs(&mut c);
    assert_eq!(c.as_bytes(), &exp[..len], "cxs bytes for {v:?}");
    assert_eq!(v.cxs_size(), len, "cxs_size for {v:?}");
    if let Some(m) = T::MAX_CXS_LEN { assert!(len <= m, "cxs MAX for {v:?}"); }

    let (exp, len) = reference_txs(v);
    let mut buf = [0u8; 64];
    let mut c = xdrs::txs::Cursor::new(&mut buf);
    v.write_txs(&mut c);
    assert_eq!(c.as_bytes(), &exp[..len], "txs bytes for {v:?}");
    assert_eq!(v.txs_size(), len, "txs_size for {v:?}");
    if let Some(m) = T::MAX_TXS_LEN { assert!(len <= m, "txs MAX for {v:?}"); }
}

#[test]
fn primitives_match_reference() {
    for v in [0i32, 1, -1, 31, 32, -31, -32, 62, 63, 127, 128, 255, 256,
              32767, 32768, 65535, 65536, i32::MAX, i32::MIN] {
        assert_trait_matches(&v);
    }
    for v in [0u32, 63, 256, 65536, u32::MAX] { assert_trait_matches(&v); }
    for v in [0i64, i64::MAX, i64::MIN] { assert_trait_matches(&v); }
    for v in [0u64, u32::MAX as u64 + 1, u64::MAX] { assert_trait_matches(&v); }
    for v in [0i16, -32, i16::MAX, i16::MIN] { assert_trait_matches(&v); }
    for v in [0u16, 63, u16::MAX] { assert_trait_matches(&v); }
    assert_trait_matches(&true);
    assert_trait_matches(&false);
    assert_trait_matches(&1.5f32);
    assert_trait_matches(&-2.75f64);
}

#[test]
fn primitive_max_values() {
    assert_eq!(<u32 as ConstWriteXDR>::MAX_CXS_LEN, Some(5));
    assert_eq!(<u32 as ConstWriteXDR>::MAX_TXS_LEN, Some(4));
    assert_eq!(<u64 as ConstWriteXDR>::MAX_CXS_LEN, Some(9));
    assert_eq!(<u64 as ConstWriteXDR>::MAX_TXS_LEN, Some(8));
    assert_eq!(<i16 as ConstWriteXDR>::MAX_CXS_LEN, Some(3));
    assert_eq!(<bool as ConstWriteXDR>::MAX_CXS_LEN, Some(1));
    assert_eq!(<bool as ConstWriteXDR>::MAX_TXS_LEN, Some(4));
    assert_eq!(<f64 as ConstWriteXDR>::MAX_CXS_LEN, Some(8));
}
```

- [ ] **Step 2: Run**: `cargo test --test const_write_trait` → FAIL (trait not found).

- [ ] **Step 3: Implement `src/const_write_xdr.rs`:**

```rust
//! The infallible cursor-based encoding trait.
//!
//! [`ConstWriteXDR`] is the runtime companion to the inherent `const fn`
//! encoders emitted by `#[derive(ConstWriteXDR)]`: non-generic methods over
//! [`cxs::Cursor`](crate::cxs::Cursor) / [`txs::Cursor`](crate::txs::Cursor)
//! that panic on buffer overflow instead of returning `Result`, plus
//! worst-case encoded sizes for compile-time buffer sizing.

use crate::{cxs, txs};

/// Types with an infallible, cursor-based XDR encoding and a (possibly
/// unbounded) worst-case encoded size.
///
/// Implementations write through [`cxs::Cursor`] / [`txs::Cursor`], which
/// panic on buffer overflow. Size a buffer from `MAX_CXS_LEN` /
/// `MAX_TXS_LEN` (when `Some`) to make encoding infallible:
///
/// ```rust
/// use xdrs::ConstWriteXDR;
/// const N: usize = match <u64 as ConstWriteXDR>::MAX_CXS_LEN {
///     Some(n) => n,
///     None => panic!("unbounded"),
/// };
/// let mut buf = [0u8; N];
/// let mut c = xdrs::cxs::Cursor::new(&mut buf);
/// 42u64.write_cxs(&mut c);
/// assert_eq!(c.as_bytes(), &[42]);
/// ```
pub trait ConstWriteXDR {
    /// Worst-case CXS encoded size over all values of the type, or `None`
    /// if unbounded.
    const MAX_CXS_LEN: Option<usize>;
    /// Worst-case TXS encoded size over all values of the type, or `None`
    /// if unbounded.
    const MAX_TXS_LEN: Option<usize>;

    /// Writes this value in CXS format. Panics on cursor overflow.
    fn write_cxs(&self, w: &mut cxs::Cursor<'_>);
    /// Writes this value in TXS format. Panics on cursor overflow.
    fn write_txs(&self, w: &mut txs::Cursor<'_>);
    /// Exact CXS encoded size of this value in bytes.
    fn cxs_size(&self) -> usize;
    /// Exact TXS encoded size of this value in bytes.
    fn txs_size(&self) -> usize;
}

// NOTE: u8/i8 intentionally do NOT implement ConstWriteXDR, so that
// `[u8; N]`/`[u8]` can take the opaque-bytes fast path without overlapping
// the generic `[T; N]`/`[T]` impls (same structure as WriteXDR).
macro_rules! const_write_xdr_prim_impls {
    { $( ($ty:ty, $write_method:ident, $size_fn:ident, $cxs_max:literal, $txs_max:literal) ),* $(,)? } => { $(
        impl ConstWriteXDR for $ty {
            const MAX_CXS_LEN: Option<usize> = Some($cxs_max);
            const MAX_TXS_LEN: Option<usize> = Some($txs_max);
            #[inline]
            fn write_cxs(&self, w: &mut cxs::Cursor<'_>) { w.$write_method(*self) }
            #[inline]
            fn write_txs(&self, w: &mut txs::Cursor<'_>) { w.$write_method(*self) }
            #[inline]
            fn cxs_size(&self) -> usize { cxs::const_writer::$size_fn(*self) }
            #[inline]
            fn txs_size(&self) -> usize { txs::const_writer::$size_fn(*self) }
        }
    )* };
}

const_write_xdr_prim_impls! {
    (i16, write_i16, size_i16, 3, 4),
    (i32, write_i32, size_i32, 5, 4),
    (i64, write_i64, size_i64, 9, 8),
    (u16, write_u16, size_u16, 3, 4),
    (u32, write_u32, size_u32, 5, 4),
    (u64, write_u64, size_u64, 9, 8),
    (bool, write_bool, size_bool, 1, 4),
    (f32, write_f32, size_f32, 4, 4),
    (f64, write_f64, size_f64, 8, 8),
}
```

In `src/lib.rs`, next to `pub mod write_xdr;`: add `pub mod const_write_xdr;`, and with the other root re-exports: `pub use const_write_xdr::*;`.

- [ ] **Step 4: Run**: `cargo test --test const_write_trait && cargo test -F int128 && cargo doc -p xdrs` → PASS, docs clean.

- [ ] **Step 5: Commit**

```bash
git add src/const_write_xdr.rs src/lib.rs tests/const_write_trait.rs
git commit -m "Add ConstWriteXDR trait with primitive impls and MAX size consts"
```

---

### Task 6: Composite `ConstWriteXDR` impls (mirror the full WriteXDR surface)

**Files:**
- Modify: `src/const_write_xdr.rs` (all impls below), `src/int128.rs` (i128/u128), `src/heapless.rs` (heapless types)
- Test: `tests/const_write_trait.rs` (extend)

**Interfaces:**
- Produces impls exactly mirroring `src/write_xdr.rs`'s surface (headers, bounds, and cfg gates copied from there — see write_xdr.rs:87-363, int128.rs:35-46, heapless.rs:24-84):

| Impl | MAX_CXS_LEN | MAX_TXS_LEN |
|---|---|---|
| `[u8]`, `&[u8]`, `[T]`, `&[T]`, `str`, `&str` | `None` | `None` |
| `[u8; N]` | `Some(cxs size_opaque_fixed(N))` = N | `Some(txs size_opaque_fixed(N))` = N padded to 4 |
| `[T; N]` (T: ConstWriteXDR) | `mul(N, T::MAX_CXS_LEN)` | `mul(N, T::MAX_TXS_LEN)` |
| `&T`, `&mut T` (T: ConstWriteXDR) | forward T's | forward T's |
| `Option<T>` | `add(Some(1), T's)` | `add(Some(4), T's)` |
| `Result<T, E>` | `add(Some(1), max(T's, E's))` | `add(Some(4), max(T's, E's))` |
| `()`, `Infallible`, `PhantomData<T>` | `Some(0)` | `Some(0)` |
| tuples `(T,)` .. 16-arity | fold `add` over elements | fold `add` over elements |
| `String`, `Vec<u8>`, `Vec<T>` (alloc-gated) | `None` | `None` |
| `i128`/`u128` (int128-gated, in int128.rs) | `Some(18)` | `Some(16)` |
| heapless `Vec<u8, N>` (heapless-gated) | `add(Some(size_u32(N as u32)), Some(N))` | `Some(4 + txs size_opaque_fixed(N))` |
| heapless `Vec<T, N>` | `add(Some(size_u32(N as u32)), mul(N, T's))` | `add(Some(4), mul(N, T's))` |
| heapless `String<N>` | same as `Vec<u8, N>` | same |

- [ ] **Step 1: Write failing tests** — extend `tests/const_write_trait.rs`:

```rust
#[test]
fn composite_shapes_match_reference() {
    assert_trait_matches(&[0xdeu8, 0xad, 0xbe]);              // [u8; 3] opaque fixed
    assert_trait_matches(&[1u32, 65536, 0]);                  // [u32; 3]
    assert_trait_matches(&Some(65536u32));
    assert_trait_matches(&Option::<u32>::None);
    assert_trait_matches(&Ok::<u32, i64>(7));
    assert_trait_matches(&Err::<u32, i64>(-9));
    assert_trait_matches(&(1u32, -1i64, true));               // tuple
    assert_trait_matches(&"hello");                           // &str
    let s: &[u8] = &[9, 8, 7];
    assert_trait_matches(&s);                                 // &[u8] variable opaque
    let t: &[u32] = &[1, 2, 3];
    assert_trait_matches(&t);                                 // &[T]
}

#[test]
fn composite_max_values() {
    assert_eq!(<[u8; 5] as ConstWriteXDR>::MAX_CXS_LEN, Some(5));
    assert_eq!(<[u8; 5] as ConstWriteXDR>::MAX_TXS_LEN, Some(8)); // padded to 4
    assert_eq!(<[u32; 3] as ConstWriteXDR>::MAX_CXS_LEN, Some(15));
    assert_eq!(<Option<u64> as ConstWriteXDR>::MAX_CXS_LEN, Some(10));
    assert_eq!(<Option<u64> as ConstWriteXDR>::MAX_TXS_LEN, Some(12));
    assert_eq!(<Result<u32, i64> as ConstWriteXDR>::MAX_CXS_LEN, Some(10)); // 1 + max(5, 9)
    assert_eq!(<(u32, bool) as ConstWriteXDR>::MAX_CXS_LEN, Some(6));
    assert_eq!(<&str as ConstWriteXDR>::MAX_CXS_LEN, None);
    assert_eq!(<&[u8] as ConstWriteXDR>::MAX_CXS_LEN, None);
    assert_eq!(<() as ConstWriteXDR>::MAX_CXS_LEN, Some(0));
}

#[cfg(feature = "int128")]
#[test]
fn int128_trait_impls() {
    assert_trait_matches(&i128::MAX);
    assert_trait_matches(&u128::MAX);
    assert_eq!(<i128 as ConstWriteXDR>::MAX_CXS_LEN, Some(18));
    assert_eq!(<u128 as ConstWriteXDR>::MAX_TXS_LEN, Some(16));
}
```

(For alloc-gated `Vec`/`String` impls add a `#[cfg(feature = "alloc")]` test asserting `MAX_* == None` and byte equality for a small `Vec<u32>`; the workspace `--all-features` run exercises it.)

- [ ] **Step 2: Run**: `cargo test --test const_write_trait` → FAIL (missing impls).

- [ ] **Step 3: Implement.** Every impl mirrors the corresponding `WriteXDR` impl in structure; write bodies call `Cursor` methods (no `?`, no length-error returns — length guards `assert!` like `ConstWriter` does). Representative impls (the rest follow mechanically; keep the same order as write_xdr.rs so the files stay diffable):

```rust
use crate::encoded_max;
use crate::valid_u32::ValidU32;

impl ConstWriteXDR for [u8] {
    const MAX_CXS_LEN: Option<usize> = None;
    const MAX_TXS_LEN: Option<usize> = None;
    fn write_cxs(&self, w: &mut cxs::Cursor<'_>) { w.write_opaque_variable(self) }
    fn write_txs(&self, w: &mut txs::Cursor<'_>) { w.write_opaque_variable(self) }
    fn cxs_size(&self) -> usize { cxs::const_writer::size_opaque_variable(self.len()) }
    fn txs_size(&self) -> usize { txs::const_writer::size_opaque_variable(self.len()) }
}

impl<const N: usize> ConstWriteXDR for [u8; N] {
    const MAX_CXS_LEN: Option<usize> = Some(cxs::const_writer::size_opaque_fixed(N));
    const MAX_TXS_LEN: Option<usize> = Some(txs::const_writer::size_opaque_fixed(N));
    fn write_cxs(&self, w: &mut cxs::Cursor<'_>) {
        let () = ValidU32::<N>::OK;
        w.write_opaque_fixed(self)
    }
    fn write_txs(&self, w: &mut txs::Cursor<'_>) {
        let () = ValidU32::<N>::OK;
        w.write_opaque_fixed(self)
    }
    fn cxs_size(&self) -> usize { cxs::const_writer::size_opaque_fixed(N) }
    fn txs_size(&self) -> usize { txs::const_writer::size_opaque_fixed(N) }
}

impl<T: ConstWriteXDR, const N: usize> ConstWriteXDR for [T; N] {
    const MAX_CXS_LEN: Option<usize> = encoded_max::mul(N, T::MAX_CXS_LEN);
    const MAX_TXS_LEN: Option<usize> = encoded_max::mul(N, T::MAX_TXS_LEN);
    fn write_cxs(&self, w: &mut cxs::Cursor<'_>) {
        let () = ValidU32::<N>::OK;
        let mut i = 0;
        while i < N { self[i].write_cxs(w); i += 1; }
    }
    // write_txs analogous; sizes sum element sizes in a loop.
    # ...
}

impl<T: ConstWriteXDR> ConstWriteXDR for [T] {
    const MAX_CXS_LEN: Option<usize> = None;
    const MAX_TXS_LEN: Option<usize> = None;
    fn write_cxs(&self, w: &mut cxs::Cursor<'_>) {
        assert!(self.len() <= u32::MAX as usize, "array length exceeds u32::MAX");
        w.write_u32(self.len() as u32);
        for elt in self { elt.write_cxs(w); }
    }
    // ...
}

impl<T: ConstWriteXDR> ConstWriteXDR for Option<T> {
    // CXS discriminant (i32 0|1) encodes in 1 byte; TXS in 4.
    const MAX_CXS_LEN: Option<usize> = encoded_max::add(Some(1), T::MAX_CXS_LEN);
    const MAX_TXS_LEN: Option<usize> = encoded_max::add(Some(4), T::MAX_TXS_LEN);
    fn write_cxs(&self, w: &mut cxs::Cursor<'_>) {
        match self {
            Some(v) => { w.write_i32(1); v.write_cxs(w); }
            None => w.write_i32(0),
        }
    }
    // ...
}
```

Notes for the remaining impls:
- `&T`/`&mut T`/`&[T]`/`&[u8]`/`&str` forward exactly like their WriteXDR twins (write_xdr.rs:93,135-162); MAX consts forward (`T::MAX_CXS_LEN`) or are `None` for the unsized-slice refs.
- `Result<T, E>`: Ok arm writes `w.write_i32(0)` then value; Err arm `w.write_i32(1)` then error (same discriminants as write_xdr.rs:318).
- Tuples: copy the `write_tuple!` macro structure (write_xdr.rs:188-227) — per element `$v.write_cxs(w);`, sizes `0 $(+ $v.cxs_size())*`, MAX `encoded_max::add(...)` folded left across element types, arities 1–16.
- `()`/`Infallible`/`PhantomData<T>`: no-op writes (`Infallible` body is `match *self {}`), sizes 0, MAX `Some(0)`.
- alloc block mirrors write_xdr.rs:343-363 (`Vec<u8>` → opaque variable, `Vec<T>` → via `&[T]`), `String` via `str`; all MAX `None`.
- `src/int128.rs`: add ConstWriteXDR impls next to the WriteXDR ones — `write_cxs` calls `w.write_i128(*self)` (Cursor has it, int128-gated); MAX per table.
- `src/heapless.rs`: mirror the three WriteXDR impls with `ValidU32::<N>::OK` guards; MAX per table.

- [ ] **Step 4: Run**: `cargo test --test const_write_trait && cargo test --workspace --all-features && cargo test -F int128` → PASS.

- [ ] **Step 5: Commit**

```bash
git add src/const_write_xdr.rs src/int128.rs src/heapless.rs tests/const_write_trait.rs
git commit -m "Mirror the WriteXDR impl surface on ConstWriteXDR"
```

---

### Task 7: xdr_common bounded-type impls

**Files:**
- Modify: `src/const_write_xdr.rs` (a `const _: () = { use xdr_common::*; ... }` block mirroring write_xdr.rs:240-316)
- Test: `tests/const_write_trait.rs` (extend)

**Interfaces:**
- Produces impls (headers and cfg gates copied from write_xdr.rs:245-311):
  - `impl<const MAX: u32> ConstWriteXDR for BytesM<MAX>` / `StringM<MAX>` / `VecM<u8, MAX>` (alloc + no-alloc variants like WriteXDR): bytes via `<Self as AsRef<[u8]>>::as_ref(self)`, `assert!(len <= MAX as usize)` instead of WriteXDR's `InvalidArrayLength` error. MAX consts: CXS `add(Some(size_u32(MAX)), Some(MAX as usize))`; TXS `Some(4 + txs size_opaque_fixed(MAX as usize))`.
  - `impl<const MAX: u32, T: ConstWriteXDR> ConstWriteXDR for VecM<T, MAX>` (alloc / no-alloc `T: ConstWriteXDR + 'static` variants): length-prefix + element loop with the same length assert; MAX: CXS `add(Some(size_u32(MAX)), mul(MAX as usize, T::MAX_CXS_LEN))`; TXS `add(Some(4), mul(MAX as usize, T::MAX_TXS_LEN))`.
  - `#[cfg(feature = "alloc")] impl<T: ConstWriteXDR> ConstWriteXDR for xdr_common::Box<T>` forwarding to `self.as_ref()`, MAX forwards T's.
- **Spec deviation (documented):** NO inherent `const fn`s on these types — their `Vec` backing is not const-accessible, and alloc-backed values cannot exist in const contexts anyway. Trait impls only.

- [ ] **Step 1: Write failing tests:**

```rust
#[cfg(feature = "alloc")]
mod xdr_common_types {
    use super::*;
    use xdrs::{BytesM, StringM, VecM};

    #[test]
    fn bounded_types_match_reference() {
        let b: BytesM<8> = vec![1u8, 2, 3].try_into().unwrap();
        assert_trait_matches(&b);
        let s: StringM<8> = "hi".try_into().unwrap();
        assert_trait_matches(&s);
        let v: VecM<u32, 4> = vec![1u32, 65536].try_into().unwrap();
        assert_trait_matches(&v);
    }

    #[test]
    fn bounded_max_values() {
        // CXS: len-varint(8)=1, so 1 + 8
        assert_eq!(<BytesM<8> as ConstWriteXDR>::MAX_CXS_LEN, Some(9));
        // TXS: 4 (len) + 8 (already 4-aligned)
        assert_eq!(<BytesM<8> as ConstWriteXDR>::MAX_TXS_LEN, Some(12));
        // TXS pads data: 4 + size_opaque_fixed(5) = 4 + 8
        assert_eq!(<BytesM<5> as ConstWriteXDR>::MAX_TXS_LEN, Some(12));
        // CXS: len-varint(4)=1 + 4 elems × 5
        assert_eq!(<VecM<u32, 4> as ConstWriteXDR>::MAX_CXS_LEN, Some(21));
        // len-varint(300) = 3 bytes (300 fits 2-byte payload + header)
        assert_eq!(<BytesM<300> as ConstWriteXDR>::MAX_CXS_LEN, Some(303));
        // Unbounded element type poisons the container max
        assert_eq!(<VecM<VecM<u8>, 4> as ConstWriteXDR>::MAX_CXS_LEN, None);
    }
}
```

Note the default `VecM<u8>` (MAX = u32::MAX): its MAX_CXS_LEN is `add(Some(5), Some(u32::MAX as usize))` — huge but finite and non-overflowing on 64-bit; the nested test asserts the *outer* mul overflow → `None` path. Add one direct assert: `assert_eq!(<VecM<u8> as ConstWriteXDR>::MAX_CXS_LEN, Some(5 + u32::MAX as usize));`

- [ ] **Step 2: Run**: `cargo test --all-features --test const_write_trait` → FAIL.

- [ ] **Step 3: Implement** the block in `src/const_write_xdr.rs`. Representative impl (others follow the WriteXDR twins):

```rust
const _: () = {
    use super::*;
    use xdr_common::*;

    impl<const MAX: u32> ConstWriteXDR for BytesM<MAX> {
        const MAX_CXS_LEN: Option<usize> = encoded_max::add(
            Some(cxs::const_writer::size_u32(MAX)),
            Some(MAX as usize),
        );
        const MAX_TXS_LEN: Option<usize> =
            Some(4 + txs::const_writer::size_opaque_fixed(MAX as usize));
        fn write_cxs(&self, w: &mut cxs::Cursor<'_>) {
            let bytes: &[u8] = <Self as AsRef<[u8]>>::as_ref(self);
            assert!(bytes.len() <= MAX as usize, "value length exceeds declared bound");
            w.write_opaque_variable(bytes);
        }
        // write_txs analogous; sizes via size_opaque_variable(bytes.len()).
        # ...
    }
    // StringM<MAX>, VecM<u8, MAX> (alloc/no-alloc), VecM<T, MAX> (alloc/no-alloc), Box<T>
};
```

- [ ] **Step 4: Run**: `cargo test --workspace --all-features && cargo test -F int128` → PASS (the no-alloc run compiles the no-alloc VecM variants).

- [ ] **Step 5: Commit**

```bash
git add src/const_write_xdr.rs tests/const_write_trait.rs
git commit -m "Add ConstWriteXDR impls for xdr_common bounded containers"
```

---

### Task 8: Derive — trait impl + MAX consts + Cursor signature switch

**Files:**
- Modify: `xdr-derive/src/generate_const.rs` (core changes + its unit tests)
- Modify: `xdr-derive/src/lib.rs` (derive doc update)
- Modify: `tests/const_writer.rs`, `tests/const_opaque_bounds.rs`, `tests/borrowed_slices.rs`, `tests/plain_ref.rs` (call-site migration)
- Test: `tests/const_write_trait.rs` (derived-type coverage)

**Interfaces:**
- Consumes: `ConstWriteXDR` trait (Task 5), `encoded_max` (Task 4), `cxs::Cursor`/`txs::Cursor` (Tasks 2-3).
- Produces, for `#[derive(ConstWriteXDR)]` on a type with no type parameters:
  1. Inherent `pub const fn const_write_cxs(&self, w: &mut cxs::Cursor<'_>)` / `const_write_txs(&self, w: &mut txs::Cursor<'_>)` / `const_cxs_size(&self) -> usize` / `const_txs_size(&self) -> usize` — same bodies as today, new writer type, `<const __N: usize>` generic GONE (delete `fresh_buf_size_ident` and both dunder-N unit tests).
  2. A `ConstWriteXDR` trait impl whose write/size methods forward to the inherent fns (`self.const_write_cxs(w)` etc.) and whose `MAX_CXS_LEN`/`MAX_TXS_LEN` are const folds (below).
- New attr `#[xdrs(runtime_only)]` (add `pub runtime_only: bool` to `Attrs`): skips the inherent `const fn`s and emits the trait impl with direct bodies instead — for types whose fields (e.g. `StringM`) have no inherent const encoders. **Breaking change (intentional, spec §6):** existing derive users' const call sites migrate from `ConstWriter::<N>` to a buffer + `Cursor`.

**MAX const fold (generated in the trait impl).** For each field type `#fty`, the max expr is uniformly `<#fty as #krate::ConstWriteXDR>::MAX_CXS_LEN` (every shape has a trait impl after Tasks 5-7). Structs:

```rust
const MAX_CXS_LEN: Option<usize> = {
    let mut m = Some(0usize);
    m = #krate::encoded_max::add(m, <#fty0 as #krate::ConstWriteXDR>::MAX_CXS_LEN);
    m = #krate::encoded_max::add(m, <#fty1 as #krate::ConstWriteXDR>::MAX_CXS_LEN);
    // ... one line per field
    m
};
```

Enums — per variant `add(disc_max, payload_fold)`, then fold `encoded_max::max` across variants (empty enum → `Some(0)`):
- explicit discriminant `#d`: `Some(#krate::cxs::const_writer::size_i32(#d))` (txs: `size_i32` from txs, always 4)
- default arm: `Some(5)` for CXS / `Some(4)` for TXS (worst-case i32), and its field 0 is EXCLUDED from the payload fold (it is the discriminant)
- implicit: `Some(#krate::cxs::const_writer::size_i32(#idx))`

**Trait write/size bodies when direct (runtime_only path; also reused by Task 9 for generics).** Uniform per field — no shape dispatch:

```rust
fn write_cxs(&self, w: &mut #krate::cxs::Cursor<'_>) {
    <#fty as #krate::ConstWriteXDR>::write_cxs(&self.#fa, w);
    // ... per field; enums: match with the same arm structure as the
    // existing const_write_variant (discriminant via w.write_i32(#discriminant),
    // bindings _self_i, default-arm field 0 skipped), bodies per-field
    // <#fty as ConstWriteXDR>::write_cxs(#bind, w);
}
fn cxs_size(&self) -> usize {
    0 + <#fty as #krate::ConstWriteXDR>::cxs_size(&self.#fa) + /* per field */
}
```

- [ ] **Step 1: Write failing derived-type tests** — extend `tests/const_write_trait.rs`:

```rust
mod derived {
    use super::*;
    use xdrs::{ConstWriteXDR, WriteXDR};

    #[derive(WriteXDR, ConstWriteXDR, Debug)]
    struct Point { x: i32, y: u64, tag: [u8; 3] }

    #[derive(WriteXDR, ConstWriteXDR, Debug)]
    enum Shape {
        Circle { radius: u32 },
        Rect { w: u32, h: u32 },
    }

    #[test]
    fn derived_trait_matches_reference() {
        assert_trait_matches(&Point { x: -32, y: u64::MAX, tag: *b"abc" });
        assert_trait_matches(&Shape::Circle { radius: 65536 });
        assert_trait_matches(&Shape::Rect { w: 0, h: u32::MAX });
    }

    #[test]
    fn derived_max_values() {
        // 5 (i32) + 9 (u64) + 3 (opaque fixed)
        assert_eq!(<Point as ConstWriteXDR>::MAX_CXS_LEN, Some(17));
        // txs: 4 + 8 + 4 (3 padded)
        assert_eq!(<Point as ConstWriteXDR>::MAX_TXS_LEN, Some(16));
        // variants: disc(idx 0|1 → 1 byte cxs) + payload; Rect wins: 1 + 5 + 5
        assert_eq!(<Shape as ConstWriteXDR>::MAX_CXS_LEN, Some(11));
        assert_eq!(<Shape as ConstWriteXDR>::MAX_TXS_LEN, Some(12)); // 4 + 4 + 4
    }

    #[derive(WriteXDR, ConstWriteXDR, Debug)]
    #[xdrs(runtime_only)]
    struct WithString { name: xdrs::StringM<16>, id: u32 }

    #[cfg(feature = "alloc")]
    #[test]
    fn runtime_only_derive() {
        let v = WithString { name: "abc".try_into().unwrap(), id: 7 };
        assert_trait_matches(&v);
        // 1 + 16 (StringM<16>) + 5 (u32)
        assert_eq!(<WithString as ConstWriteXDR>::MAX_CXS_LEN, Some(22));
    }
}
```

(Gate the whole `WithString` type `#[cfg(feature = "alloc")]` since StringM construction needs alloc.)

- [ ] **Step 2: Run**: `cargo test -F derive --test const_write_trait` → FAIL (derive emits no trait impl yet). Note which existing tests also run: `cargo test --workspace --all-features` currently PASSES — it must pass again by Step 6.

- [ ] **Step 3: Implement in `generate_const.rs`:**
  1. `Attrs`: add `pub runtime_only: bool`.
  2. Signature switch: in `const_write_struct` (lines ~108-127) and `const_write_enum` (~441-467) replace `<const #buf: usize>(&self, w: &mut cxs::ConstWriter<#buf>)` with `(&self, w: &mut cxs::Cursor<'_>)` (txs likewise). Delete `fresh_buf_size_ident` and its two unit tests (`const_generic_named_dunder_n_*`). The field-dispatch code (`const_write_value_expr`, `const_size_value_expr`) is UNCHANGED — leaf fallback `.const_write_cxs(w)` still typechecks since children switched too.
  3. Gate inherent emission on `!attrs.runtime_only` (type-param gating comes in Task 9 — for now `runtime_only` is the only switch).
  4. New functions `trait_impl_struct`/`trait_impl_enum` (or one `trait_impl(input, item)`) emitting the `impl ConstWriteXDR` block: MAX folds as specified above; write/size methods forwarding to inherent fns when they exist, direct uniform bodies when `runtime_only`. Emit inside the same `const _: () = { ... }` wrapper; reference the trait as `#krate::ConstWriteXDR` (fully qualified — do not import it unqualified, the call-site scope already has the derive macro under that name; UFCS paths avoid any ambiguity).
  5. Enum trait bodies reuse the existing variant-arm machinery (`const_var_bindings`, discriminant selection identical to `const_write_variant` lines 639-646) with per-field trait calls.
  6. Update derive rustdoc in `xdr-derive/src/lib.rs` (document the trait emission, `runtime_only`, the new signatures, and the MAX consts; delete the `__N` hygiene bullet).
- [ ] **Step 4: Migrate integration-test call sites** in `tests/const_writer.rs`, `tests/const_opaque_bounds.rs`, `tests/borrowed_slices.rs`, `tests/plain_ref.rs`. Old → new pattern:

```rust
// OLD
const FOO_CXS: [u8; 4] = {
    let mut w = xdrs::cxs::ConstWriter::<4>::new();
    FOO.const_write_cxs(&mut w);
    w.into_bytes()
};
// NEW
const FOO_CXS: [u8; 4] = {
    let mut buf = [0u8; 4];
    let mut w = xdrs::cxs::Cursor::new(&mut buf);
    FOO.const_write_cxs(&mut w);
    assert!(w.pos() == 4); // preserves into_bytes' exact-fill guarantee
    buf
};
```

The `[u8; V.const_cxs_size()]`-sized variants keep working (size fns unchanged); replace the writer construction the same way with `let mut buf = [0u8; V.const_cxs_size()];`. In `const_opaque_bounds.rs`, `ConstWriter::<0>::new()` becomes `let mut buf = [0u8; 0]; let mut w = ...Cursor::new(&mut buf);`.

- [ ] **Step 5: Run the full gate**

Run: `cargo test --workspace --all-features && cargo test -F int128 && cargo test -F derive --test const_write_trait`
Expected: PASS, including the generator's own unit tests (updated) and all four migrated integration files.

- [ ] **Step 6: Commit**

```bash
git add xdr-derive/src tests/
git commit -m "Derive ConstWriteXDR trait impl with MAX consts; switch inherent fns to Cursor"
```

---

### Task 9: Derive — accept type parameters

**Files:**
- Modify: `xdr-derive/src/generate_const.rs`
- Test: `tests/generic_const_derive.rs` (new)

**Interfaces:**
- Consumes: direct trait-body emission from Task 8.
- Produces: `#[derive(ConstWriteXDR)]` on a type with type parameters emits ONLY the trait impl (no inherent const fns), with synthesized bounds: every type parameter gets `#param: #krate::ConstWriteXDR` appended to the impl's where-clause (serde-style; deviation from the WriteXDR derive's verbatim-generics approach is deliberate — generic callers need the bound to exist). `into_type`/`via` stay rejected.

- [ ] **Step 1: Write failing test** — `tests/generic_const_derive.rs`:

```rust
#![cfg(feature = "derive")]
use core::mem::MaybeUninit;
use xdrs::{ConstWriteXDR, WriteXDR};

#[derive(WriteXDR, ConstWriteXDR, Debug)]
struct UserExn { code: u64, detail: [u8; 4] }

// The sdk ContractExn<E> shape: generic payload enum.
#[derive(WriteXDR, ConstWriteXDR, Debug)]
enum ContractExn<E> {
    System { code: u32 },
    User { exn: E },
}

fn reference_cxs<T: WriteXDR>(v: &T) -> ([u8; 64], usize) {
    let mut mu = [MaybeUninit::<u8>::uninit(); 64];
    let bytes = xdrs::cxs::encode_into(v, &mut mu).unwrap();
    let mut out = [0u8; 64];
    out[..bytes.len()].copy_from_slice(bytes);
    (out, bytes.len())
}

// Generic code can bound on the trait — the whole point.
fn encode_infallible<'a, T: ConstWriteXDR>(v: &T, buf: &'a mut [u8]) -> &'a [u8] {
    let mut c = xdrs::cxs::Cursor::new(buf);
    v.write_cxs(&mut c);
    let n = c.pos();
    &buf[..n]
}

#[test]
fn generic_enum_derives_and_matches_reference() {
    let v = ContractExn::User { exn: UserExn { code: u64::MAX, detail: *b"oops" } };
    let (expected, len) = reference_cxs(&v);
    let mut buf = [0u8; 64];
    assert_eq!(encode_infallible(&v, &mut buf), &expected[..len]);
    assert_eq!(v.cxs_size(), len);

    let s = ContractExn::<UserExn>::System { code: 7 };
    let (expected, len) = reference_cxs(&s);
    let mut buf = [0u8; 64];
    assert_eq!(encode_infallible(&s, &mut buf), &expected[..len]);
}

#[test]
fn generic_enum_max_composes() {
    // System: 1 (disc) + 5; User: 1 (disc) + (9 + 4)
    assert_eq!(<ContractExn<UserExn> as ConstWriteXDR>::MAX_CXS_LEN, Some(14));
    // A huge-but-finite E composes: User arm = 1 (disc) + 5 (len varint) + u32::MAX.
    assert_eq!(<ContractExn<xdrs::VecM<u8>> as ConstWriteXDR>::MAX_CXS_LEN,
               Some(1 + 5 + u32::MAX as usize));
    // A truly unbounded field type poisons the max but the type still derives.
    assert_eq!(<ContractExn<&[u8]> as ConstWriteXDR>::MAX_CXS_LEN, None);
}
```

- [ ] **Step 2: Run**: `cargo test -F derive --test generic_const_derive` → FAIL ("does not support type parameters").

- [ ] **Step 3: Implement:**
  1. In `const_write_data`, replace the type-param rejection with: `let has_type_params = input.generics.params.iter().any(|p| matches!(p, syn::GenericParam::Type(_)));` — inherent const fns are emitted only when `!has_type_params && !attrs.runtime_only`; the trait impl is always emitted (direct bodies when inherent fns are absent).
  2. Bound synthesis for the trait impl: clone `input.generics`, push `syn::parse_quote!(#param_ident: #krate::ConstWriteXDR)` into the where-clause for each `GenericParam::Type`, then `split_for_impl()` on the clone. (Only the trait impl gets these bounds.)
  3. Update the `type_generics_are_rejected` unit test → `type_generics_get_trait_impl_only`: assert output contains `ConstWriteXDR for` and does NOT contain `const_write_cxs` for a `Bad<T>` input.
- [ ] **Step 4: Run**: `cargo test -F derive --test generic_const_derive && cargo test --workspace --all-features` → PASS.

- [ ] **Step 5: Commit**

```bash
git add xdr-derive/src tests/generic_const_derive.rs
git commit -m "Derive ConstWriteXDR for generic types via trait-only emission"
```

---

### Task 10: Docs, version 0.8.0, full ci.sh

**Files:**
- Modify: `src/lib.rs` (crate docs), `Cargo.toml` (version + internal dep pins), `CXS.md` (only if it documents the writer APIs — check), `xdr-derive/src/lib.rs` (crate-level doc list of derives, already touched in Task 8)

- [ ] **Step 1: Crate docs** — in `src/lib.rs` module docs, add to the trait list: `ConstWriteXDR` (infallible cursor encoding + worst-case sizes) and mention `cxs::Cursor`/`txs::Cursor` under the format bullets. While there, fix the stale `xdr_common` feature bullet (xdr-common is a non-optional dependency; the bullet describes a feature that doesn't exist in Cargo.toml — reword it as a description of the `xdr_common` type support rather than a feature).
- [ ] **Step 2: Version bump** — root `Cargo.toml`: `[workspace.package] version = "0.7.1"` → `"0.8.0"`, and every internal workspace dep pin `=0.7.1` → `=0.8.0` (xdr-common, xdr-derive, xdr-repr entries in `[workspace.dependencies]`). Run `cargo tree -p xdrs | head` to confirm resolution.
- [ ] **Step 3: Run the FULL CI script**: `./ci.sh` — this includes no-alloc runs, all-features workspace tests, benches compile, `cargo doc` with `--deny warnings` (missing_docs!), and nightly docsrs docs. If the nightly toolchain isn't installed locally, run every non-nightly line by hand and note the skip in the commit message; CI will cover the nightly steps.
Expected: everything green.
- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "Document ConstWriteXDR surface; bump workspace to 0.8.0"
```

---

### Task 11: PR and CI to green

- [ ] **Step 1:** `git push -u origin geoff/const-encode-trait`
- [ ] **Step 2:** Open the PR with `gh pr create` — title "Add infallible cursor encoding surface: Cursor, ConstWriteXDR trait, derive support"; body summarizes the spec (link `docs/superpowers/specs/2026-07-09-const-encode-trait-design.md`), calls out the deliberate derive-signature break and the 0.8.0 bump.
- [ ] **Step 3:** Use the check-ci skill: monitor checks, fix any failures, address review-bot comments. Iterate until all checks pass.

---

## Self-review notes (kept for executors)

- Spec §1 → Task 1; §2 → Tasks 2-3; §3 → Task 5; §4 → Task 4; §5 → Tasks 6-7; §6 → Tasks 8-9; §7 → test steps throughout; §8 → Tasks 10-11.
- Deviations from spec, all deliberate: (a) xdr_common types get NO inherent const fns (Vec backing not const-accessible) — trait impls only; (b) new `#[xdrs(runtime_only)]` derive attr (escape hatch made necessary by (a)); (c) derive synthesizes `T: ConstWriteXDR` bounds rather than copying generics verbatim.
- The `# ...` lines inside two code blocks mark where mechanically-analogous methods (txs twins, size fns) are elided; the pattern to follow is stated inline each time and the WriteXDR twin is the reference. Everything non-mechanical is written out.
