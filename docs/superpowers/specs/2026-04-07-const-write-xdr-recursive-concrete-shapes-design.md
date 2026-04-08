# ConstWriteXDR: Recursive Concrete Shape Support

## Summary

Extend `#[derive(ConstWriteXDR)]` so it can recursively inline const encoding
for concrete container shapes, instead of only handling primitives and a few
special cases at the outermost field level.

This slice keeps `ConstWriteXDR` as inherent-method code generation on the
derived type. It does not introduce a public const trait and does not generate
impls for wrapper types such as `Option<T>` or `&[T]`.

The immediate goal is to make the existing borrowed contract-spec view types in
`sdk_rs` derivable without reshaping them, while also covering the common
concrete recursive shapes that runtime `WriteXDR` already supports.

## Problem

The current derive implementation in `xdr-derive/src/generate_const.rs` makes a
single top-level decision per field:

- primitives map to direct writer calls
- `&str`, `&[u8]`, and `[u8; N]` use bespoke handling
- anything else falls back to `self.field.const_write_*`

That works only when the whole field type itself has generated const methods.
It fails for common recursive shapes such as:

- `Option<Foo>`
- `Option<Option<Foo>>`
- `&[Foo]`
- `[Foo; N]`
- `(Foo, Bar)`
- `Result<Foo, Bar>`

Even when all leaf types are concrete and already const-encodable, the outer
wrapper type does not itself have generated const methods, so the derive fails.

The tempting generic solution would be to express "T can const-encode" as a
bound and then abstract over `T`. On stable Rust this is not viable:

- inherent methods on concrete types cannot be called through an unconstrained
  type parameter
- const traits and `~const` bounds are not available for this use on stable

So the design must stay concrete: recurse over known type syntax and inline the
encoding logic into the generated methods.

## Goals

- Support recursive const encoding for concrete container shapes.
- Allow derives on types with lifetime parameters.
- Allow derives on types with const generics.
- Preserve the current `ConstWriteXDR` public surface:
  - `const_write_cxs`
  - `const_write_txs`
  - `const_cxs_size`
  - `const_txs_size`
- Keep `ConstWriteXDR` as inherent-method codegen, not a trait API.
- Keep generated code localized to the derived type itself.
- Move the implementation toward parity with the concrete shapes already
  supported by runtime `WriteXDR`.

## Non-goals

- No public `ConstWriteXDR` trait.
- No nightly-only const trait design.
- No support for arbitrary type parameters in encoded shapes.
- No support for `#[xdrs(into_type)]` or `#[xdrs(via)]` in const mode.
- No attempt to support alloc-backed generic containers such as `Vec<T>` in
  this slice.
- No wrapper impl generation for `Option<T>`, `Result<T, E>`, slices, arrays,
  or tuples.

## Supported Shape Boundary

The derive should recursively support these concrete shapes in both field
positions and enum-variant payload positions:

- primitives already supported today
- `i128` / `u128`
- `&str`
- `&[u8]`
- `[u8; N]`
- `Option<T>`
- `Result<T, E>`
- `&[T]`
- `[T; N]`
- tuples up to the same arity runtime `WriteXDR` supports
- nested combinations of the above

Leaf handling:

- primitives and existing byte/string special cases emit direct writer or size
  helper calls
- concrete named leaves emit calls to their generated inherent const methods

Examples that should work:

- `Option<Foo>`
- `Option<Option<Foo>>`
- `&[(Foo, Bar)]`
- `Option<&[Foo]>`
- `Option<&[(Foo, Bar, Baz)]>`
- `Blob<'a, const N: usize> { items: &'a [Foo], pad: [u8; N] }`

Examples that should still fail:

- `Foo<T> { value: T }`
- `Foo<T> { items: &'static [T] }`
- `Foo<T> { value: Option<T> }`
- any shape that requires proving a generic type parameter has const-encoding
  capability

## Generics Rule

The existing derive rejects all generics. This slice should narrow that rule:

- allow lifetime parameters
- allow const generics
- reject type parameters

This must apply recursively, not just at the outer derived type boundary. A
field whose syntax tree contains a type parameter anywhere in its encoded shape
must fail with a targeted derive error.

This is the stable-Rust boundary that keeps the implementation honest.

## Code Generation Strategy

The outer derive structure should remain unchanged:

- one inherent impl block on the derived type
- four generated methods on that type

The change is inside those generated methods. Instead of a flat outermost field
matcher, the derive should recurse over `syn::Type` and inline the encoding
logic for the concrete shape.

Conceptually:

`gen_write_expr(access, ty, fmt)` returns tokens that write the value at
`access` with format `fmt`.

`gen_size_expr(access, ty, fmt)` returns tokens that compute the encoded size of
the value at `access` with format `fmt`.

For example, a field of type `Option<&[Foo]>` should expand like:

1. match on the option
2. for `Some(inner)`, write the XDR discriminant
3. recurse into `&[Foo]`
4. write the slice length
5. emit a `while` loop over elements
6. recurse into `Foo`
7. call `Foo`'s generated const methods at the leaf

The important property is that all recursion stays inside the generated method
body. The derive must not synthesize impls for wrapper/container types.

This avoids conflicts and overlap concerns such as:

- `Option<Foo>`
- `Option<Option<Foo>>`

Both should simply nest inline `match` expansions inside the derived type's own
generated methods.

## Shape-Specific Expansion Rules

### `Option<T>`

Write path:

- write `i32` discriminant `1` for `Some`
- recurse into the inner value
- write `i32` discriminant `0` for `None`

Size path:

- `4 + inner_size` for `Some`
- `4` for `None`

This matches runtime `WriteXDR for Option<T>`.

### `Result<T, E>`

Write path:

- write `i32` discriminant `0` for `Ok`
- recurse into the `Ok` payload
- write `i32` discriminant `1` for `Err`
- recurse into the `Err` payload

Size path:

- `4 + ok_size` for `Ok`
- `4 + err_size` for `Err`

This matches runtime `WriteXDR for Result<T, E>`.

### `&[T]`

Write path:

- write the `u32` length prefix
- emit a `while` loop over the slice
- recurse into each element

Size path:

- `4 + sum(element_sizes)`

This is only valid when `T`'s syntax tree is concrete and const-encodable.

### `[T; N]`

Write path:

- emit a `while` loop over the array
- recurse into each element

Size path:

- sum of element sizes across the array

Special case:

- `[u8; N]` continues to use the existing opaque fixed-byte encoding path

### Tuples

Write path:

- destructure or index the tuple
- recurse into each element in order

Size path:

- sum of element sizes in order

Tuple support should match the runtime `WriteXDR` arity limit to keep the two
surfaces aligned.

### Concrete Named Leaves

When a type does not match a special recursive container case but is still a
concrete path shape, the derive should emit the existing leaf behavior:

- call `const_write_cxs` / `const_write_txs`
- call `const_cxs_size` / `const_txs_size`

This is how user-defined derived types compose with each other.

## Unsupported Shapes And Errors

Unsupported shapes should fail at derive time with specific messages anchored to
the offending field or variant payload.

Important errors:

- type parameter encountered in const-encoded shape
- unsupported type form for recursive const encoding
- `into_type` / `via` usage in const mode

The derive should prefer explicit compile errors over silently emitting code
that later fails with vague method-resolution errors.

## Files

Primary implementation file:

- `xdr-derive/src/generate_const.rs`

Likely tests:

- `tests/const_writer.rs`

If compile-fail coverage is needed for rejection rules, add focused UI-style
tests or derive-failure coverage in the existing test strategy used by the
crate.

## Testing

Add red/green coverage for:

- lifetime-only derived types
- const-generic arrays
- `Option<Foo>`
- `Option<Option<Foo>>`
- `Result<Foo, Bar>`
- `&[Foo]`
- `[Foo; N]`
- tuples of derived leaves
- nested combinations such as `Option<&[(Foo, Bar)]>`

Verification should assert both:

- const-generated bytes match runtime encoding
- size helpers match runtime encoded length

Negative coverage should confirm:

- type-parameterized shapes still fail
- the error points at the offending field or payload

## Recommendation

Implement this as a recursive concrete-shape expander inside
`xdr-derive/src/generate_const.rs`, keeping the public API and top-level derive
structure unchanged.

This gives `ConstWriteXDR` the expressive power needed by `sdk_rs` and similar
borrowed view models, while staying within stable Rust and avoiding a premature
const-trait design.
