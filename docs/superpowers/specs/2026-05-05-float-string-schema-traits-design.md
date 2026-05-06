# Float and String Schema Trait Support

**Date**: 2026-05-05
**Issue**: #74 (f32/f64 schema traits missing) + comment (String/&str schema traits missing)
**Scope**: Full f32/f64 support (Reader/Writer + ReadXDR/WriteXDR + schema traits), String/&str schema traits

## Summary

`xdr-repr` documents `f32`/`f64` as the Rust mappings for XDR `float`/`double`, and the AST + formatters already handle `TypeSpec::Float` and `TypeSpec::Double`. But the trait layer (`HasTypeSpec`, `Declare`) omits them, and the runtime layer (`ReadXDR`, `WriteXDR`) has no implementations at all. Additionally, `String` and `&str` have runtime codec support but no schema trait impls.

This design adds full support for both.

## Wire Encoding: f32/f64

### TXS (Traditional XDR Serialization)

Straight RFC 4506: IEEE 754 bytes in big-endian order.

- `f32`: 4 bytes, `to_be_bytes()` / `from_be_bytes()`
- `f64`: 8 bytes, `to_be_bytes()` / `from_be_bytes()`

No padding needed (both are already 4-byte aligned).

### CXS (Compact XDR Serialization)

IEEE 754 bytes in little-endian order, matching CXS's existing LE convention for integer encoding. CXS uses variable-width compression for integers; floats are written as raw fixed-size bytes to avoid mangling the IEEE 754 bit pattern through integer compression.

- `f32`: 4 bytes, `to_le_bytes()` / `from_le_bytes()`
- `f64`: 8 bytes, `to_le_bytes()` / `from_le_bytes()`

**Note**: CXS departs from RFC 4506's big-endian convention here, consistent with how CXS already uses little-endian for all integer types. This must be documented in the code.

### NaN / Infinity / Special Values

Passed through faithfully per RFC 4506. No special handling.

## Changes

### 1. `Reader` trait (`src/serialize/reader.rs`)

Add two new **required** methods (required because TXS and CXS need different byte order):

```rust
fn read_f32(&mut self) -> ReadResult<f32>;
fn read_f64(&mut self) -> ReadResult<f64>;
```

### 2. `Writer` trait (`src/serialize/writer.rs`)

Add two new **required** methods:

```rust
fn write_f32(&mut self, n: f32) -> WriteResult<()>;
fn write_f64(&mut self, n: f64) -> WriteResult<()>;
```

### 3. CXS Reader (`src/cxs/buf_reader.rs`)

Implement `read_f32` and `read_f64`: read 4/8 bytes via `read_opaque_fixed`, convert with `from_le_bytes()`.

### 4. CXS Writer (`src/cxs/put.rs`)

Add `write_f32` and `write_f64` to the `Writer<CXS>` blanket impl for `Put`: convert with `to_le_bytes()`, write via `put_bytes()`.

### 5. CXS ConstWriter (`src/cxs/const_writer.rs`)

Add methods:
- `write_f32(&mut self, n: f32)` — writes `n.to_le_bytes()`
- `write_f64(&mut self, n: f64)` — writes `n.to_le_bytes()`
- `size_f32(_n: f32) -> usize` — returns 4
- `size_f64(_n: f64) -> usize` — returns 8

### 6. TXS Reader (`src/txs/buf_reader.rs`)

Implement `read_f32` and `read_f64`: read 4/8 bytes via `get_input`, convert with `from_be_bytes()`.

### 7. TXS Writer (`src/txs/put.rs`)

Add `write_f32` and `write_f64` to the `Writer<TXS>` blanket impl: convert with `to_be_bytes()`, write via `put_bytes()`. Can use the existing `write_int!` macro or standalone impls.

### 8. TXS ConstWriter (`src/txs/const_writer.rs`)

Add methods:
- `write_f32(&mut self, n: f32)` — writes `n.to_be_bytes()`
- `write_f64(&mut self, n: f64)` — writes `n.to_be_bytes()`
- `size_f32(_n: f32) -> usize` — returns 4
- `size_f64(_n: f64) -> usize` — returns 8

### 9. `ReadXDR` impls (`src/read_xdr.rs`)

Add impls for `f32` and `f64`, following the integer macro pattern:

```rust
impl<'r> ReadXDR<'r> for f32 {
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<f32> {
        r.read_f32()
    }
}

impl<'r> ReadXDR<'r> for f64 {
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<f64> {
        r.read_f64()
    }
}
```

These can be added to the existing `construct_int_impls!` macro invocation if the macro shape fits (it dispatches `r.$read_method()`), or as standalone impls.

### 10. `WriteXDR` impls (`src/write_xdr.rs`)

Add impls for `f32` and `f64`, following the integer macro pattern:

```rust
impl WriteXDR for f32 {
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
        w.write_f32(*self)
    }
}

impl WriteXDR for f64 {
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
        w.write_f64(*self)
    }
}
```

### 11. Schema traits (`xdr-repr/src/ty.rs`)

**f32/f64** — add to existing macro tables:

```rust
impl_has_type_spec! {
    // ... existing entries ...
    f32 => TypeSpec::Float,
    f64 => TypeSpec::Double,
}

impl_declare_named! {
    // ... existing entries ...
    f32 => TypeSpec::Float,
    f64 => TypeSpec::Double,
}
```

**String** — new impls (requires `alloc` feature gate):

```rust
impl HasTypeSpec for String {
    fn type_spec() -> TypeSpec {
        TypeSpec::Struct(StructBody(vec![Declaration::String {
            name: ident("unnamed_0"),
            max: Value::Const(Constant::Unsigned(u32::MAX)),
        }]))
    }
}

impl Declare for String {
    fn declare(name: Ident) -> Declaration {
        Declaration::String {
            name,
            max: Value::Const(Constant::Unsigned(u32::MAX)),
        }
    }
}
```

**&str** — same pattern:

```rust
impl HasTypeSpec for &str {
    fn type_spec() -> TypeSpec {
        TypeSpec::Struct(StructBody(vec![Declaration::String {
            name: ident("unnamed_0"),
            max: Value::Const(Constant::Unsigned(u32::MAX)),
        }]))
    }
}

impl Declare for &str {
    fn declare(name: Ident) -> Declaration {
        Declaration::String {
            name,
            max: Value::Const(Constant::Unsigned(u32::MAX)),
        }
    }
}
```

### 12. `ConstWriteXDR` derive (`xdr-derive/src/generate_const.rs`)

The derive macro has hardcoded type dispatch tables at lines 717–761:

- `primitive_size_fn()` — add `"f32" => size_f32`, `"f64" => size_f64`
- `primitive_write_method()` — add `"f32" => write_f32`, `"f64" => write_f64`

## Testing

- **Round-trip tests** for f32/f64 via both CXS and TXS encode/decode: normal values, NaN, infinity, negative zero, subnormals
- **Schema output tests**: `f32` field produces `float field_name;`, `f64` field produces `double field_name;`
- **Schema output tests**: `String` field produces `string field_name<>;`, `&str` field produces `string field_name<>;`
- **ConstWriter tests**: const-context encoding of f32/f64 values for both CXS and TXS
- **Issue #74 PoC**: adapt the reproduction tests to assert success (compilation succeeds, schema output is correct)
