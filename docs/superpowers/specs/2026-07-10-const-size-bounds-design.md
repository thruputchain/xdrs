# Const size bounds for ConstWriteXDR

Date: 2026-07-10

## Goal

Compute, at compile time, the worst-case encoded size of a type across all of
its values — one bound per serialization format. `#[derive(ConstWriteXDR)]`
gains two inherent const fns alongside the existing value-dependent
`const_cxs_size(&self)` / `const_txs_size(&self)`:

```rust
pub const fn const_max_cxs_size() -> Option<usize>;
pub const fn const_max_txs_size() -> Option<usize>;
```

`None` means "no usable bound": the type is unbounded (variable length with no
static cap), possibly recursive (reachable only through indirection), or its
bound overflows `usize` on the current target.

## Non-goals

- No trait; these are inherent fns, matching the existing ConstWriteXDR derive
  surface.
- No annotation to opt indirected types back into bounded-ness. Indirection is
  always a cut.
- No minimum-size bounds.

## Pointer-cut rule

Bounds fold through by-value layout only. Any shape that reaches its contents
through a pointer emits **literal `None`** — the generated code never names the
pointee's `const_max_*_size` fns, so const-evaluation cycles for recursive
types are impossible by construction. A recursive type must pass through
indirection (by-value self-containment is unrepresentable in Rust), and every
indirected shape produces `None` without recursing.

Cut shapes: `&T`, `&[T]`, `&[u8]`, `&str`, and element-generic
`VecM<T, MAX>` (heap-backed). Byte-shaped bounded containers keep exact
bounds: `BytesM<MAX>`, `StringM<MAX>`, `VecM<u8, MAX>`; when the `MAX`
parameter is not spelled out in the field type, it defaults to `u32::MAX`
(matching the `xdr_common` type defaults), giving a huge but finite bound.

## Shape folding

| Shape | CXS bound | TXS bound |
|---|---|---|
| `i16`/`u16` | 3 | 4 |
| `i32`/`u32` | 5 | 4 |
| `i64`/`u64` | 9 | 8 |
| `i128`/`u128` (feature `int128`) | 18 | 16 |
| `bool` | 1 | 4 |
| `f32` | 4 | 4 |
| `f64` | 8 | 8 |
| `[u8; N]` | N (None if N > u32::MAX) | padded(N) |
| `BytesM<MAX>`/`StringM<MAX>`/`VecM<u8, MAX>` | size_u32(MAX) + MAX | 4 + padded(MAX) |
| `PhantomData`, `Infallible` | 0 | 0 |
| `Option<T>` | size_i32(1) + bound(T) | 4 + bound(T) |
| `Result<T, E>` | size_i32(1) + max(bound(T), bound(E)) | 4 + max |
| tuple | sum of element bounds | sum |
| `[T; N]` | N * bound(T), None if N > u32::MAX | same |
| named by-value leaf `Foo` | `<Foo>::const_max_cxs_size()` | `<Foo>::const_max_txs_size()` |
| `&T`, `&[T]`, `&[u8]`, `&str`, `VecM<T, MAX>` (T ≠ u8) | None | None |

All arithmetic is checked; overflow maps to `None` (a bound near `u32::MAX`
is finite on 64-bit targets but overflows `usize` on 32-bit — such types fall
back to `None` there rather than failing compilation).

Enums: bound = max over variants of (discriminant size + field bounds).
Explicit or index discriminants use the exact `size_i32(d)`; a
`default_variant` arm carries a runtime discriminant, so it uses
`max_size_i32()` and skips its first field (which is the discriminant).
Uninhabited enums bound to `Some(0)`.

## Module layout

Per the repo's existing segregation of CXS and TXS const serialization, the
leaf and fold helpers are implemented separately (and identically-shaped) in
`src/cxs/const_writer.rs` and `src/txs/const_writer.rs`:

- `max_size_{i16,u16,i32,u32,i64,u64,i128,u128,bool,f32,f64}() -> usize`
- `max_size_opaque_fixed(len: usize) -> Option<usize>`
- `max_size_opaque_variable(max_len: u32) -> Option<usize>`
- `max_size_add(a: Option<usize>, b: Option<usize>) -> Option<usize>`
- `max_size_max(a: Option<usize>, b: Option<usize>) -> Option<usize>`
- `max_size_array(len: usize, elem: Option<usize>) -> Option<usize>`

The fold helpers are None-propagating with checked arithmetic;
`max_size_array` also yields `None` when `len > u32::MAX` (such arrays can
never be encoded — the writers assert on them).

## Testing

- Unit tests for helper values and overflow behavior in both modules.
- Derive token tests in `xdr-derive/src/generate_const.rs`.
- Integration tests (`tests/const_size_bounds.rs`): exact bounds for
  primitive/tuple/array/Option/Result/enum compositions in both formats;
  `None` for pointer-cut and unbounded shapes; a recursive type
  (`Option<&Node>`) compiles and bounds to `None`; a bound is used as a
  `ConstWriter` buffer size and worst-case values fit exactly (TXS) /
  within bound (CXS); nested struct leaves fold through
  `const_max_*_size()`.
