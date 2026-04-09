# xdr-repr Embedded Union Support for `Result`

## Summary

Make embedded unions a first-class shape in `xdr-repr`, then model
`Result<T, E>` as an anonymous inline union instead of a synthetic named type.

The chosen direction is:

- keep `Result<T, E>` nestable like tuples, arrays, and `Option<T>`
- represent `Result<T, E>` as `TypeSpec::Union(Box<UnionBody>)`
- match the existing runtime codec in `xdrs`:
  - discriminant type `int`
  - discriminant field name `which`
  - case `0` for `Ok(T)`
  - case `1` for `Err(E)`
- avoid name mangling and avoid generating top-level synthetic `TypeDef`s

This fixes the current mismatch where runtime `Result` encoding is real, but
repr/spec generation only exposes a holdover placeholder model.

## Problem

The runtime codec already has a concrete `Result<T, E>` wire format.

In `xdrs`:

- `WriteXDR for Result<T, E>` writes discriminant `0` for `Ok` and `1` for
  `Err`, then writes the payload
- `ReadXDR for Result<T, E>` reads the same discriminant and dispatches to the
  matching payload type

So on the wire, `Result<T, E>` is already a tagged union.

The `xdr-repr` side does not model that shape correctly today:

- `xdr-repr::result` is public, but its active `Declare for Result<T, E>` impl
  just delegates to `T`
- embedded unions exist in the AST as `TypeSpec::Union`, but formatting and
  JSON introspection still panic on that variant
- docs currently describe embedded unions as unsupported
- the dedicated `xdr-repr` `Result` test is commented out

That leaves the repo in an inconsistent state:

- runtime encoding says `Result<T, E>` is a union
- repr/spec generation says `Result<T, E>` is effectively just `T`

## Goals

- Make embedded unions usable in `xdr-repr` formatting and JSON introspection.
- Represent `Result<T, E>` as an embedded inline union with no synthetic names.
- Keep `Result<T, E>` composable inside structs, tuples, arrays, options, and
  other inline composite shapes.
- Align `xdr-repr` output with the existing runtime wire format in `xdrs`.
- Add tests that lock in both the generic embedded-union behavior and the
  specific `Result<T, E>` mapping.

## Non-goals

- Do not change the runtime `ReadXDR` / `WriteXDR` encoding for `Result<T, E>`.
- Do not introduce synthetic top-level names such as `ResultBoolUnsigned`.
- Do not add a `Result`-only formatting hack while leaving embedded unions
  unsupported elsewhere.
- Do not broaden this slice into full embedded-enum support. Embedded enums may
  remain unsupported for now.
- Do not remove the public `GetErrorType` helper in this slice.

## Decision

Treat `Result<T, E>` the same way the crate already treats other anonymous
composite shapes:

- tuples use embedded structs
- `Option<T>` uses an embedded optional declaration
- arrays can use embedded struct-like type specs

`Result<T, E>` should therefore use an embedded union.

The concrete representation is:

```xdr
union switch (int which)
{
case 0:
    <T> ok;
case 1:
    <E> err;
}
```

Where:

- `<T>` and `<E>` are produced through the existing `Declare` machinery
- the case payload names are fixed as `ok` and `err`
- if either side collapses to `void`, the case declaration remains `void`

This keeps `Result<T, E>` anonymous, deterministic, and aligned with the
runtime codec.

## Design

### 1. `Result<T, E>` Repr Model

Replace the placeholder `Declare for Result<T, E>` model in
`xdr-repr/src/result.rs` with a real `HasTypeSpec` + `Declare` pair.

`HasTypeSpec for Result<T, E>` should return:

- `TypeSpec::Union(Box<UnionBody { ... }))`

with:

- `discriminant_decl = declare_type("which", TypeSpec::Int)`
- `case_specs = vec![ case 0 => T, case 1 => E ]`
- `default_decl = None`

`Declare for Result<T, E>` should then follow the tuple pattern:

- return `Declaration::Named { name, ty: Self::type_spec() }`

That makes `Result<T, E>` usable as a field type, tuple element, array element,
or any other place that already accepts embedded `TypeSpec`s.

### 2. Embedded Union Formatting

Extend `Format for TypeSpec` in `xdr-repr/src/format.rs` so
`TypeSpec::Union(body)` renders inline instead of panicking.

The inline form should follow the same style already used for named unions:

```xdr
union switch (int which)
{
case 0:
    bool ok;
case 1:
    unsigned int err;
}
```

When used inside a declaration, the surrounding declaration formatter should
continue to append the field name after the inline type body, just as it
already does for embedded structs.

While touching this formatter match, remove the existing fallback panic for the
primitive variants that are already representable in the AST but currently fall
through the `_ => panic!` branch.

### 3. Embedded Union JSON

Extend `TypeSpec::get_type_json_repr()` and the declaration-facing JSON path so
embedded unions serialize as real JSON objects instead of panicking.

The JSON should mirror the existing AST shape closely enough to be useful for
introspection without inventing synthetic names.

Proposed shape:

```json
{
  "type": "union",
  "discriminant": {
    "name": "which",
    "element_type": { "type": "int" }
  },
  "cases": [
    {
      "values": [0],
      "element_type": { "type": "bool" }
    },
    {
      "values": [1],
      "element_type": { "type": "unsigned" }
    }
  ]
}
```

Rules:

- `discriminant` is serialized from the existing `Declaration`
- `values` contains either numeric literals or identifier strings, depending on
  the `Value`
- `element_type` uses the same recursive type-description scheme already used
  elsewhere
- `default` is omitted when `default_decl` is `None`
- when `default_decl` is present, serialize it under `"default"`

This is sufficient for `Result<T, E>` and also makes `TypeSpec::Union` usable
for future embedded-union callers.

### 4. Docs and Public Contract

Update docs and doctests in `xdr-repr/src/lib.rs` and related API docs so they
stop claiming embedded unions always panic.

After this change the supported boundary should be:

- embedded structs: supported
- embedded unions: supported
- embedded enums: still unsupported

The `Result<T, E>` docs in `xdr-repr/src/result.rs` should describe it as an
anonymous inline union matching the runtime codec, not as a name-mangling or
SDK-only hook.

### 5. Compatibility Notes

Keep `GetErrorType` as-is for now.

It is public and may be used by downstream code, but it should no longer be the
core mechanism that explains how `Result<T, E>` is represented. The actual
representation should come directly from `HasTypeSpec` / `Declare`.

## Test-First Plan

Write the tests first and watch them fail before implementation.

### `xdr-repr/tests/result.rs`

Replace the commented placeholder with active coverage for:

- `Result<(), ()>` type-spec shape
- `Result<bool, u32>` formatting
- `Result<bool, u32>` JSON
- a nested declaration such as `declare_type("value", <Result<bool, u32>>::type_spec())`

### `xdr-repr/tests/json_validity.rs`

Add embedded-union validity coverage:

- `TypeSpec::Union(...)` now produces valid JSON
- nested struct fields containing embedded unions produce valid JSON
- `Result<T, E>` JSON contains the expected discriminant and case structure

Update any existing doctest- or panic-oriented assumptions about embedded
unions accordingly.

### `tests/repr/...`

Add or extend derive-facing repr tests to prove that a derived type using a
`Result<T, E>` field emits inline union structure rather than pretending the
field is just `T`.

Representative example:

```rust
#[derive(XDR)]
struct Wrapper {
    choice: Result<bool, u32>,
}
```

The formatted XDR spec for `Wrapper` should contain an inline union field with
cases `0` and `1`.

## Validation

Before implementation:

- new `Result` / embedded-union repr tests should fail for the right reason
  rather than by typo or unrelated breakage

After implementation:

- `cargo test -p xdr-repr`
- `cargo test --test repr --features "repr derive alloc"`
- `cargo test --workspace --all-features`

should all pass.

Also verify:

- embedded union formatting no longer panics
- embedded union JSON no longer panics
- `Result<T, E>` repr output matches the runtime discriminant mapping

## Risks

- Inline union formatting inside declarations must stay syntactically coherent
  with the existing formatter output for embedded structs.
- The JSON shape must be stable enough to be useful without overfitting to a
  one-off `Result` case.
- `Declare` for `Result<T, E>` must preserve the current behavior of special
  inner types such as tuples, `Option<T>`, arrays, and `()`.
- This slice should not accidentally claim embedded enums are supported unless
  they are implemented and tested too.
