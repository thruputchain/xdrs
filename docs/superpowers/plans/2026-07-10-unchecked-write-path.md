# Unchecked Write Path Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** An infallible, unsafe serialization path (`WriterUnchecked<F>` + `WriteXDR::write_xdr_unchecked` + `encode_into_unchecked`) with zero runtime checks in release builds, for minimum code size in wasm smart contracts.

**Architecture:** A new `WriterUnchecked<F>` trait mirrors `Writer<F>` with unsafe, infallible methods. The existing `WriteXDR` trait gains a required `unsafe fn write_xdr_unchecked`; every impl (including derive-generated ones) mirrors its checked body with `debug_assert!`s instead of error returns. Each format module (txs, cxs — kept fully segregated) gains a crate-private `PutUnchecked` trait with a blanket `WriterUnchecked` impl, an `UncheckedBufWriter` struct, and an `encode_into_unchecked` entry point.

**Tech Stack:** Rust (edition 2024, no_std), proc-macro derive (`xdr-derive`), wasm32 size measurement (`wasm-size-test`).

**Spec:** `docs/superpowers/specs/2026-07-10-unchecked-write-path-design.md`

## Global Constraints

- Edition 2024: every unsafe operation inside an `unsafe fn` needs an explicit `unsafe { }` block (`unsafe_op_in_unsafe_fn` warns, and CI sets `RUSTFLAGS="--deny warnings"`).
- `#![warn(missing_docs)]` is enforced: every new public item needs docs; every public `unsafe fn` (and unsafe trait method) needs a `# Safety` section (clippy `missing_safety_doc`).
- All preconditions are `debug_assert!` only — **no** check, branch, or panic path may survive in release builds on the unchecked path.
- TXS and CXS stay segregated: unchecked code lives in `src/txs/…` and `src/cxs/…` separately, mirroring the existing `put.rs`/`into_buf_writer.rs` structure.
- Adding a required method to `WriteXDR` breaks compilation until ALL impls have it — Task 4 is deliberately atomic (lib impls + derive in one commit).
- Work on branch `geoff/unchecked-write-path` (already checked out). Run `cargo fmt` before each commit.
- The full test suite is `./ci.sh`; per-task commands below are the fast subsets.

---

### Task 1: `WriterUnchecked<F>` trait

**Files:**
- Create: `src/serialize/writer_unchecked.rs`
- Modify: `src/serialize/mod.rs` (register module)
- Modify: `src/lib.rs` (re-export; crate-docs bullet)

**Interfaces:**
- Consumes: nothing.
- Produces: `pub trait WriterUnchecked<F>` with 11 unsafe methods: `write_opaque_fixed(&mut self, bytes: &[u8])`, `write_opaque_variable(&mut self, bytes: &[u8])`, `write_i64(i64)`, `write_i32(i32)`, `write_i16(i16)`, `write_u64(u64)`, `write_u32(u32)`, `write_u16(u16)`, `write_bool(bool)`, `write_f32(f32)`, `write_f64(f64)` — all `unsafe fn(&mut self, …)` returning `()`. Re-exported at crate root (`xdrs::WriterUnchecked`).

- [ ] **Step 1: Write `src/serialize/writer_unchecked.rs`**

```rust
//! Writing XDR values to a serialization format without runtime checks.
//!
//! This module defines the [`WriterUnchecked`] trait, the infallible, unsafe
//! counterpart to [`Writer`](crate::serialize::writer::Writer). Its methods
//! perform no runtime bounds checking: the caller must guarantee up front
//! that the writer's underlying buffer is large enough for everything
//! written, typically by sizing it with the worst-case bounds computed by
//! `#[derive(ConstWriteXDR)]` (`const_max_cxs_size` / `const_max_txs_size`).
//!
//! Preconditions are checked with `debug_assert!` only, so misuse is caught
//! in debug builds at zero release-mode cost. This is intended for
//! code-size-sensitive targets (e.g. wasm smart contracts) where the
//! `WriteResult` plumbing and per-write bounds checks of
//! [`Writer`](crate::serialize::writer::Writer) are unwanted overhead.

/// Something that can write XDR values to a serialization format, without
/// runtime checks.
///
/// This is the infallible, unsafe counterpart to
/// [`Writer`](crate::serialize::writer::Writer): the same primitive-writing
/// methods, but no method can fail and no method checks that space remains
/// in the underlying buffer.
///
/// # Safety
///
/// Every method shares one contract: the writer's underlying buffer must
/// have enough space remaining for the encoded form of the argument. The
/// opaque methods additionally require `bytes.len() <= u32::MAX`.
/// Violations are `debug_assert!`ed; in release builds they are undefined
/// behavior.
pub trait WriterUnchecked<F> {
    /// Writes a byte sequence of fixed length.
    ///
    /// # Safety
    ///
    /// The underlying buffer must have space for the encoded form, and
    /// `bytes.len()` must not exceed `u32::MAX`.
    #[doc(hidden)]
    unsafe fn write_opaque_fixed(&mut self, bytes: &[u8]);

    /// Writes a byte sequence of variable length.
    ///
    /// # Safety
    ///
    /// The underlying buffer must have space for the encoded form, and
    /// `bytes.len()` must not exceed `u32::MAX`.
    unsafe fn write_opaque_variable(&mut self, bytes: &[u8]);

    /// Writes a signed 64-bit integer.
    ///
    /// # Safety
    ///
    /// The underlying buffer must have space for the encoded form.
    unsafe fn write_i64(&mut self, n: i64);

    /// Writes a signed 32-bit integer.
    ///
    /// # Safety
    ///
    /// The underlying buffer must have space for the encoded form.
    unsafe fn write_i32(&mut self, n: i32);

    /// Writes a signed 16-bit integer.
    ///
    /// # Safety
    ///
    /// The underlying buffer must have space for the encoded form.
    unsafe fn write_i16(&mut self, n: i16);

    /// Writes an unsigned 64-bit integer.
    ///
    /// # Safety
    ///
    /// The underlying buffer must have space for the encoded form.
    unsafe fn write_u64(&mut self, n: u64);

    /// Writes an unsigned 32-bit integer.
    ///
    /// # Safety
    ///
    /// The underlying buffer must have space for the encoded form.
    unsafe fn write_u32(&mut self, n: u32);

    /// Writes an unsigned 16-bit integer.
    ///
    /// # Safety
    ///
    /// The underlying buffer must have space for the encoded form.
    unsafe fn write_u16(&mut self, n: u16);

    /// Writes a boolean value.
    ///
    /// # Safety
    ///
    /// The underlying buffer must have space for the encoded form.
    unsafe fn write_bool(&mut self, b: bool);

    /// Writes a 32-bit IEEE 754 floating-point value.
    ///
    /// # Safety
    ///
    /// The underlying buffer must have space for the encoded form.
    unsafe fn write_f32(&mut self, n: f32);

    /// Writes a 64-bit IEEE 754 floating-point value.
    ///
    /// # Safety
    ///
    /// The underlying buffer must have space for the encoded form.
    unsafe fn write_f64(&mut self, n: f64);
}
```

- [ ] **Step 2: Register and re-export**

In `src/serialize/mod.rs`, after `pub mod writer;`:

```rust
pub mod writer_unchecked;
```

In `src/lib.rs`, after the `pub use serialize::writer::*;` line (line 114):

```rust
pub use serialize::writer_unchecked::*;
```

In the crate docs at the top of `src/lib.rs`, after the bullet "- Traits [`Reader`] and [`Writer`], …" (line 8-9), add:

```rust
//! - Trait [`WriterUnchecked`], an infallible, unsafe counterpart to
//!   [`Writer`] with no runtime checks, for code-size-sensitive targets.
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -F int128`
Expected: clean (no warnings).

- [ ] **Step 4: Commit**

```bash
cargo fmt
git add src/serialize/writer_unchecked.rs src/serialize/mod.rs src/lib.rs
git commit -m "Add WriterUnchecked trait: infallible, unsafe counterpart to Writer"
```

---

### Task 2: TXS unchecked path (`PutUnchecked` + `UncheckedBufWriter`)

**Files:**
- Modify: `src/txs/put.rs` (append `PutUnchecked` + blanket impl)
- Create: `src/txs/unchecked_buf_writer.rs`
- Modify: `src/txs/mod.rs` (register module)

**Interfaces:**
- Consumes: `WriterUnchecked<F>` from Task 1; existing `pub struct TXS` and `mu::write_copy_of_slice`.
- Produces: crate-private `trait PutUnchecked { unsafe fn put_bytes(&mut self, &[u8]); unsafe fn put_opaque_variable(&mut self, &[u8]); unsafe fn put_opaque_fixed(&mut self, &[u8]); }` with blanket `impl<P: PutUnchecked> WriterUnchecked<TXS> for P`; `pub struct UncheckedBufWriter<'a>` with `pub fn new(buf: &'a mut [MaybeUninit<u8>]) -> Self` and `pub fn written(&self) -> usize`. Debug-assert message is exactly `"UncheckedBufWriter: buffer overflow"` (Task 6 matches on it).

- [ ] **Step 1: Write failing unit tests**

Create `src/txs/unchecked_buf_writer.rs` containing ONLY the module doc and the test module for now (so the test drives the implementation):

```rust
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
```

Register in `src/txs/mod.rs` after `pub mod into_buf_writer;`:

```rust
pub mod unchecked_buf_writer;
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -F int128 unchecked_buf_writer`
Expected: FAIL to compile — `UncheckedBufWriter` not found, no `WriterUnchecked<TXS>` impl.

- [ ] **Step 3: Add `PutUnchecked` + blanket impl to `src/txs/put.rs`**

Append at the end of `src/txs/put.rs` (and add `use crate::serialize::writer_unchecked::*;` next to the existing `use crate::serialize::writer::*;`):

```rust
/// The unchecked counterpart to [`Put`]: implement these basic byte
/// operations to get a `WriterUnchecked<TXS>` implementation for free.
pub(crate) trait PutUnchecked {
    /// # Safety
    /// The underlying buffer must have space for `bytes.len()` more bytes.
    unsafe fn put_bytes(&mut self, bytes: &[u8]);

    /// # Safety
    /// The underlying buffer must have space for the 4-byte length prefix
    /// plus the data padded to a 4-byte boundary, and
    /// `bytes.len() <= u32::MAX`.
    unsafe fn put_opaque_variable(&mut self, bytes: &[u8]);

    /// # Safety
    /// The underlying buffer must have space for the data padded to a
    /// 4-byte boundary, and `bytes.len() <= u32::MAX`.
    unsafe fn put_opaque_fixed(&mut self, bytes: &[u8]);
}

macro_rules! write_int_unchecked {
    [ $($write:ident($nbytes:expr): $ty:ty),* $(,)? ] => {

    $(
        #[inline]
        unsafe fn $write(&mut self, n: $ty) {
            let bytes: [u8; $nbytes] = n.to_be_bytes();
            // SAFETY: the caller guarantees space for the encoded form.
            unsafe { self.put_bytes(&bytes) }
        }
    )*

    };
}

impl<P> WriterUnchecked<TXS> for P
where
    P: PutUnchecked,
{
    #[inline]
    unsafe fn write_opaque_fixed(&mut self, bytes: &[u8]) {
        debug_assert!(bytes.len() <= u32::MAX as usize);
        // SAFETY: forwarded caller contract.
        unsafe { self.put_opaque_fixed(bytes) }
    }

    #[inline]
    unsafe fn write_opaque_variable(&mut self, bytes: &[u8]) {
        debug_assert!(bytes.len() <= u32::MAX as usize);
        // SAFETY: forwarded caller contract.
        unsafe { self.put_opaque_variable(bytes) }
    }

    #[inline]
    unsafe fn write_bool(&mut self, v: bool) {
        // SAFETY: forwarded caller contract.
        unsafe { self.write_i32(if v { 1 } else { 0 }) }
    }

    #[inline]
    unsafe fn write_i16(&mut self, n: i16) {
        // SAFETY: forwarded caller contract.
        unsafe { self.write_i32(n as i32) }
    }

    #[inline]
    unsafe fn write_u16(&mut self, n: u16) {
        // SAFETY: forwarded caller contract.
        unsafe { self.write_u32(n as u32) }
    }

    write_int_unchecked![
        write_i64(8): i64,
        write_i32(4): i32,

        write_u64(8): u64,
        write_u32(4): u32,
    ];

    #[inline]
    unsafe fn write_f32(&mut self, n: f32) {
        let bytes: [u8; 4] = n.to_be_bytes();
        // SAFETY: forwarded caller contract.
        unsafe { self.put_bytes(&bytes) }
    }

    #[inline]
    unsafe fn write_f64(&mut self, n: f64) {
        let bytes: [u8; 8] = n.to_be_bytes();
        // SAFETY: forwarded caller contract.
        unsafe { self.put_bytes(&bytes) }
    }
}
```

- [ ] **Step 4: Implement `UncheckedBufWriter` in `src/txs/unchecked_buf_writer.rs`**

Insert between the `type Mu8 = …;` line and the test module:

```rust
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
            mu::write_copy_of_slice(buf.get_unchecked_mut(4 + len..size), PAD.get_unchecked(0..rem));
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
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -F int128 unchecked_buf_writer`
Expected: 4 tests PASS (3 value tests + 1 should_panic).

- [ ] **Step 6: Commit**

```bash
cargo fmt
git add src/txs/put.rs src/txs/unchecked_buf_writer.rs src/txs/mod.rs
git commit -m "Add TXS UncheckedBufWriter and PutUnchecked with WriterUnchecked blanket impl"
```

---

### Task 3: CXS unchecked path (`PutUnchecked` + `UncheckedBufWriter`)

**Files:**
- Modify: `src/cxs/put.rs` (append `PutUnchecked` + blanket impl + unchecked u8/i8 helpers)
- Create: `src/cxs/unchecked_buf_writer.rs`
- Modify: `src/cxs/mod.rs` (register module)

**Interfaces:**
- Consumes: `WriterUnchecked<F>` from Task 1; existing `pub struct CXS`, header constants (`V_MULTIBYTE`, `S_8BYTE`, `S_4BYTE`, `S_2BYTE`, `S_1BYTE`, `S_HEADER`), `limits::{IH_MIN, IH_MAX, UH_MAX}`, `mu::write_copy_of_slice`, and `crate::cxs::encode_into` (for differential unit tests).
- Produces: crate-private `trait PutUnchecked { unsafe fn put_byte(&mut self, u8); unsafe fn put_bytes(&mut self, &[u8]); }` with blanket `impl<P: PutUnchecked> WriterUnchecked<CXS> for P`; `pub struct UncheckedBufWriter<'a>` with `new`/`written` (same shape as Task 2's). Same debug-assert message `"UncheckedBufWriter: buffer overflow"`.

- [ ] **Step 1: Write failing unit tests**

Create `src/cxs/unchecked_buf_writer.rs` with module doc, imports, and tests only. The tests are differential against the checked CXS writer, exercising every integer-width branch of the CXS compression logic:

```rust
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serialize::writer_unchecked::WriterUnchecked;

    // Writes `v` with both the checked and unchecked CXS writers and
    // asserts identical output.
    fn check<T: crate::WriteXDR>(
        v: &T,
        write_unchecked: impl FnOnce(&mut UncheckedBufWriter),
    ) {
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
        for n in [0i32, 31, -31, 32, -32, 127, -128, 128, -129, i32::MAX, i32::MIN] {
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
        check(&true, |w| unsafe { WriterUnchecked::<CXS>::write_bool(w, true) });
        check(&1.5f32, |w| unsafe { WriterUnchecked::<CXS>::write_f32(w, 1.5) });
        check(&1.5f64, |w| unsafe { WriterUnchecked::<CXS>::write_f64(w, 1.5) });
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
```

Register in `src/cxs/mod.rs` after `pub mod into_buf_writer;`:

```rust
pub mod unchecked_buf_writer;
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -F int128 cxs::unchecked_buf_writer`
Expected: FAIL to compile — `UncheckedBufWriter` not found, no `WriterUnchecked<CXS>` impl.

- [ ] **Step 3: Add `PutUnchecked` + blanket impl to `src/cxs/put.rs`**

Add `use crate::serialize::writer_unchecked::*;` next to the existing `use crate::serialize::writer::*;`, then append at the end of the file. The bodies replicate the checked `Writer<CXS>` impl's branch structure exactly — same constants, same width-selection conditions — minus the `Result`s:

```rust
/// The unchecked counterpart to [`Put`]: implement these basic byte
/// operations to get a `WriterUnchecked<CXS>` implementation for free.
pub(crate) trait PutUnchecked {
    /// # Safety
    /// The underlying buffer must have space for 1 more byte.
    unsafe fn put_byte(&mut self, b: u8);

    /// # Safety
    /// The underlying buffer must have space for `bytes.len()` more bytes.
    unsafe fn put_bytes(&mut self, bytes: &[u8]);
}

impl<P> WriterUnchecked<CXS> for P
where
    P: PutUnchecked,
{
    #[inline]
    unsafe fn write_opaque_fixed(&mut self, bytes: &[u8]) {
        debug_assert!(bytes.len() <= u32::MAX as usize);
        // SAFETY: forwarded caller contract.
        unsafe { self.put_bytes(bytes) }
    }

    unsafe fn write_opaque_variable(&mut self, bytes: &[u8]) {
        debug_assert!(bytes.len() <= u32::MAX as usize);
        // SAFETY: forwarded caller contract.
        unsafe {
            self.write_u32(bytes.len() as u32);
            self.put_bytes(bytes);
        }
    }

    #[inline]
    unsafe fn write_bool(&mut self, v: bool) {
        // SAFETY: forwarded caller contract.
        unsafe { self.write_i32(if v { 1 } else { 0 }) }
    }

    #[inline]
    unsafe fn write_i64(&mut self, n: i64) {
        if n > i32::MAX as i64 || n < i32::MIN as i64 {
            // Use the 8-byte representation
            let h = V_MULTIBYTE | S_8BYTE;
            let b = n.to_le_bytes();
            let combined = [h, b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]];
            // SAFETY: forwarded caller contract.
            unsafe { self.put_bytes(&combined) }
        } else {
            // SAFETY: forwarded caller contract.
            unsafe { self.write_i32(n as i32) }
        }
    }

    #[inline]
    unsafe fn write_i32(&mut self, n: i32) {
        if n > i16::MAX as i32 || n < i16::MIN as i32 {
            // use 4-byte repr
            let h = V_MULTIBYTE | S_4BYTE;
            let b = n.to_le_bytes();
            let combined = [h, b[0], b[1], b[2], b[3]];
            // SAFETY: forwarded caller contract.
            unsafe { self.put_bytes(&combined) }
        } else {
            // SAFETY: forwarded caller contract.
            unsafe { self.write_i16(n as i16) }
        }
    }

    #[inline]
    unsafe fn write_i16(&mut self, n: i16) {
        if n > i8::MAX as i16 || n < i8::MIN as i16 {
            // use 2-byte repr
            let h = V_MULTIBYTE | S_2BYTE;
            let b = n.to_le_bytes();
            let combined = [h, b[0], b[1]];
            // SAFETY: forwarded caller contract.
            unsafe { self.put_bytes(&combined) }
        } else {
            // SAFETY: forwarded caller contract.
            unsafe { write_i8_unchecked(self, n as i8) }
        }
    }

    #[inline]
    unsafe fn write_u64(&mut self, n: u64) {
        if n > u32::MAX as u64 {
            // Use 8-byte representation
            let h = V_MULTIBYTE | S_8BYTE;
            let b = n.to_le_bytes();
            let combined = [h, b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]];
            // SAFETY: forwarded caller contract.
            unsafe { self.put_bytes(&combined) }
        } else {
            // SAFETY: forwarded caller contract.
            unsafe { self.write_u32(n as u32) }
        }
    }

    #[inline]
    unsafe fn write_u32(&mut self, n: u32) {
        if n > u16::MAX as u32 {
            // Use 4-byte representation
            let h = V_MULTIBYTE | S_4BYTE;
            let b = n.to_le_bytes();
            let combined = [h, b[0], b[1], b[2], b[3]];
            // SAFETY: forwarded caller contract.
            unsafe { self.put_bytes(&combined) }
        } else {
            // SAFETY: forwarded caller contract.
            unsafe { self.write_u16(n as u16) }
        }
    }

    #[inline]
    unsafe fn write_u16(&mut self, n: u16) {
        if n > u8::MAX as u16 {
            // Use 2-byte representation
            let h = V_MULTIBYTE | S_2BYTE;
            let b = n.to_le_bytes();
            let combined = [h, b[0], b[1]];
            // SAFETY: forwarded caller contract.
            unsafe { self.put_bytes(&combined) }
        } else {
            // SAFETY: forwarded caller contract.
            unsafe { write_u8_unchecked(self, n as u8) }
        }
    }

    /// Writes a 32-bit IEEE 754 float as raw little-endian bytes,
    /// mirroring the checked CXS writer.
    #[inline]
    unsafe fn write_f32(&mut self, n: f32) {
        let bytes: [u8; 4] = n.to_le_bytes();
        // SAFETY: forwarded caller contract.
        unsafe { self.put_bytes(&bytes) }
    }

    /// Writes a 64-bit IEEE 754 double as raw little-endian bytes,
    /// mirroring the checked CXS writer.
    #[inline]
    unsafe fn write_f64(&mut self, n: f64) {
        let bytes: [u8; 8] = n.to_le_bytes();
        // SAFETY: forwarded caller contract.
        unsafe { self.put_bytes(&bytes) }
    }
}

/// # Safety
/// The underlying buffer must have space for the encoded form (1-2 bytes).
#[inline]
unsafe fn write_u8_unchecked<P>(p: &mut P, n: u8)
where
    P: PutUnchecked,
{
    if n > UH_MAX {
        // Use 1-additional-byte representation
        let h = V_MULTIBYTE | S_1BYTE;
        let combined = [h, n];
        // SAFETY: forwarded caller contract.
        unsafe { p.put_bytes(&combined) }
    } else {
        // Use header-byte-only representation
        // SAFETY: forwarded caller contract.
        unsafe { p.put_byte(n) }
    }
}

/// # Safety
/// The underlying buffer must have space for the encoded form (1-2 bytes).
#[inline]
unsafe fn write_i8_unchecked<P>(p: &mut P, n: i8)
where
    P: PutUnchecked,
{
    if !(IH_MIN..=IH_MAX).contains(&n) {
        // Use the 1-byte repr
        let h = V_MULTIBYTE | S_1BYTE;
        let combined = [h, n as u8];
        // SAFETY: forwarded caller contract.
        unsafe { p.put_bytes(&combined) }
    } else {
        // Represent this small, signed number in the header-byte with 6-bit,
        // one's-complement encoding
        let nu: u8 = n.unsigned_abs();
        let v = if n >= 0 {
            nu
        } else {
            // invert and set the high bit
            (!nu & 0b01_1111) | 0b0010_0000
        };

        let s = S_HEADER;
        let h = v | s;
        // SAFETY: forwarded caller contract.
        unsafe { p.put_byte(h) }
    }
}
```

- [ ] **Step 4: Implement `UncheckedBufWriter` in `src/cxs/unchecked_buf_writer.rs`**

Insert between the `type Mu8 = …;` line and the test module:

```rust
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
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -F int128 cxs::unchecked_buf_writer`
Expected: 5 tests PASS.

- [ ] **Step 6: Commit**

```bash
cargo fmt
git add src/cxs/put.rs src/cxs/unchecked_buf_writer.rs src/cxs/mod.rs
git commit -m "Add CXS UncheckedBufWriter and PutUnchecked with WriterUnchecked blanket impl"
```

---

### Task 4: `WriteXDR::write_xdr_unchecked` — trait method, all impls, derive

This task is atomic: adding a required trait method breaks every impl until all of them (including derive-generated ones) are updated. One commit at the end.

**Files:**
- Modify: `src/write_xdr.rs` (trait method, free fn, all impls, unit test)
- Modify: `src/int128.rs` (i128/u128 impls)
- Modify: `src/heapless.rs` (heapless Vec/String impls)
- Modify: `xdr-derive/src/generate.rs` (emit `write_xdr_unchecked` in derived impls)

**Interfaces:**
- Consumes: `WriterUnchecked<F>` (Task 1); `UncheckedBufWriter` (Tasks 2-3, for the unit test).
- Produces: `unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W)` as a required method on `WriteXDR`; free fn `pub unsafe fn write_xdr_unchecked<T: WriteXDR, F, W: WriterUnchecked<F>>(this: &T, w: &mut W)` in `write_xdr.rs` (re-exported at crate root). Debug-assert messages `"BytesM exceeds declared bound"`, `"VecM exceeds declared bound"`, `"StringM exceeds declared bound"` (Task 6 matches on `"exceeds declared bound"`).

- [ ] **Step 1: Add trait method, free fn, and a failing unit test to `src/write_xdr.rs`**

Add import at the top, next to `use crate::serialize::writer::*;`:

```rust
use crate::serialize::writer_unchecked::*;
```

Extend the `WriteXDR` trait (keep `write_xdr` as-is, add below it):

```rust
    /// Writes this value as XDR data to the writer, without runtime checks.
    ///
    /// This is the infallible, unsafe counterpart to
    /// [`write_xdr`](Self::write_xdr), for use with
    /// [`WriterUnchecked`] writers on code-size-sensitive targets.
    ///
    /// # Safety
    ///
    /// - The writer's underlying buffer must have enough space remaining
    ///   for the encoded form of `self`.
    /// - `self` must respect all declared XDR bounds (e.g. a
    ///   `VecM<T, MAX>` must not be longer than `MAX`), since a violation
    ///   would make the encoding exceed any precomputed size bound.
    ///
    /// Violations are `debug_assert!`ed; in release builds they are
    /// undefined behavior.
    unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W);
```

Add the free fn after the existing `pub fn write_xdr` (line 52-58):

```rust
/// Writes an XDR value to the writer, without runtime checks.
///
/// This is a convenience function that calls
/// [`WriteXDR::write_xdr_unchecked`] on the value.
///
/// # Safety
///
/// Same contract as [`WriteXDR::write_xdr_unchecked`]: the writer must have
/// space for the encoded form of `this`, and `this` must respect all
/// declared XDR bounds.
#[inline]
pub unsafe fn write_xdr_unchecked<T, F, W>(this: &T, w: &mut W)
where
    T: WriteXDR,
    W: WriterUnchecked<F>,
{
    // SAFETY: forwarded caller contract.
    unsafe { this.write_xdr_unchecked(w) }
}
```

Add the unit test at the end of `src/write_xdr.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::MaybeUninit;

    #[test]
    fn tuple_unchecked_writes_txs() {
        let mut buf = [MaybeUninit::uninit(); 8];
        let mut w = crate::txs::unchecked_buf_writer::UncheckedBufWriter::new(&mut buf);
        // SAFETY: two u32s occupy exactly the 8-byte buffer.
        unsafe { (1u32, 2u32).write_xdr_unchecked(&mut w) };
        assert_eq!(w.written(), 8);
        // SAFETY: the first 8 bytes were just initialized.
        let bytes = unsafe { crate::mu::assume_init_ref(&buf[..8]) };
        assert_eq!(bytes, [0, 0, 0, 1, 0, 0, 0, 2]);
    }
}
```

- [ ] **Step 2: Verify it fails**

Run: `cargo check -F int128`
Expected: FAIL — every `impl WriteXDR` is missing `write_xdr_unchecked`.

- [ ] **Step 3: Mirror every impl in `src/write_xdr.rs`**

Each existing impl gains the unchecked method. Complete list — each block shows the method to add inside the named impl:

In the `write_xdr_int_impls!` macro, add to the generated impl (after the checked method):

```rust
            #[inline]
            unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
                // SAFETY: forwarded caller contract.
                unsafe { w.$write_method(*self) }
            }
```

`impl WriteXDR for [u8]`:

```rust
    unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
        // SAFETY: forwarded caller contract.
        unsafe { w.write_opaque_variable(self) }
    }
```

`impl WriteXDR for &[u8]`:

```rust
    #[inline]
    unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
        // SAFETY: forwarded caller contract.
        unsafe { w.write_opaque_variable(self) }
    }
```

`impl<const N: usize> WriteXDR for [u8; N]`:

```rust
    #[inline]
    unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
        let () = ValidU32::<N>::OK;

        // SAFETY: forwarded caller contract.
        unsafe { w.write_opaque_fixed(self) }
    }
```

`impl<T: WriteXDR, const N: usize> WriteXDR for [T; N]`:

```rust
    unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
        let () = ValidU32::<N>::OK;

        for elt in self {
            // SAFETY: the caller guarantees space for every element.
            unsafe { elt.write_xdr_unchecked(w) };
        }
    }
```

`impl<T: WriteXDR> WriteXDR for [T]` (the length-vs-`u32::MAX` check becomes a debug_assert):

```rust
    unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
        debug_assert!(self.len() <= u32::MAX as usize);
        // SAFETY: the caller guarantees space for the length prefix and
        // every element.
        unsafe {
            w.write_u32(self.len() as u32);
            for elt in self {
                elt.write_xdr_unchecked(w);
            }
        }
    }
```

`impl<T: WriteXDR> WriteXDR for &T`:

```rust
    #[inline]
    unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
        // SAFETY: forwarded caller contract.
        unsafe { (*self).write_xdr_unchecked(w) }
    }
```

`impl<T: WriteXDR> WriteXDR for &mut T`:

```rust
    #[inline]
    unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
        // SAFETY: forwarded caller contract.
        unsafe { (**self).write_xdr_unchecked(w) }
    }
```

`impl<T: WriteXDR> WriteXDR for &[T]`:

```rust
    #[inline(always)]
    unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
        // SAFETY: forwarded caller contract.
        unsafe { <[T] as WriteXDR>::write_xdr_unchecked(*self, w) }
    }
```

`impl WriteXDR for str`:

```rust
    unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
        // SAFETY: forwarded caller contract.
        unsafe { w.write_opaque_variable(self.as_bytes()) }
    }
```

`impl WriteXDR for &str`:

```rust
    unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
        // SAFETY: forwarded caller contract.
        unsafe { w.write_opaque_variable((*self).as_bytes()) }
    }
```

`impl WriteXDR for alloc::string::String`:

```rust
    unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
        // SAFETY: forwarded caller contract.
        unsafe { self.as_str().write_xdr_unchecked(w) }
    }
```

`impl WriteXDR for ()`:

```rust
    unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, _w: &mut W) {}
```

`impl WriteXDR for Infallible` (uninhabited — no panic machinery needed):

```rust
    unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, _w: &mut W) {
        match *self {}
    }
```

In the `write_tuple!` macro, add to the generated impl:

```rust
            unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
                let ( $($v),+ ) = self;
                // SAFETY: the caller guarantees space for the whole tuple.
                unsafe { $( write_xdr_unchecked($v, w); )+ }
            }
```

`impl<T: WriteXDR> WriteXDR for (T,)`:

```rust
    unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
        let (t,) = self;
        // SAFETY: forwarded caller contract.
        unsafe { t.write_xdr_unchecked(w) }
    }
```

`impl<T: WriteXDR> WriteXDR for Option<T>`:

```rust
    unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
        // SAFETY: the caller guarantees space for the tag and the payload.
        unsafe {
            if let Some(val) = self {
                w.write_i32(1);
                val.write_xdr_unchecked(w);
            } else {
                w.write_i32(0);
            }
        }
    }
```

Inside the `const _: () = { … }` block for xdr_common types, add to the inner imports:

```rust
    use crate::serialize::writer_unchecked::WriterUnchecked;
```

`impl<const MAX: u32> WriteXDR for BytesM<MAX>`:

```rust
        unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
            debug_assert!(
                self.0.len() <= MAX as usize,
                "BytesM exceeds declared bound"
            );
            // SAFETY: the caller guarantees the value respects its declared
            // bound and that the writer has space for the encoded form.
            unsafe { self.0.write_xdr_unchecked(w) }
        }
```

`impl<const MAX: u32> WriteXDR for VecM<u8, MAX>` (the `alloc` version):

```rust
        unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
            debug_assert!(
                self.0.len() <= MAX as usize,
                "VecM exceeds declared bound"
            );
            // SAFETY: the caller guarantees the value respects its declared
            // bound and that the writer has space for the encoded form.
            unsafe { self.0.write_xdr_unchecked(w) }
        }
```

`impl<const MAX: u32> WriteXDR for VecM<u8, MAX>` (the `not(alloc)` version):

```rust
        unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
            debug_assert!(self.len() <= MAX as usize, "VecM exceeds declared bound");
            // SAFETY: the caller guarantees the value respects its declared
            // bound and that the writer has space for the encoded form.
            unsafe { <Self as AsRef<[u8]>>::as_ref(self).write_xdr_unchecked(w) }
        }
```

`impl<const MAX: u32> WriteXDR for StringM<MAX>`:

```rust
        unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
            debug_assert!(
                self.0.len() <= MAX as usize,
                "StringM exceeds declared bound"
            );
            // SAFETY: the caller guarantees the value respects its declared
            // bound and that the writer has space for the encoded form.
            unsafe { self.0.write_xdr_unchecked(w) }
        }
```

`impl<const MAX: u32, T> WriteXDR for VecM<T, MAX>` (both the `alloc` and `not(alloc)` versions get the identical body):

```rust
        unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
            debug_assert!(self.len() <= MAX as usize, "VecM exceeds declared bound");
            // SAFETY: the caller guarantees the value respects its declared
            // bound and that the writer has space for the encoded form.
            unsafe { <Self as AsRef<[T]>>::as_ref(self).write_xdr_unchecked(w) }
        }
```

`impl<T: WriteXDR> WriteXDR for xdr_common::Box<T>`:

```rust
        unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
            // SAFETY: forwarded caller contract.
            unsafe { self.as_ref().write_xdr_unchecked(w) }
        }
```

`impl<T, E> WriteXDR for Result<T, E>`:

```rust
    unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
        // SAFETY: the caller guarantees space for the tag and the payload.
        unsafe {
            match self {
                Ok(x) => {
                    w.write_i32(0);
                    x.write_xdr_unchecked(w);
                }
                Err(e) => {
                    w.write_i32(1);
                    e.write_xdr_unchecked(w);
                }
            }
        }
    }
```

`impl<T> WriteXDR for core::marker::PhantomData<T>`:

```rust
    unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, _w: &mut W) {}
```

Inside the `#[cfg(feature = "alloc")] const _: () = { … }` block, `impl WriteXDR for Vec<u8>`:

```rust
        unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
            // SAFETY: forwarded caller contract.
            unsafe { w.write_opaque_variable(self.as_ref()) }
        }
```

and `impl<T> WriteXDR for Vec<T>`:

```rust
        unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
            let slice: &[T] = self.as_ref();
            // SAFETY: forwarded caller contract.
            unsafe { slice.write_xdr_unchecked(w) }
        }
```

- [ ] **Step 4: Mirror `src/int128.rs`**

In the second `const _: () = { … }` block, add to the inner imports:

```rust
    use crate::serialize::writer_unchecked::*;
```

Add to `impl WriteXDR for i128`:

```rust
        #[inline]
        unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
            let highbits = (*self >> 64) as i64;
            let lowbits = (*self & 0xffffffffffffffff) as u64;

            // SAFETY: the caller guarantees space for both halves.
            unsafe {
                write_xdr_unchecked(&highbits, w);
                write_xdr_unchecked(&lowbits, w);
            }
        }
```

Add to `impl WriteXDR for u128` (same body with `u64` highbits):

```rust
        #[inline]
        unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
            let highbits = (*self >> 64) as u64;
            let lowbits = (*self & 0xffffffffffffffff) as u64;

            // SAFETY: the caller guarantees space for both halves.
            unsafe {
                write_xdr_unchecked(&highbits, w);
                write_xdr_unchecked(&lowbits, w);
            }
        }
```

- [ ] **Step 5: Mirror `src/heapless.rs`**

Add import next to the existing writer import:

```rust
use crate::serialize::writer_unchecked::WriterUnchecked;
```

Add to `impl<const N: usize> WriteXDR for Vec<u8, N>`:

```rust
    unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
        let () = ValidU32::<N>::OK;

        // SAFETY: a heapless Vec<u8, N> never exceeds N <= u32::MAX
        // elements; the caller guarantees buffer space.
        unsafe { w.write_opaque_variable(self.as_slice()) }
    }
```

Add to `impl<T, const N: usize> WriteXDR for Vec<T, N>`:

```rust
    unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
        let () = ValidU32::<N>::OK;

        // SAFETY: the caller guarantees space for the length prefix and
        // every element.
        unsafe {
            w.write_u32(self.len() as u32);
            for elt in self {
                elt.write_xdr_unchecked(w);
            }
        }
    }
```

Add to `impl<const N: usize> WriteXDR for String<N>`:

```rust
    unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
        let () = ValidU32::<N>::OK;

        // SAFETY: a heapless String<N> never exceeds N <= u32::MAX bytes;
        // the caller guarantees buffer space.
        unsafe { w.write_opaque_variable(self.as_bytes()) }
    }
```

- [ ] **Step 6: Verify the lib compiles and its tests pass (derive still red)**

Run: `cargo test -F int128 && cargo test -F int128,heapless,alloc`
Expected: PASS (including the new `tuple_unchecked_writes_txs`).

Run: `cargo test -F derive,int128 --no-run`
Expected: FAIL — derived `WriteXDR` impls in tests are missing `write_xdr_unchecked`. This is the red for the derive step.

- [ ] **Step 7: Extend the derive in `xdr-derive/src/generate.rs`**

In `write_xdr()` (line 10), add to the `mod __xdrs` re-exports:

```rust
                pub use #krate::serialize::writer_unchecked::WriterUnchecked;
```

In `write_into_type()` (line 41), add the method to the generated impl after `write_xdr`:

```rust
            unsafe fn write_xdr_unchecked<F, W: __xdrs::WriterUnchecked<F>>(&self, w: &mut W) {
                unsafe {
                    <#into_type as __xdrs::WriteXDR>::write_xdr_unchecked(
                        &::core::convert::Into::into(::core::clone::Clone::clone(self)),
                        w,
                    )
                }
            }
```

In `write_struct()` (line 58), collect a second field list and add the method. The `#[allow(unused_unsafe)]` covers unit structs, whose generated `unsafe {}` block contains no unsafe operation:

```rust
fn write_struct(input: &syn::DeriveInput, item: &syn::DataStruct) -> Result<TokenStream> {
    let ty_ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let write_fields = item
        .fields
        .iter()
        .enumerate()
        .map(|(i, f)| write_field(i, f))
        .collect::<Result<Vec<_>>>()?;

    let write_fields_unchecked = item
        .fields
        .iter()
        .enumerate()
        .map(|(i, f)| write_field_unchecked(i, f))
        .collect::<Result<Vec<_>>>()?;

    Ok(quote! {
        impl #impl_generics __xdrs::WriteXDR for #ty_ident #ty_generics #where_clause {
            fn write_xdr<F, W: __xdrs::Writer<F>>(&self, w: &mut W) -> __xdrs::WriteResult<()> {
                #(#write_fields)*
                Ok(())
            }

            #[allow(unused_unsafe)]
            unsafe fn write_xdr_unchecked<F, W: __xdrs::WriterUnchecked<F>>(&self, w: &mut W) {
                unsafe {
                    #(#write_fields_unchecked)*
                }
            }
        }
    })
}
```

Add `write_field_unchecked` after `write_field` (line 78-96) — identical structure, unchecked call, no `?`:

```rust
fn write_field_unchecked(index: usize, field: &syn::Field) -> Result<TokenStream> {
    let ty = &field.ty;
    let fa = FieldAccess::new(index, field);
    let attrs = FieldAttrs::from_field(field)?;
    attrs.check(field)?;

    if let Some(into_type) = attrs.into_type.as_ref().or(attrs.via.as_ref()) {
        Ok(quote! {
            <#into_type as __xdrs::WriteXDR>::write_xdr_unchecked(
                &::core::convert::Into::into(::core::clone::Clone::clone(&self.#fa)),
                w,
            );
        })
    } else {
        Ok(quote! {
            <#ty as __xdrs::WriteXDR>::write_xdr_unchecked(&self.#fa, w);
        })
    }
}
```

In `write_enum()` (line 98), collect unchecked variant arms and add the method (`#[allow(unused_unsafe)]` covers uninhabited enums, whose match has no arms):

```rust
    let write_variants_unchecked: Vec<_> = item
        .variants
        .iter()
        .enumerate()
        .map(|(i, v)| write_variant_unchecked(i, v, input))
        .collect::<Result<Vec<_>>>()?;
```

and in the generated impl, after `write_xdr`:

```rust
            #[allow(unused_unsafe)]
            unsafe fn write_xdr_unchecked<F, W: __xdrs::WriterUnchecked<F>>(&self, __write_xdr_writer: &mut W) {
                unsafe {
                    match self {
                        #(#write_variants_unchecked)*
                    }
                }
            }
```

Add `write_variant_unchecked` and `write_binding_unchecked` after their checked counterparts (lines 158-219) — same discriminant logic, no `?`:

```rust
fn write_variant_unchecked(
    index: usize,
    v: &syn::Variant,
    input: &syn::DeriveInput,
) -> Result<TokenStream> {
    let is_default_arm = variant_is_default(v);

    let var_ident = &v.ident;
    let ty_ident = &input.ident;

    let var_bindings = var_bindings(&v.fields);
    let write_bindings: Vec<_> = v
        .fields
        .iter()
        .enumerate()
        .map(|(i, field)| {
            if is_default_arm && i == 0 {
                Ok(quote! {})
            } else {
                write_binding_unchecked(i, field)
            }
        })
        .collect::<Result<Vec<_>>>()?;

    let variant_idx = index as i32;
    let discriminant = if let Some((_, d)) = v.discriminant.as_ref() {
        // An explicit-discriminant variant should never be considered a default arm
        assert!(!is_default_arm);

        quote! { #d }
    } else if is_default_arm {
        // Use the default-arm value as the discriminant
        quote! { *_self_0 }
    } else {
        // Use the zero-based index
        quote! { #variant_idx }
    };

    Ok(quote! {
        #ty_ident::#var_ident #var_bindings => {
            __write_xdr_writer.write_i32(#discriminant);

            #(#write_bindings)*
        }
    })
}

fn write_binding_unchecked(index: usize, field: &syn::Field) -> Result<TokenStream> {
    let ty = &field.ty;
    let bind = tmp_binding(index);
    let attrs = FieldAttrs::from_field(field)?;
    attrs.check(field)?;

    if let Some(into_type) = attrs.into_type.as_ref().or(attrs.via.as_ref()) {
        Ok(quote! {
            <#into_type as __xdrs::WriteXDR>::write_xdr_unchecked(
                &::core::convert::Into::into(::core::clone::Clone::clone(#bind)),
                __write_xdr_writer,
            );
        })
    } else {
        Ok(quote! {
            <#ty as __xdrs::WriteXDR>::write_xdr_unchecked(#bind, __write_xdr_writer);
        })
    }
}
```

- [ ] **Step 8: Verify everything is green**

Run: `cargo test -F derive,int128`
Expected: PASS.

Run: `cargo test --workspace --all-features`
Expected: PASS.

- [ ] **Step 9: Commit**

```bash
cargo fmt
git add src/write_xdr.rs src/int128.rs src/heapless.rs xdr-derive/src/generate.rs
git commit -m "Add WriteXDR::write_xdr_unchecked to trait, all impls, and derive"
```

---

### Task 5: `encode_into_unchecked` entry points

**Files:**
- Modify: `src/txs/utils.rs`
- Modify: `src/cxs/utils.rs`

**Interfaces:**
- Consumes: `UncheckedBufWriter` (Tasks 2-3), `WriteXDR::write_xdr_unchecked` (Task 4).
- Produces: `pub unsafe fn txs::encode_into_unchecked<T: WriteXDR + ?Sized>(value: &T, buf: &mut [MU<u8>]) -> usize` and the identical `cxs::` function.

- [ ] **Step 1: Add `txs::encode_into_unchecked` with doctest**

In `src/txs/utils.rs`, add import next to the existing ones:

```rust
use crate::txs::unchecked_buf_writer::*;
```

Add after `encode_into` (line 63-72):

```rust
/// Encodes a value as TXS-encoded XDR data into a pre-allocated buffer,
/// without runtime checks.
///
/// This is the infallible, unsafe counterpart to [`encode_into`]: no bounds
/// checking is performed and no error can be returned, minimizing code size
/// on constrained targets. Returns the number of bytes written.
///
/// # Safety
///
/// - `buf.len()` must be at least the encoded size of `value`. With
///   `#[derive(ConstWriteXDR)]`, `const_max_txs_size()` (worst case over
///   all values) or `const_txs_size()` (exact for this value) provide
///   suitable sizes.
/// - `value` must respect all declared XDR bounds (e.g. a `VecM<T, MAX>`
///   must not be longer than `MAX`), since a violation would make the
///   encoding exceed any precomputed size bound.
///
/// Violations are caught by `debug_assert!` in debug builds; in release
/// builds they are undefined behavior.
///
/// # Example
///
/// ```rust
/// use xdrs::txs;
/// use core::mem::MaybeUninit;
///
/// let value: u32 = 42;
///
/// let mut buf = [MaybeUninit::uninit(); 4];
///
/// // SAFETY: a u32 always encodes to exactly 4 TXS bytes.
/// let written = unsafe { txs::encode_into_unchecked(&value, &mut buf) };
/// assert_eq!(written, 4);
/// ```
pub unsafe fn encode_into_unchecked<T>(value: &T, buf: &mut [MU<u8>]) -> usize
where
    T: WriteXDR + ?Sized,
{
    let mut writer = UncheckedBufWriter::new(buf);

    // SAFETY: the caller guarantees buf can hold the encoded form of value
    // and that value respects its declared bounds.
    unsafe { value.write_xdr_unchecked(&mut writer) };

    writer.written()
}
```

- [ ] **Step 2: Add `cxs::encode_into_unchecked` with doctest**

In `src/cxs/utils.rs`, add import:

```rust
use crate::cxs::unchecked_buf_writer::*;
```

Add after `encode_into`:

```rust
/// Encodes a value as CXS-encoded XDR data into a pre-allocated buffer,
/// without runtime checks.
///
/// This is the infallible, unsafe counterpart to [`encode_into`]: no bounds
/// checking is performed and no error can be returned, minimizing code size
/// on constrained targets. Returns the number of bytes written.
///
/// # Safety
///
/// - `buf.len()` must be at least the encoded size of `value`. With
///   `#[derive(ConstWriteXDR)]`, `const_max_cxs_size()` (worst case over
///   all values) or `const_cxs_size()` (exact for this value) provide
///   suitable sizes.
/// - `value` must respect all declared XDR bounds (e.g. a `VecM<T, MAX>`
///   must not be longer than `MAX`), since a violation would make the
///   encoding exceed any precomputed size bound.
///
/// Violations are caught by `debug_assert!` in debug builds; in release
/// builds they are undefined behavior.
///
/// # Example
///
/// ```rust
/// use xdrs::cxs;
/// use core::mem::MaybeUninit;
///
/// let value: u32 = 300;
///
/// // Worst-case CXS size of a u32 is 5 bytes.
/// let mut buf = [MaybeUninit::uninit(); 5];
///
/// // SAFETY: the buffer is sized to the worst case.
/// let written = unsafe { cxs::encode_into_unchecked(&value, &mut buf) };
/// assert_eq!(written, 3); // 300 takes the 2-byte repr plus a header byte
/// ```
pub unsafe fn encode_into_unchecked<T>(value: &T, buf: &mut [MU<u8>]) -> usize
where
    T: WriteXDR + ?Sized,
{
    let mut writer = UncheckedBufWriter::new(buf);

    // SAFETY: the caller guarantees buf can hold the encoded form of value
    // and that value respects its declared bounds.
    unsafe { value.write_xdr_unchecked(&mut writer) };

    writer.written()
}
```

- [ ] **Step 3: Run the doctests**

Run: `cargo test --doc --features "alloc,derive,repr,ns,int128"`
Expected: PASS, including the two new doctests.

- [ ] **Step 4: Commit**

```bash
cargo fmt
git add src/txs/utils.rs src/cxs/utils.rs
git commit -m "Add txs/cxs encode_into_unchecked entry points"
```

---

### Task 6: Differential integration tests

**Files:**
- Create: `tests/unchecked_writer.rs`
- Modify: `Cargo.toml` (add `[[test]]` block)

**Interfaces:**
- Consumes: `txs::encode_into_unchecked` / `cxs::encode_into_unchecked` (Task 5), `#[derive(WriteXDR)]`/`#[derive(ConstWriteXDR)]`, `mu::assume_init_ref`, debug-assert messages from Tasks 2 and 4.
- Produces: nothing consumed later; the regression suite.

- [ ] **Step 1: Add the `[[test]]` block to `Cargo.toml`**

After the `const_size_bounds` block (line 131-133):

```toml
[[test]]
name = "unchecked_writer"
required-features = ["alloc", "derive"]
```

- [ ] **Step 2: Write `tests/unchecked_writer.rs`**

```rust
//! Differential tests for the unchecked write path: for every value, the
//! unchecked writers must produce byte-identical output to the checked
//! writers, in both formats. Also exercises the debug_assert guards on
//! contract violations (debug builds only).
#![allow(dead_code)]

use core::mem::MaybeUninit as MU;
use xdrs::{cxs, mu, txs, BytesM, StringM, VecM, WriteXDR};

fn assert_unchecked_matches_checked<T: WriteXDR>(value: &T) {
    // TXS
    let mut cbuf = [MU::uninit(); 512];
    let clen = txs::encode_into(value, &mut cbuf).unwrap();
    let mut ubuf = [MU::uninit(); 512];
    // SAFETY: the checked encode just confirmed the value fits in 512
    // bytes, and all test values respect their declared bounds.
    let ulen = unsafe { txs::encode_into_unchecked(value, &mut ubuf) };
    assert_eq!(clen, ulen, "TXS length mismatch");
    // SAFETY: both writers initialized their first `len` bytes.
    let (c, u) = unsafe {
        (
            mu::assume_init_ref(&cbuf[..clen]),
            mu::assume_init_ref(&ubuf[..ulen]),
        )
    };
    assert_eq!(c, u, "TXS bytes mismatch");

    // CXS
    let mut cbuf = [MU::uninit(); 512];
    let clen = cxs::encode_into(value, &mut cbuf).unwrap();
    let mut ubuf = [MU::uninit(); 512];
    // SAFETY: as above.
    let ulen = unsafe { cxs::encode_into_unchecked(value, &mut ubuf) };
    assert_eq!(clen, ulen, "CXS length mismatch");
    // SAFETY: both writers initialized their first `len` bytes.
    let (c, u) = unsafe {
        (
            mu::assume_init_ref(&cbuf[..clen]),
            mu::assume_init_ref(&ubuf[..ulen]),
        )
    };
    assert_eq!(c, u, "CXS bytes mismatch");
}

// Same shape as tests/const_size_bounds.rs Primitives — known to support
// both WriteXDR and ConstWriteXDR (with max bounds).
#[derive(xdrs::WriteXDR, xdrs::ConstWriteXDR)]
struct Primitives {
    a: u32,
    b: i64,
    c: bool,
    d: f32,
    e: [u8; 5],
    f: (u16, i16),
    g: core::marker::PhantomData<u64>,
}

#[derive(xdrs::WriteXDR)]
struct Nested {
    p: Primitives,
    q: Option<u32>,
    r: Result<u32, i64>,
    s: [u32; 3],
}

#[derive(xdrs::WriteXDR)]
enum Shape {
    Unit,
    Tuple(u32, i64),
    Struct { x: [u8; 3], y: bool },
}

#[derive(xdrs::WriteXDR)]
struct Unit;

fn worst_primitives() -> Primitives {
    Primitives {
        a: u32::MAX,
        b: i64::MAX,
        c: true,
        d: 0.5,
        e: [0xAB; 5],
        f: (u16::MAX, i16::MIN),
        g: core::marker::PhantomData,
    }
}

fn small_primitives() -> Primitives {
    Primitives {
        a: 0,
        b: 0,
        c: false,
        d: 0.0,
        e: [0; 5],
        f: (0, 0),
        g: core::marker::PhantomData,
    }
}

#[test]
fn structs_match() {
    assert_unchecked_matches_checked(&worst_primitives());
    assert_unchecked_matches_checked(&small_primitives());
    assert_unchecked_matches_checked(&Unit);
    assert_unchecked_matches_checked(&Nested {
        p: worst_primitives(),
        q: Some(7),
        r: Err(-1),
        s: [1, 70000, u32::MAX],
    });
    assert_unchecked_matches_checked(&Nested {
        p: small_primitives(),
        q: None,
        r: Ok(3),
        s: [0; 3],
    });
}

#[test]
fn enums_match() {
    assert_unchecked_matches_checked(&Shape::Unit);
    assert_unchecked_matches_checked(&Shape::Tuple(300, -5_000_000_000));
    assert_unchecked_matches_checked(&Shape::Struct {
        x: [1, 2, 3],
        y: true,
    });
}

#[test]
fn containers_match() {
    let v = VecM::<u32, 10>(vec![1, 2, 300]);
    let b = BytesM::<8>(vec![1, 2, 3]);
    let s = StringM::<16>(b"hello".to_vec());
    assert_unchecked_matches_checked(&v);
    assert_unchecked_matches_checked(&b);
    assert_unchecked_matches_checked(&s);

    let bytes: &[u8] = b"some raw bytes";
    assert_unchecked_matches_checked(&bytes);
    assert_unchecked_matches_checked(&"a str");
    assert_unchecked_matches_checked(&vec![1u32, 2, 3]);
    assert_unchecked_matches_checked(&(1u32, true, 3.5f64));
}

#[cfg(feature = "int128")]
#[test]
fn int128_matches() {
    assert_unchecked_matches_checked(&i128::MIN);
    assert_unchecked_matches_checked(&u128::MAX);
    assert_unchecked_matches_checked(&0i128);
}

// The headline use case: size the buffer with the const worst-case bound,
// then encode without checks.
#[test]
fn buffer_sized_from_const_max_bound() {
    const TXS_BOUND: usize = match Primitives::const_max_txs_size() {
        Some(n) => n,
        None => panic!("unbounded"),
    };
    const CXS_BOUND: usize = match Primitives::const_max_cxs_size() {
        Some(n) => n,
        None => panic!("unbounded"),
    };

    let value = worst_primitives();

    let mut buf = [MU::uninit(); TXS_BOUND];
    // SAFETY: the buffer is sized to the worst-case TXS bound.
    let written = unsafe { txs::encode_into_unchecked(&value, &mut buf) };
    // TXS is fixed-width: the worst case is exact.
    assert_eq!(written, TXS_BOUND);

    let mut buf = [MU::uninit(); CXS_BOUND];
    // SAFETY: the buffer is sized to the worst-case CXS bound.
    let written = unsafe { cxs::encode_into_unchecked(&value, &mut buf) };
    assert!(written <= CXS_BOUND);

    // A small value fits within the same worst-case buffer.
    let small = small_primitives();
    let mut buf = [MU::uninit(); CXS_BOUND];
    // SAFETY: CXS_BOUND bounds the encoded size of every Primitives value.
    let written = unsafe { cxs::encode_into_unchecked(&small, &mut buf) };
    assert!(written <= CXS_BOUND);
}

// Contract violations must be caught by debug_assert in debug builds.
// Gated on debug_assertions: without them these calls would be UB.
#[cfg(debug_assertions)]
mod debug_assert_violations {
    use super::*;

    #[test]
    #[should_panic(expected = "UncheckedBufWriter: buffer overflow")]
    fn short_buffer_txs() {
        let mut buf = [MU::uninit(); 3]; // a u32 needs 4
        let _ = unsafe { txs::encode_into_unchecked(&42u32, &mut buf) };
    }

    #[test]
    #[should_panic(expected = "UncheckedBufWriter: buffer overflow")]
    fn short_buffer_cxs() {
        let mut buf: [MU<u8>; 0] = [];
        let _ = unsafe { cxs::encode_into_unchecked(&42u32, &mut buf) };
    }

    #[test]
    #[should_panic(expected = "exceeds declared bound")]
    fn overlong_vecm() {
        // VecM's inner Vec is public, so the declared bound can be violated
        // by construction; the unchecked path debug_asserts it.
        let v = VecM::<u32, 2>(vec![1, 2, 3]);
        let mut buf = [MU::uninit(); 64];
        let _ = unsafe { txs::encode_into_unchecked(&v, &mut buf) };
    }

    #[test]
    #[should_panic(expected = "exceeds declared bound")]
    fn overlong_bytesm() {
        let b = BytesM::<2>(vec![1, 2, 3]);
        let mut buf = [MU::uninit(); 64];
        let _ = unsafe { cxs::encode_into_unchecked(&b, &mut buf) };
    }
}
```

- [ ] **Step 3: Run the tests**

Run: `cargo test -F alloc,derive,int128 --test unchecked_writer`
Expected: all tests PASS (including the 4 should_panic tests).

- [ ] **Step 4: Commit**

```bash
cargo fmt
git add tests/unchecked_writer.rs Cargo.toml
git commit -m "Add differential and debug-assert tests for the unchecked write path"
```

---

### Task 7: wasm-size-test entry points

**Files:**
- Modify: `wasm-size-test/src/lib.rs`

**Interfaces:**
- Consumes: `cxs::encode_into_unchecked` (Task 5).
- Produces: `encode_u32_unchecked`, `encode_i64_unchecked`, `encode_bytes_unchecked` wasm exports paralleling the checked ones, so the code-size delta is observable.

- [ ] **Step 1: Add unchecked exports**

Append to `wasm-size-test/src/lib.rs`:

```rust
// --- encode_into_unchecked (no checks, no Result) ---

#[unsafe(no_mangle)]
pub extern "C" fn encode_u32_unchecked(n: u32, out: *mut u8) -> u32 {
    // SAFETY: the host guarantees `out` points to at least 5 writable bytes
    // (the worst-case CXS size of a u32).
    let buf = unsafe { core::slice::from_raw_parts_mut(out as *mut MaybeUninit<u8>, 5) };
    unsafe { xdrs::cxs::encode_into_unchecked(&n, buf) as u32 }
}

#[unsafe(no_mangle)]
pub extern "C" fn encode_i64_unchecked(n: i64, out: *mut u8) -> u32 {
    // SAFETY: the host guarantees `out` points to at least 9 writable bytes
    // (the worst-case CXS size of an i64).
    let buf = unsafe { core::slice::from_raw_parts_mut(out as *mut MaybeUninit<u8>, 9) };
    unsafe { xdrs::cxs::encode_into_unchecked(&n, buf) as u32 }
}

#[unsafe(no_mangle)]
pub extern "C" fn encode_bytes_unchecked(ptr: *const u8, len: u32, out: *mut u8) -> u32 {
    // SAFETY: the host guarantees `ptr` points to `len` readable bytes and
    // `out` points to at least `5 + len` writable bytes (the worst-case
    // CXS size of a length prefix plus the data).
    unsafe {
        let data = core::slice::from_raw_parts(ptr, len as usize);
        let buf =
            core::slice::from_raw_parts_mut(out as *mut MaybeUninit<u8>, 5 + len as usize);
        xdrs::cxs::encode_into_unchecked(&data, buf) as u32
    }
}
```

- [ ] **Step 2: Build for wasm and inspect**

Run:

```bash
cd wasm-size-test
cargo build --release --target wasm32-unknown-unknown
ls -l target/wasm32-unknown-unknown/release/wasm_size_test.wasm
```

Expected: builds cleanly. (If the `wasm32-unknown-unknown` target is missing, install with `rustup target add wasm32-unknown-unknown`.)

If `wasm-tools` or `wasm2wat` is available, verify the no-checks claim by eyeballing `encode_u32_unchecked`: it must contain no `unreachable` and no length-compare branch, unlike `encode_u32`:

```bash
wasm2wat target/wasm32-unknown-unknown/release/wasm_size_test.wasm 2>/dev/null | less
```

This inspection is informational — record the observed function sizes in the commit message if available, but do not block on missing tooling.

- [ ] **Step 3: Commit**

```bash
cargo fmt
git add wasm-size-test/src/lib.rs
git commit -m "Add unchecked encode entry points to wasm-size-test"
```

---

### Task 8: Full CI

**Files:** none new.

- [ ] **Step 1: Run the full CI script**

Run: `./ci.sh` (from the repo root)
Expected: ends with `CI-CLEAN!`. Fix anything it flags (fmt, clippy, docs, doctests) and amend the relevant commit or add a fixup commit.

- [ ] **Step 2: Verify the working tree is clean and the branch is pushable**

```bash
git status
git log --oneline main..HEAD
```

Expected: clean tree; commits from Tasks 1-7 present.
