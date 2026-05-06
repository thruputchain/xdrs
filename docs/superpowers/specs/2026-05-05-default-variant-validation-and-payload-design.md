# Default Variant Validation and Payload Round-Trip Fix

**Date**: 2026-05-05
**Issue**: #54

## Problem

The `Default` variant in `#[derive(ReadXDR, WriteXDR)]` enums is recognized by name only.
`check_variants` documents that it must have one `i32` field but never enforces any shape
constraint. This causes two bugs:

1. **Extra fields lose data**: `Default(i32, i32)` compiles, but the writer emits only field 0
   as the discriminant and skips field 1. The reader fills both fields with the discriminant
   value `d`, producing `Default(7, 7)` from `Default(7, 9)`.

2. **Fieldless default accepted**: `Default` (unit variant) compiles, silently accepts all
   unknown discriminants, discards the discriminant value, and creates a decoder/schema
   mismatch.

## Design

### Part 1: Validate the default variant shape in `check_variants`

In `xdr-derive/src/util.rs`, after confirming the default variant is trailing, add:

- **Reject fieldless**: the default variant must have at least one field.
- **Require first field is `i32`**: the first field (named or unnamed) must have type `i32`,
  since it stores the unmatched discriminant value.
- **Allow additional fields**: any fields beyond the first are valid payload — no restriction
  on count or types. This matches XDR union semantics where `default: <declaration>` can carry
  arbitrary payload.

### Part 2: Fix writer codegen (`write_variant`)

`xdr-derive/src/generate.rs`, `write_variant` function.

Currently the default arm sets `write_bindings` to empty, skipping all field writes. Change to:

- Keep writing `*_self_0` as the discriminant (existing behavior).
- Emit `write_bindings` for fields 1..N (the payload fields). Skip only field 0.

### Part 3: Fix reader codegen (`read_variant` and `init_fields`)

`xdr-derive/src/generate.rs`, `read_variant` and `init_fields` functions.

**`read_variant`**: currently returns `quote! {}` for every field of a default arm (reads
nothing). Change to skip only field 0, and emit `read_xdr` calls for fields 1..N.

**`init_fields`**: currently initializes every default-arm field with `d`. Change so only
field 0 gets `d`; fields 1..N get their read bindings (`_self_1`, `_self_2`, etc.).

### Part 4: Fix schema generation

`xdr-derive/src/generate_type.rs`, `union_typedef` function.

Currently the default arm unconditionally emits `declare_void()`. Change to emit actual field
declarations for the payload fields (fields 1..N), using the same logic as non-default variant
fields. Field 0 (the discriminant) is excluded from the declaration since it is the switch
variable.

If the default variant has only the single `i32` discriminant field and no payload, emit
`declare_void()` as before.

Note: `variant_fields_decl` already has `unimplemented!` for unnamed tuple variants with
fields — this is a pre-existing limitation unrelated to this fix. Schema generation for
default arms with unnamed payload fields will hit the same limitation. The codec round-trip
(ReadXDR/WriteXDR) works regardless of schema generation.

### Part 5: Tests

- **Round-trip test for default-with-payload**: define an enum with a `Default` variant
  carrying an `i32` discriminant plus additional payload fields (e.g., `Default(i32, u32)` and
  `Default { code: i32, data: u32 }`). Encode and decode through both CXS and TXS, assert
  the decoded value equals the original.
- **Existing single-field default tests**: must continue to pass unchanged (`tests/enums.rs`
  cases for `Foo::Default(3)` and `FooDefault::Default { d: discriminant }`).
- **Fieldless default rejection**: confirm that the derive macro rejects a fieldless `Default`
  variant at compile time. The existing `#[cfg(any())]` block in
  `tests/repr/derive_enums.rs:168` stays as-is.

### Files changed

| File | Change |
|------|--------|
| `xdr-derive/src/util.rs` | Add field-shape validation to `check_variants` |
| `xdr-derive/src/generate.rs` | Fix `write_variant`, `read_variant`, `init_fields` for default arms with payload |
| `xdr-derive/src/generate_type.rs` | Fix `union_typedef` default arm declaration |
| `tests/enums.rs` | Add round-trip tests for default-with-payload (unnamed and named fields) |
