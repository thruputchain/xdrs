# Reject Implicit Discriminants with External Discriminant Type

**Date**: 2026-05-06
**Issue**: [#167 — External Discriminant Implicit Value Drift](https://github.com/thruputchain/xdrs/issues/167)
**Severity**: High (codec correctness)

## Problem

When a data-bearing enum uses `#[xdr(discrim_type = "Status")]` with implicit
Rust variant discriminants (no `= expr`), the generated XDR schema correctly
references the external enum's values (e.g. `Ready=10, Failed=11`), but the
runtime `ReadXDR`/`WriteXDR` derives fall back to zero-based variant indices
(`0, 1, …`). This causes:

- `WriteXDR` to emit the wrong discriminant on the wire.
- `ReadXDR` to reject schema-compliant bytes with `BadDiscriminator`.

The existing `check_variants()` validation rejects mixed explicit/implicit
discriminants but does not cross-reference against `discrim_type`, so the
invalid combination compiles without error.

## Fix Strategy

**Compile-time rejection (Option A from brainstorming):** when `discrim_type` is
set and any non-default variant has an implicit discriminant, emit a compile
error directing the user to add explicit discriminant expressions.

## Changes

### 1. `xdr-derive/src/util.rs` — add `has_xdr_discrim_type` helper

Add a standalone function that inspects `&syn::DeriveInput` for the
`#[xdr(discrim_type = "...")]` attribute. This avoids coupling `util.rs` to any
module's private `Attrs` struct and lets all four `check_variants` call sites
use it.

```rust
pub fn has_xdr_discrim_type(input: &syn::DeriveInput) -> bool
```

Parses `input.attrs` looking for a `Meta::NameValue` with path `discrim_type`
inside an `xdr(...)` list.

### 2. `xdr-derive/src/util.rs` — extend `check_variants` signature

Add a `has_discrim_type: bool` parameter:

```rust
pub fn check_variants(
    input: &syn::DeriveInput,
    item: &syn::DataEnum,
    has_discrim_type: bool,
) -> Result<()>
```

New validation clause (before or after the existing mixed-discriminant check):

```
if has_discrim_type && n_implicit > 0 {
    err(input,
        "derive XDR: all payload variants must have explicit discriminants \
         when `discrim_type` is set \
         (e.g. `VariantName { ... } = ExternalEnum::VariantName as i32`)")
}
```

### 3. Update call sites

Four call sites, each passing `has_xdr_discrim_type(input)`:

| File | Function | Line |
|---|---|---|
| `generate_type.rs` | `from_enum` | 138 |
| `generate.rs` | `write_enum` | 150 |
| `generate.rs` | `read_enum` | 396 |
| `generate_const.rs` | `const_write_enum` | 402 |

Each becomes:

```rust
check_variants(input, item, has_xdr_discrim_type(input))?;
```

### 4. Tests

**Compile-fail test:** A new test (in `tests/` or via `trybuild`) that
derives `ReadXDR`/`WriteXDR`/`XDR` on an enum with `#[xdr(discrim_type)]` and
implicit discriminants, asserting the expected compile error message.

**Positive regression test:** A test exercising the *explicit* discriminant +
`discrim_type` path (similar to the existing `separate_discriminant` test in
`tests/repr/derive_enums.rs`) to confirm it still compiles and round-trips
correctly.

## What This Does NOT Change

- Enums without `discrim_type` are unaffected (implicit discriminants remain
  valid).
- Enums with `discrim_type` and all-explicit discriminants are unaffected.
- No runtime codegen changes in `write_variant` or `read_variant` — the fix is
  purely a validation gate that prevents the broken combination from compiling.

## Risk

Minimal. This is a strictly additive validation check. The only code that breaks
is code that was already silently producing incorrect codecs. Users who hit the
new error get a clear message telling them exactly what to write.
