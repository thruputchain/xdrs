# ConstWriteXDR: Compile-Time XDR Encoding

## Problem

We need to encode XDR types at compile time so their byte representations can be
placed in statics and linker sections (e.g. `#[link_section = "groundhogspecrawv0"]`
for WASM). The existing `WriteXDR` trait goes through the `Writer<F>` trait, which
can't be `const fn` — Rust doesn't support const trait methods yet (RFC 3198).

## Approach

Add a parallel const encoding path that bypasses traits entirely:

1. Two const-compatible writer structs (`cxs::ConstWriter<N>`, `txs::ConstWriter<N>`)
2. A new `#[derive(ConstWriteXDR)]` that generates inherent `const fn` methods on
   opted-in types — no trait, no namespace pollution on types that don't need it

## Const Writers

### `cxs::ConstWriter<const N: usize>`

File: `src/cxs/const_writer.rs`

```rust
pub struct ConstWriter<const N: usize> {
    buf: [u8; N],
    pos: usize,
}
```

Methods replicate the CXS variable-width encoding from `cxs::put.rs`:

- `const fn new() -> Self`
- `const fn write_i32(&mut self, n: i32)` — cascading size: 4-byte if outside i16
  range, else 2-byte if outside i8 range, else 1-byte/header-only
- `const fn write_i64(&mut self, n: i64)` — 8-byte if outside i32 range, else
  delegates to write_i32
- `const fn write_u32(&mut self, n: u32)` — unsigned cascade
- `const fn write_u64(&mut self, n: u64)`
- `const fn write_i16(&mut self, n: i16)`
- `const fn write_u16(&mut self, n: u16)`
- `const fn write_bool(&mut self, v: bool)` — encodes as i32 (0 or 1)
- `const fn write_opaque_fixed(&mut self, bytes: &[u8])` — raw bytes, no padding
- `const fn write_opaque_variable(&mut self, bytes: &[u8])` — u32 length prefix
  via write_u32, then raw bytes
- `const fn pos(&self) -> usize`
- `const fn into_bytes(self) -> [u8; N]` — panics if `pos != N`

All methods panic on buffer overflow. In a const context this becomes a
compile-time error.

### `txs::ConstWriter<const N: usize>`

File: `src/txs/const_writer.rs`

Same struct shape, but TXS encoding: fixed-width big-endian integers, 4-byte
aligned padding.

- `const fn write_i32(&mut self, n: i32)` — always 4 bytes, big-endian
- `const fn write_i64(&mut self, n: i64)` — always 8 bytes, big-endian
- `const fn write_u32(&mut self, n: u32)` / `write_u64(&mut self, n: u64)`
- `const fn write_i16(&mut self, n: i16)` — promotes to i32
- `const fn write_u16(&mut self, n: u16)` — promotes to u32
- `const fn write_bool(&mut self, v: bool)` — as i32
- `const fn write_opaque_fixed(&mut self, bytes: &[u8])` — bytes + zero-pad to
  4-byte boundary
- `const fn write_opaque_variable(&mut self, bytes: &[u8])` — 4-byte big-endian
  length prefix + bytes + zero-pad to 4-byte boundary
- `const fn pos(&self) -> usize`
- `const fn into_bytes(self) -> [u8; N]` — panics if `pos != N`

## Derive Macro: `ConstWriteXDR`

New proc macro derive in `xdr-derive`, generates two inherent const methods per type.
The generated code is wrapped in a `const _: () = { ... };` block (like the existing
`WriteXDR` derive) to keep internal imports contained:

```rust
const _: () = {
    use ::xdrs::cxs;
    use ::xdrs::txs;

    impl MyStruct {
        pub const fn const_write_cxs<const N: usize>(&self, w: &mut cxs::ConstWriter<N>) { ... }
        pub const fn const_write_txs<const N: usize>(&self, w: &mut txs::ConstWriter<N>) { ... }
    }
};
```

### Structs

Each field emits a direct call:

- Primitive fields (`i32`, `u64`, `bool`, etc.): `w.write_<type>(self.field)`
- `[u8; N]` fields: `w.write_opaque_fixed(&self.field)`
- `&[u8]` / `&str` fields: `w.write_opaque_variable(self.field)`
- `i128` / `u128` fields (with `int128` feature): inline shift-and-split logic
  (write high 64 bits, write low 64 bits)
- Nested struct/enum fields (that also derive `ConstWriteXDR`):
  `self.field.const_write_cxs(w)` / `self.field.const_write_txs(w)`

### Enums

Must be `#[repr(i32)]`. Match on self, write discriminant, then variant fields:

```rust
match self {
    Self::Foo => { w.write_i32(0); }
    Self::Bar { val } => {
        w.write_i32(1);
        w.write_u64(*val);
    }
}
```

### Not Supported

These produce compile errors if used with `ConstWriteXDR`:

- `#[xdrs(into_type = "...")]` / `#[xdrs(via = "...")]` — requires
  `.clone().into()`, not const
- Generic type parameters — no const trait bounds to express constraints
- `Option<T>`, `Result<T, E>`, tuples, `Vec<T>`, `String`, `Box<T>` — no
  const methods on these; user gets "method not found" at compile time

### Attributes

- `#[xdrs(crate = "path")]` — works as today

## File Layout

New files:
- `src/cxs/const_writer.rs` — CXS const writer
- `src/txs/const_writer.rs` — TXS const writer
- `xdr-derive/src/generate_const.rs` — const derive codegen

Modified files:
- `src/cxs/mod.rs` — add `pub mod const_writer;`
- `src/txs/mod.rs` — add `pub mod const_writer;`
- `xdr-derive/src/lib.rs` — add `ConstWriteXDR` derive entry point

## Usage

```rust
use xdrs::{ConstWriteXDR, WriteXDR, ReadXDR};

#[derive(ConstWriteXDR, WriteXDR, ReadXDR)]
#[repr(i32)]
enum SpecVersion {
    V1 = 0,
}

#[derive(ConstWriteXDR, WriteXDR, ReadXDR)]
struct InterfaceSpec {
    version: SpecVersion,
    package_len: u32,
}

const SPEC: InterfaceSpec = InterfaceSpec {
    version: SpecVersion::V1,
    package_len: 5,
};

const TXS_BYTES: [u8; 8] = {
    let mut w = xdrs::txs::ConstWriter::<8>::new();
    SPEC.const_write_txs(&mut w);
    w.into_bytes()
};

#[used]
#[cfg_attr(target_family = "wasm", unsafe(link_section = "groundhogspecrawv0"))]
static RAW_INTERFACE_SPEC: [u8; 8] = TXS_BYTES;
```

## Out of Scope

- Const size calculation (`const_encoded_size_cxs/txs`) — possible future addition
- Const decoding — not needed for the link-section use case
- Automatic `N` inference — user provides the size, wrong size is a compile error
