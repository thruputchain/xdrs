# Infallible cursor encoding surface: `Cursor` + `ConstWriteXDR` trait

Date: 2026-07-09
Status: approved (Approach B; see discussion of alternatives at end)

## Motivation

sdk_rs needs an infallible, non-generic, cursor-style entry to the CXS/TXS
encoding rules for small rv32 codegen (abort payloads, user exception types).
xdrs already has most of the machinery: `cxs::ConstWriter<N>` /
`txs::ConstWriter<N>` (infallible panicking const writers) and the
`#[derive(ConstWriteXDR)]` macro emitting inherent `const fn` encoders.

Gaps this design closes:

1. `ConstWriter::into_bytes` panics unless the buffer is filled exactly;
   partial-fill users (value-dependent payload lengths) have no way to get
   the written prefix.
2. The derive rejects type parameters, and the generated methods are
   *inherent*, so generic code (e.g. sdk `throw<E>`) has no trait bound to
   call through. `ContractExn<E>` cannot participate at all.
3. Bounded containers (`VecM`/`StringM`/`BytesM`) provide no cursor encoding.
4. No worst-case encoded size, so callers hand-compute buffer bounds.
5. Trait methods generic over a buffer-size const `N` would monomorphize one
   encoder per (type, N) pair — against the code-size goal.

## Constraints

- **Stable Rust only** (pinned toolchain 1.95.0). No `generic_const_exprs`,
  no `const_trait_impl`. (Verified: slice-backed cursors with `&mut [u8]`
  fields, indexing, and `split_at` all work in `const fn` / const items on
  stable.)
- **Compile-time encoding is a requirement.** The existing const-context
  capability (encoding values into `const` items) must be preserved.
  The additions are additive to that surface.
- Backwards API compatibility is explicitly NOT required (one deliberate
  breaking change is called out below). Version bumps to 0.8.0.
- Format parity: everything exists for both CXS and TXS.

## Design

### 1. `as_bytes` on both `ConstWriter`s (immediate; unblocks sdk_rs PR #279)

```rust
/// The written prefix of the buffer (`&buf[..pos]`).
pub const fn as_bytes(&self) -> &[u8]
```

on `cxs::ConstWriter<N>` and `txs::ConstWriter<N>`. Implemented with
`split_at` (const-stable). No behavior change to `into_bytes`.

### 2. Non-generic slice cursor: `cxs::Cursor<'a>` and `txs::Cursor<'a>`

```rust
pub struct Cursor<'a> { buf: &'a mut [u8], pos: usize }
```

- All methods are `const fn`, panic on overflow (same contract as
  `ConstWriter`).
- Full vocabulary mirroring `ConstWriter`: `write_bool`, `write_i16/32/64`,
  `write_u16/32/64`, `write_i128/u128` (feature `int128`), `write_f32/f64`,
  `write_opaque_fixed`, `write_opaque_variable`.
- `new(&'a mut [u8]) -> Self`, `pos(&self) -> usize`,
  `as_bytes(&self) -> &[u8]` (written prefix).
- **The width/format rules live exactly once.** The encoding logic moves to
  shared `const` helpers (private free fns or equivalent); both `Cursor` and
  `ConstWriter` become thin delegates. No duplicated format logic.
- `ConstWriter<N>` is retained unchanged in API (plus `as_bytes`): it remains
  the owned-buffer convenience for building `const` arrays. `Cursor` is the
  canonical cursor everything else targets.

### 3. The `ConstWriteXDR` trait (in xdrs, same name as the derive)

```rust
pub trait ConstWriteXDR {
    /// Worst-case CXS encoded size over all values, `None` if unbounded.
    const MAX_CXS_LEN: Option<usize>;
    /// Worst-case TXS encoded size over all values, `None` if unbounded.
    const MAX_TXS_LEN: Option<usize>;

    fn write_cxs(&self, w: &mut cxs::Cursor<'_>);
    fn write_txs(&self, w: &mut txs::Cursor<'_>);
    /// Exact CXS encoded size of this value.
    fn cxs_size(&self) -> usize;
    /// Exact TXS encoded size of this value.
    fn txs_size(&self) -> usize;
}
```

Key properties:

- Methods are **non-generic** → exactly one encoder instantiation per type,
  regardless of buffer size. Generic code bounds on `T: ConstWriteXDR`.
- Methods are *not* `const fn` (const trait methods are unstable). The trait
  is the runtime surface; const contexts use the inherent `const fn`s. When
  const traits stabilize, the surfaces collapse.
- `MAX_*_LEN` are **required** (no default) — a hand-written impl cannot
  silently poison containing types' maxes by omission.
- Per-format maxes because CXS (varint) and TXS (fixed-width) sizes differ.
  A single `MAX_ENCODED_LEN` would be wrong.
- Associated consts make the trait not dyn-compatible. Accepted; if `dyn`
  ever matters, split the consts into a companion trait then (YAGNI now).
- Consumers get compile-time infallibility at concrete types:
  `const N: usize = match T::MAX_CXS_LEN { Some(n) => n, None => panic!("unbounded") };`
  (const panic = compile error). No `generic_const_exprs` needed —
  `Cursor`/`ConstWriter` keep the caller-picks-buffer shape.

### 4. `Option<usize>` const arithmetic helpers

Public const fns (needed by derive-generated code), e.g. module
`xdrs::encoded_max`:

- `add(Option<usize>, Option<usize>) -> Option<usize>` — checked;
  `None` if either is `None` or on overflow.
- `mul(usize, Option<usize>) -> Option<usize>` — checked, same rules.
- `max(Option<usize>, Option<usize>) -> Option<usize>` — `None` if either
  is `None` (an unbounded variant makes the whole type unbounded).

Overflow maps to `None` (unbounded) rather than panicking: total, safe, and
routes oversized types to the fallible path instead of failing the build of
a type nobody const-encodes.

### 5. Leaf and container impls in xdrs

Mirror the `WriteXDR` impl surface. All write/size bodies delegate to
`Cursor` methods / existing `size_*` const fns — never re-derive wire format.

| Type | MAX_CXS_LEN | MAX_TXS_LEN |
|---|---|---|
| `bool` | 1 | 4 |
| `i16`/`u16` | 3 | 4 |
| `i32`/`u32` | 5 | 4 |
| `i64`/`u64` | 9 | 8 |
| `i128`/`u128` (feature) | 18 | 16 |
| `f32` | 4 | 4 |
| `f64` | 8 | 8 |
| `[u8; N]` | N | N padded to 4 |
| `[T; N]` | N × T (checked) | N × T (checked) |
| `Option<T>` | 1 + T | 4 + T |
| `[u8]`, `&[u8]`, `[T]`, `&str` | `None` | `None` |
| `Vec<T>`, `Vec<u8>`, `String` (alloc) | `None` | `None` |
| `BytesM<MAX>`, `StringM<MAX>`, `VecM<u8, MAX>` | size_u32(MAX) + MAX | 4 + MAX padded |
| `VecM<T, MAX>` | size_u32(MAX) + MAX × T (checked) | 4 + MAX × T (checked) |
| `xdr_common::Box<T>` | forward to T | forward to T |

(Exact TXS numbers to be taken from the existing `txs` size fns, not this
table, if they disagree — the txs module is authoritative.)

Notes:

- `VecM<T, MAX>` etc. implement the **trait** (runtime): generic `T` element
  iteration through `T: ConstWriteXDR` bounds is fine in non-const code.
- The byte-shaped bounded types (`BytesM`, `StringM`, `VecM<u8, MAX>`) also
  get inherent `const fn` encoders (const-context capable — no type-parameter
  calls involved). Placement follows the existing pattern for their
  `WriteXDR` impls (feature `xdr_common`).
- Whatever `WriteXDR` covers under `heapless`/other features gets mirrored
  where feasible; anything skipped is documented.

### 6. Derive changes (`#[derive(ConstWriteXDR)]`)

The derive now emits, per type:

1. **Trait impl** of `ConstWriteXDR` (always). For concrete types the write
   methods forward to the inherent `const fn`s; for type-parametric types
   the bodies are emitted directly in the trait impl.
2. **Inherent `const fn`s** (`const_write_cxs`, `const_write_txs`,
   `const_cxs_size`, `const_txs_size`) — **only for types without type
   parameters** (unchanged restriction there; calling a type parameter's
   methods inside `const fn` requires const traits).
3. **`MAX_CXS_LEN` / `MAX_TXS_LEN`** as const folds using
   `xdrs::encoded_max::{add, mul, max}`:
   - struct: fold `add` over field maxes;
   - enum: for each variant, `add(discriminant size for that variant's
     value, fold of payload field maxes)`, then fold `max` over variants
     (tighter than max-disc + max-payload);
   - named leaf positions use `<FieldTy as ConstWriteXDR>::MAX_CXS_LEN`;
     syntactically-recognized shapes (arrays, `Option`, tuples, slices,
     opaque bytes) recurse syntactically, mirroring the existing generator.
4. **Type parameters are now accepted.** Bounds: each type parameter used in
   fields gets `T: ConstWriteXDR` (same bound-copying strategy as the
   `WriteXDR` derive). This is the change that lets `ContractExn<E>` derive.

**Deliberate breaking change:** the inherent `const fn` writers change
signature from `&mut ConstWriter<N>` to `&mut Cursor<'_>` — one generated
body instead of two, one encoder instantiation per type. Const-context users
switch from `ConstWriter::new()/into_bytes()` to a local buffer + `Cursor`
(verified stable). `ConstWriter` itself remains for direct primitive
composition. This is the 0.8.0 bump.

The existing hygiene discipline (raw-identifier type recognition, `__N`
avoidance, `::core::assert!`) carries over; the buffer-size const-generic
hygiene concerns disappear along with the const generic.

### 7. Testing

- **Byte-equality vs the fallible Writer path** (both formats) across width
  boundaries: 62/63, ±31/32, 127/128, 255/256, 65535/65536, u32::MAX,
  i32::MIN, u64::MAX — for `Cursor`, for trait impls, and for derived types.
- `MAX_*_LEN` property: for a battery of values (including extremes),
  `exact size ≤ MAX` for every bounded type; derived maxes match
  hand-computed expectations for representative structs/enums.
- Generic derive test: a `ContractExn<E>`-shaped generic enum derives and
  round-trips byte-equal.
- Const-context tests: derived concrete type encoded into a `const` item via
  `Cursor`; `as_bytes` partial-fill; existing `ConstWriter` const tests keep
  passing.
- Existing derive test suites (hygiene, opaque bounds, default variants)
  updated for the new signature and extended for trait/MAX emission.
- All of `ci.sh` green (workspace all-features, no-alloc, docsrs docs,
  `--deny warnings` incl. `missing_docs`).

### 8. Release

Semver-major-ish bump 0.7.1 → 0.8.0 (derive output signature change).
Publish to the groundhog registry after merge; sdk_rs then bumps its pin,
reworks PR #279 on `Cursor`, and later retires its hand-written encoders via
the derive.

## Alternatives considered

- **Trait over `ConstWriter<N>`** (const-generic trait methods): rejected —
  per-(type, N) monomorphization defeats the code-size goal; N-threading
  infects every generic signature.
- **Generic `write_xdr<T: WriteXDR>` bridge on the const types**: rejected
  earlier — routes through the fallible writer machinery, the exact
  code-size path this avoids.
- **Folding infallibility into `WriteXDR`/`Writer`**: rejected —
  `WriteResult` is hard-coded into `Writer`'s signatures; fixing it means
  redesigning that machinery (not additive).
- **Faking `u32::MAX` maxes for unbounded types**: rejected — poisons every
  containing type's max; `None` routes to the fallible path honestly.
