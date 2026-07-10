# Unchecked (infallible, unsafe) write path

Date: 2026-07-10

## Goal

An infallible, unsafe serialization path with zero runtime checks in release
builds, for minimum code size when embedding in a smart contract (wasm). The
caller sizes the output buffer up front — typically with the
`const_max_cxs_size()` / `const_max_txs_size()` bounds from
`#[derive(ConstWriteXDR)]` — and in exchange the entire `WriteResult`
plumbing, buffer bounds checks, and value-bound checks disappear from the
compiled code. All preconditions are `debug_assert!`ed, so misuse is caught
in debug builds and tests at zero release cost.

## Non-goals

- No separate `WriteXDRUnchecked` trait or derive. The method is added to the
  existing `WriteXDR` trait so `#[derive(WriteXDR)]` covers it with no new
  derive plumbing and no changes to downstream type definitions.
- No unchecked *read* path.
- No change to the checked path's behavior or performance.
- No attempt to keep semver compatibility for manual `WriteXDR` implementors:
  adding a required method is a breaking change. All manual impls are
  in-crate (`int128.rs`, `heapless.rs`); downstream users of the derive are
  unaffected.

## New trait: `WriterUnchecked<F>`

`src/serialize/writer_unchecked.rs`, sibling of `writer.rs`. The same 11
methods as `Writer<F>`, each an `unsafe fn` returning `()`:

```rust
pub trait WriterUnchecked<F> {
    unsafe fn write_opaque_fixed(&mut self, bytes: &[u8]);    // doc(hidden), like Writer
    unsafe fn write_opaque_variable(&mut self, bytes: &[u8]);
    unsafe fn write_i64(&mut self, n: i64);
    // ... i32, i16, u64, u32, u16, bool, f32, f64
}
```

Safety contract per method: the writer's underlying buffer must have enough
space remaining for the encoded form of the argument, and for the opaque
methods `bytes.len() <= u32::MAX`. The trait itself is safe to implement —
the obligation rests on callers, so `unsafe` lives on the methods.

## `WriteXDR` gains a required unsafe method

```rust
pub trait WriteXDR {
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()>;

    /// # Safety
    /// The writer must have space for the encoded form of `self`, and
    /// `self` must respect all declared XDR bounds (e.g. `VecM<_, MAX>`
    /// length <= MAX), since a violation would make the encoding exceed
    /// any precomputed size bound.
    unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W);
}
```

Every impl in `write_xdr.rs` gains the mirrored method (ints, floats, bool,
`[u8]`, `&[u8]`, `[u8; N]`, `[T; N]`, `[T]`, `&T`, `&mut T`, `&[T]`, `str`,
`&str`, `String`, `()`, `Infallible`, tuples 1–16, `Option`, `Result`,
`PhantomData`, `VecM`/`BytesM`/`StringM`, `Box`, `Vec`), as do the manual
impls in `int128.rs` (composed from two 64-bit writes) and `heapless.rs`.
Bodies mirror the checked versions with no `?`; runtime value-bound checks
(`len > u32::MAX` → `InvalidArrayLength`, `VecM`/`BytesM`/`StringM`
`len > MAX`) become `debug_assert!`s.

## Format impls, segregated

Mirroring the existing per-format `Put` structure, kept fully separate:

- `src/txs/put.rs`: crate-private `PutUnchecked` (`put_bytes`,
  `put_opaque_variable`, `put_opaque_fixed`, all `unsafe fn`) and a blanket
  `impl<P: PutUnchecked> WriterUnchecked<TXS> for P` that mirrors the
  checked blanket impl (16-bit promotion, bool as i32, big-endian).
- `src/cxs/put.rs`: crate-private `PutUnchecked` (`put_byte`, `put_bytes`,
  both `unsafe fn`) and a blanket `impl<P: PutUnchecked> WriterUnchecked<CXS>
  for P` duplicating the CXS variable-width compression logic without
  `Result`. The `len > u32::MAX` check in `write_opaque_variable` becomes a
  `debug_assert!`.

New writer structs, one per format module (`src/txs/unchecked_buf_writer.rs`,
`src/cxs/unchecked_buf_writer.rs`):

```rust
pub struct UncheckedBufWriter<'a> {
    buf: &'a mut [MaybeUninit<u8>],
    written: usize,
}
```

`new()` is safe (unsafety attaches to the write calls, not construction);
`written()` mirrors `IntoBufWriter`. Each `PutUnchecked` method
`debug_assert!(self.written + len <= self.buf.len())`, then writes via the
existing `mu::write_copy_of_slice` on an unchecked subslice
(`get_unchecked_mut`) — no branch, no panic path in release.

## Entry points

In each format's `utils.rs`:

```rust
/// # Safety
/// - `buf.len()` >= the encoded size of `value`. With
///   `#[derive(ConstWriteXDR)]`, `const_max_txs_size()` (worst case) or
///   `const_txs_size()` (exact) provide suitable sizes.
/// - `value` respects all declared XDR bounds.
pub unsafe fn encode_into_unchecked<T>(value: &T, buf: &mut [MU<u8>]) -> usize
where
    T: WriteXDR + ?Sized,
```

Constructs an `UncheckedBufWriter`, calls `value.write_xdr_unchecked(&mut w)`,
returns `w.written()`.

## Derive changes

`xdr-derive/src/generate.rs`: the existing `WriteXDR` derive emits
`write_xdr_unchecked` alongside `write_xdr` in the same `impl` block, for
every shape it supports today (structs, enums with explicit/index/default
discriminants, `#[xdr(into)]` delegation). Bodies mirror the checked codegen
with `unsafe` propagated and no `?`. No new proc-macro entry point; no
change to `#[derive(XDR)]` composition.

## Testing

- Differential tests (`tests/unchecked_writer.rs`): for derived structs,
  enums, and container-bearing types, `encode_into_unchecked` output is
  byte-identical to `encode_into` in both formats, with buffers sized
  exactly from `const_max_*_size()` / `const_*_size()`.
- `#[should_panic]` tests for the debug asserts: buffer one byte short;
  `VecM` constructed over its `MAX` via the public field. (Tests build with
  `debug_assertions` on.)
- Unit tests for `UncheckedBufWriter` primitives in both format modules.
- `wasm-size-test`: unchecked encode entry points alongside the checked
  ones, so the size win and absence of check/panic code are observable in
  the compiled wasm.
- All new public items documented (`missing_docs` is enforced), each unsafe
  fn with a `# Safety` section.
