# Optional Byte Buffers: Add `HasTypeSpec` for Byte-Buffer Types

**Date**: 2026-05-05
**Issue**: #99

## Problem

`Option<T>` schema generation requires `T: HasTypeSpec`, but the four byte-buffer
types (`&[u8]`, `Vec<u8>`, `BytesM<MAX>`, `VecM<u8, MAX>`) only implement `Declare`.
Runtime codecs (CXS/TXS) round-trip `Option<Vec<u8>>` and `Option<&[u8]>` correctly,
but `#[derive(XDR)]` on structs containing those fields fails with trait-bound errors.

## Approach

Add `HasTypeSpec` implementations for all four byte-buffer types using the
established embedded single-field struct pattern (same as `[u8; N]` and `[T; N]`).

Each impl returns:
```
TypeSpec::Struct(StructBody(vec![
    Declaration::VariableOpaque { name: ident("unnamed_0"), max }
]))
```

Where `max` is `u32::MAX` for unbounded types and `MAX` for bounded types.

## Changes

### `xdr-repr/src/ty.rs`

1. `impl HasTypeSpec for &[u8]` — max = `u32::MAX`, next to existing `Declare` impl
2. `impl HasTypeSpec for Vec<u8>` — max = `u32::MAX`, inside the alloc-gated `const _` block
3. `impl<const MAX: u32> HasTypeSpec for BytesM<MAX>` — max = `MAX`
4. `impl<const MAX: u32> HasTypeSpec for VecM<u8, MAX>` — delegates to `BytesM::<MAX>::type_spec()`

### Tests

Unit tests in `xdr-repr` confirming each new `HasTypeSpec` impl returns the expected
`TypeSpec::Struct(StructBody(...))` with the correct `VariableOpaque` declaration.

### No other changes

No derive macro changes. No runtime codec changes. The existing `Option<T: HasTypeSpec>`
impl and `Declare` impls are unchanged.
