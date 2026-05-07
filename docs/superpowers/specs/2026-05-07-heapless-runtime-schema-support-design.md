# Heapless Runtime and Schema Support

**Date**: 2026-05-07
**Issue**: #169

## Problem

The root `xdrs` crate supports runtime codecs for `heapless::Vec<u8, N>` and
`heapless::String<N>` behind the `heapless` feature, but `xdr-repr` has no
matching schema traits. A type can derive `ReadXDR` and `WriteXDR` for these
fields, but adding `derive(XDR)` fails with raw missing `Declare` bounds.

The current runtime surface is also incomplete: `heapless::Vec<T, N>` should
support XDR variable-array encoding for element types that already implement
the normal XDR runtime traits.

## Design

Add heapless support in two layers:

1. Runtime codecs in the root crate:
   - Preserve `heapless::Vec<u8, N>` as variable opaque bytes.
   - Preserve `heapless::String<N>` as a bounded XDR string.
   - Add `heapless::Vec<T, N>` as a bounded XDR variable array where
     `T: ReadXDR` or `T: WriteXDR`.

2. Schema traits in `xdr-repr`:
   - Map `heapless::Vec<u8, N>` to `Declaration::VariableOpaque` with max `N`.
   - Map `heapless::String<N>` to `Declaration::String` with max `N`.
   - Map `heapless::Vec<T, N>` to `Declaration::VariableArray` with max `N`
     where `T: HasTypeSpec`.

The `u8` special case must remain byte-opaque. This matches existing `Vec<u8>`
and `VecM<u8, MAX>` behavior and avoids treating `u8` as a scalar XDR element
type, which this crate does not currently expose.

## Feature Wiring

Add an optional `heapless` dependency and `heapless` feature to `xdr-repr`.
The root crate's `heapless` feature should enable both `dep:heapless` and
`xdr-repr?/heapless`, so users who enable `xdrs` with `heapless`, `derive`,
and `repr` get matching runtime and schema support.

## Runtime Details

For `heapless::Vec<T, N>` decode:

- Read the XDR length prefix.
- Reject lengths greater than `N` with `ReadError::MaxLengthExceeded`.
- Read each element with `read_xdr`.
- Push into the heapless vector. The push should not fail after the length
  check; if it does, return `ReadError::MaxLengthExceeded`.

For encode:

- Reject lengths greater than `N` with `WriteError::InvalidArrayLength`.
- Write the length prefix.
- Write each element with `write_xdr`.

`heapless::Vec<u8, N>` continues using `read_opaque_variable(N)` and
`write_opaque_variable`.

`heapless::String<N>` continues using `read_opaque_variable(N)`, UTF-8
validation, and `write_opaque_variable(self.as_bytes())`.

## Tests

Add tests that cover:

- `heapless::Vec<u32, 4>` CXS and TXS round trips as variable arrays.
- Over-limit decode for `heapless::Vec<u32, 2>` rejects before pushing past
  capacity.
- Existing `heapless::Vec<u8, N>` byte-opaque encoding remains unchanged.
- Existing `heapless::String<N>` encoding remains unchanged.
- `derive(XDR)` output for:
  - `heapless::Vec<u8, 8>` -> `opaque bytes<8>;`
  - `heapless::String<16>` -> `string text<16>;`
  - `heapless::Vec<u32, 4>` -> `unsigned int nums<4>;`
- Direct `xdr-repr` `Declare` and `HasTypeSpec` behavior behind the new
  `heapless` feature.

## Non-Goals

- Do not add scalar `u8` XDR support.
- Do not change existing `Vec<u8>`, `VecM<u8, MAX>`, or `[u8; N]` schema
  mappings.
- Do not add heapless support for `heapless::Deque`, `heapless::IndexMap`, or
  other heapless collection types.
