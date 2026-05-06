# Float and String Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add full f32/f64 codec + schema support and String/&str schema traits (issue #74).

**Architecture:** Extend the Reader/Writer traits with `read_f32`/`read_f64`/`write_f32`/`write_f64`, implement them in both TXS (big-endian) and CXS (little-endian) formats including ConstWriter, wire up ReadXDR/WriteXDR, add HasTypeSpec/Declare impls for f32/f64/String/&str in xdr-repr, and add f32/f64 to the ConstWriteXDR derive's type tables.

**Tech Stack:** Rust, no new dependencies

---

### Task 1: Add f32/f64 to Writer trait and TXS writer

**Files:**
- Modify: `src/serialize/writer.rs:37-65` (Writer trait)
- Modify: `src/txs/put.rs:26-81` (TXS Writer blanket impl)

- [ ] **Step 1: Write the failing test**

Note: `f32`/`f64` do not implement `Eq`, so `check_codec` cannot be used. Use `check_encode` for encode verification and a separate `to_bits()` comparison for decode verification.

Add to `tests/ints.rs` inside the existing `mod txs` block, after the last `enc_test_int!` invocation:

```rust
    #[test]
    fn f32_one() {
        let val: f32 = 1.0;
        // IEEE 754 big-endian: 3F800000
        let expected = &hex::decode("3f800000").unwrap();
        TXS::check_encode(&val, expected);
        let decoded: f32 = TXS::decode(expected).unwrap();
        assert_eq!(decoded.to_bits(), val.to_bits());
    }

    #[test]
    fn f64_one() {
        let val: f64 = 1.0;
        // IEEE 754 big-endian: 3FF0000000000000
        let expected = &hex::decode("3ff0000000000000").unwrap();
        TXS::check_encode(&val, expected);
        let decoded: f64 = TXS::decode(expected).unwrap();
        assert_eq!(decoded.to_bits(), val.to_bits());
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --workspace --all-features --test ints txs::f32_one -- --nocapture 2>&1 | head -20`
Expected: compilation failure — `Writer` has no `write_f32`, `ReadXDR`/`WriteXDR` not implemented for `f32`.

- [ ] **Step 3: Add write_f32/write_f64 to Writer trait**

In `src/serialize/writer.rs`, add these two required methods to the `Writer<F>` trait, after `write_bool`:

```rust
    /// Writes a 32-bit IEEE 754 floating-point value.
    fn write_f32(&mut self, n: f32) -> WriteResult<()>;

    /// Writes a 64-bit IEEE 754 floating-point value.
    fn write_f64(&mut self, n: f64) -> WriteResult<()>;
```

- [ ] **Step 4: Implement write_f32/write_f64 in TXS Writer**

In `src/txs/put.rs`, add to the `impl<P> Writer<TXS> for P` block, after the `write_int!` macro invocation:

```rust
    #[inline]
    fn write_f32(&mut self, n: f32) -> WriteResult<()> {
        let bytes: [u8; 4] = n.to_be_bytes();
        self.put_bytes(&bytes)
    }

    #[inline]
    fn write_f64(&mut self, n: f64) -> WriteResult<()> {
        let bytes: [u8; 8] = n.to_be_bytes();
        self.put_bytes(&bytes)
    }
```

- [ ] **Step 5: Verify compilation fails (CXS writer not yet updated)**

Run: `cargo check --workspace --all-features 2>&1 | head -20`
Expected: compilation error in CXS — `Writer<CXS>` doesn't implement `write_f32`/`write_f64`.

- [ ] **Step 6: Commit**

```bash
git add src/serialize/writer.rs src/txs/put.rs tests/ints.rs
git commit -m "Add write_f32/write_f64 to Writer trait; implement for TXS"
```

---

### Task 2: Add f32/f64 to CXS writer

**Files:**
- Modify: `src/cxs/put.rs:26-128` (CXS Writer blanket impl)

- [ ] **Step 1: Write the failing test**

Add to `tests/ints.rs` inside the existing `mod cxs` block (not inside `mod non_minimal`), after the last test:

```rust
    #[test]
    fn f32_one() {
        let val: f32 = 1.0;
        // IEEE 754 little-endian: 0000803f
        let expected = &hex::decode("0000803f").unwrap();
        CXS::check_encode(&val, expected);
        let decoded: f32 = CXS::decode(expected).unwrap();
        assert_eq!(decoded.to_bits(), val.to_bits());
    }

    #[test]
    fn f64_one() {
        let val: f64 = 1.0;
        // IEEE 754 little-endian: 000000000000f03f
        let expected = &hex::decode("000000000000f03f").unwrap();
        CXS::check_encode(&val, expected);
        let decoded: f64 = CXS::decode(expected).unwrap();
        assert_eq!(decoded.to_bits(), val.to_bits());
    }
```

- [ ] **Step 2: Implement write_f32/write_f64 in CXS Writer**

In `src/cxs/put.rs`, add to the `impl<P> Writer<CXS> for P` block, after `write_u16`:

```rust
    /// Writes a 32-bit IEEE 754 float as raw little-endian bytes.
    ///
    /// Unlike integers, floats are not compressed with CXS variable-width
    /// encoding — the bit pattern is written as-is. Little-endian byte order
    /// follows CXS convention (not RFC 4506 big-endian).
    #[inline]
    fn write_f32(&mut self, n: f32) -> WriteResult<()> {
        let bytes: [u8; 4] = n.to_le_bytes();
        self.put_bytes(&bytes)
    }

    /// Writes a 64-bit IEEE 754 double as raw little-endian bytes.
    ///
    /// Unlike integers, floats are not compressed with CXS variable-width
    /// encoding — the bit pattern is written as-is. Little-endian byte order
    /// follows CXS convention (not RFC 4506 big-endian).
    #[inline]
    fn write_f64(&mut self, n: f64) -> WriteResult<()> {
        let bytes: [u8; 8] = n.to_le_bytes();
        self.put_bytes(&bytes)
    }
```

- [ ] **Step 3: Verify compilation succeeds but tests still fail (no ReadXDR/WriteXDR yet)**

Run: `cargo check --workspace --all-features 2>&1 | tail -5`
Expected: compilation succeeds (or fails only due to missing ReadXDR/WriteXDR for f32/f64).

- [ ] **Step 4: Commit**

```bash
git add src/cxs/put.rs tests/ints.rs
git commit -m "Implement write_f32/write_f64 for CXS writer"
```

---

### Task 3: Add f32/f64 to Reader trait, TXS reader, and CXS reader

**Files:**
- Modify: `src/serialize/reader.rs:27-127` (Reader trait)
- Modify: `src/txs/buf_reader.rs:104-163` (TXS BufReader)
- Modify: `src/cxs/buf_reader.rs:224-252` (CXS BufReader)

- [ ] **Step 1: Add read_f32/read_f64 to Reader trait**

In `src/serialize/reader.rs`, add these two required methods to the `Reader<'r>` trait, after `read_u16`:

```rust
    /// Reads a 32-bit IEEE 754 floating-point value.
    fn read_f32(&mut self) -> ReadResult<f32>;

    /// Reads a 64-bit IEEE 754 floating-point value.
    fn read_f64(&mut self) -> ReadResult<f64>;
```

- [ ] **Step 2: Implement in TXS BufReader**

In `src/txs/buf_reader.rs`, add to the `impl<'r> Reader<'r> for BufReader<'r>` block, after the `read_int!` macro invocation:

```rust
    #[inline]
    fn read_f32(&mut self) -> ReadResult<f32> {
        let input = self.get_input(4)?;
        let mut bytes = [0u8; 4];
        bytes.copy_from_slice(input);
        Ok(f32::from_be_bytes(bytes))
    }

    #[inline]
    fn read_f64(&mut self) -> ReadResult<f64> {
        let input = self.get_input(8)?;
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(input);
        Ok(f64::from_be_bytes(bytes))
    }
```

- [ ] **Step 3: Implement in CXS BufReader**

In `src/cxs/buf_reader.rs`, add to the `impl<'r> Reader<'r> for BufReader<'r>` block, after the `read_unsigned!` macro invocation:

```rust
    /// Reads a 32-bit IEEE 754 float from raw little-endian bytes.
    ///
    /// CXS encodes floats as fixed-size raw bytes (not variable-width
    /// compressed). Little-endian byte order follows CXS convention
    /// (not RFC 4506 big-endian).
    #[inline]
    fn read_f32(&mut self) -> ReadResult<f32> {
        let input = self.read_opaque_fixed(4)?;
        let mut bytes = [0u8; 4];
        bytes.copy_from_slice(input);
        Ok(f32::from_le_bytes(bytes))
    }

    /// Reads a 64-bit IEEE 754 double from raw little-endian bytes.
    ///
    /// CXS encodes floats as fixed-size raw bytes (not variable-width
    /// compressed). Little-endian byte order follows CXS convention
    /// (not RFC 4506 big-endian).
    #[inline]
    fn read_f64(&mut self) -> ReadResult<f64> {
        let input = self.read_opaque_fixed(8)?;
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(input);
        Ok(f64::from_le_bytes(bytes))
    }
```

- [ ] **Step 4: Verify compilation succeeds (Reader/Writer complete, but ReadXDR/WriteXDR still missing)**

Run: `cargo check --workspace --all-features 2>&1 | tail -5`
Expected: compiles (test file still won't compile due to missing ReadXDR/WriteXDR).

- [ ] **Step 5: Commit**

```bash
git add src/serialize/reader.rs src/txs/buf_reader.rs src/cxs/buf_reader.rs
git commit -m "Add read_f32/read_f64 to Reader trait; implement for TXS and CXS"
```

---

### Task 4: Add ReadXDR/WriteXDR impls for f32/f64

**Files:**
- Modify: `src/read_xdr.rs:87-110` (ReadXDR impls)
- Modify: `src/write_xdr.rs:60-83` (WriteXDR impls)

- [ ] **Step 1: Add ReadXDR impls for f32/f64**

In `src/read_xdr.rs`, add to the `construct_int_impls!` macro invocation (after the `read_bool for bool` line):

```rust
    read_f32 for f32,
    read_f64 for f64,
```

- [ ] **Step 2: Add WriteXDR impls for f32/f64**

In `src/write_xdr.rs`, add to the `write_xdr_int_impls!` macro invocation (after the `write_bool for bool` line):

```rust
    write_f32 for f32,
    write_f64 for f64,
```

- [ ] **Step 3: Run tests to verify TXS and CXS f32/f64 tests pass**

Run: `cargo test --workspace --all-features --test ints -- f32 f64 --nocapture 2>&1`
Expected: all 4 float tests pass (txs::f32_one, txs::f64_one, cxs::f32_one, cxs::f64_one).

- [ ] **Step 4: Add more float test cases**

Add to `tests/ints.rs` inside `mod txs`, after `f64_one`:

```rust
    #[test]
    fn f32_zero() {
        let val: f32 = 0.0;
        let expected = &hex::decode("00000000").unwrap();
        TXS::check_encode(&val, expected);
        let decoded: f32 = TXS::decode(expected).unwrap();
        assert_eq!(decoded.to_bits(), val.to_bits());
    }

    #[test]
    fn f32_neg_zero() {
        let val: f32 = -0.0;
        let expected = &hex::decode("80000000").unwrap();
        TXS::check_encode(&val, expected);
        let decoded: f32 = TXS::decode(expected).unwrap();
        assert_eq!(decoded.to_bits(), val.to_bits());
    }

    #[test]
    fn f32_infinity() {
        let val: f32 = f32::INFINITY;
        let expected = &hex::decode("7f800000").unwrap();
        TXS::check_encode(&val, expected);
        let decoded: f32 = TXS::decode(expected).unwrap();
        assert_eq!(decoded.to_bits(), val.to_bits());
    }

    #[test]
    fn f32_neg_infinity() {
        let val: f32 = f32::NEG_INFINITY;
        let expected = &hex::decode("ff800000").unwrap();
        TXS::check_encode(&val, expected);
        let decoded: f32 = TXS::decode(expected).unwrap();
        assert_eq!(decoded.to_bits(), val.to_bits());
    }

    #[test]
    fn f64_zero() {
        let val: f64 = 0.0;
        let expected = &hex::decode("0000000000000000").unwrap();
        TXS::check_encode(&val, expected);
        let decoded: f64 = TXS::decode(expected).unwrap();
        assert_eq!(decoded.to_bits(), val.to_bits());
    }

    #[test]
    fn f64_neg_zero() {
        let val: f64 = -0.0;
        let expected = &hex::decode("8000000000000000").unwrap();
        TXS::check_encode(&val, expected);
        let decoded: f64 = TXS::decode(expected).unwrap();
        assert_eq!(decoded.to_bits(), val.to_bits());
    }

    #[test]
    fn f64_infinity() {
        let val: f64 = f64::INFINITY;
        let expected = &hex::decode("7ff0000000000000").unwrap();
        TXS::check_encode(&val, expected);
        let decoded: f64 = TXS::decode(expected).unwrap();
        assert_eq!(decoded.to_bits(), val.to_bits());
    }

    #[test]
    fn f64_neg_infinity() {
        let val: f64 = f64::NEG_INFINITY;
        let expected = &hex::decode("fff0000000000000").unwrap();
        TXS::check_encode(&val, expected);
        let decoded: f64 = TXS::decode(expected).unwrap();
        assert_eq!(decoded.to_bits(), val.to_bits());
    }
```

Add to `tests/ints.rs` inside `mod cxs`, after `f64_one`:

```rust
    #[test]
    fn f32_zero() {
        let val: f32 = 0.0;
        let expected = &hex::decode("00000000").unwrap();
        CXS::check_encode(&val, expected);
        let decoded: f32 = CXS::decode(expected).unwrap();
        assert_eq!(decoded.to_bits(), val.to_bits());
    }

    #[test]
    fn f32_neg_zero() {
        let val: f32 = -0.0;
        let expected = &hex::decode("00000080").unwrap();
        CXS::check_encode(&val, expected);
        let decoded: f32 = CXS::decode(expected).unwrap();
        assert_eq!(decoded.to_bits(), val.to_bits());
    }

    #[test]
    fn f32_infinity() {
        let val: f32 = f32::INFINITY;
        let expected = &hex::decode("0000807f").unwrap();
        CXS::check_encode(&val, expected);
        let decoded: f32 = CXS::decode(expected).unwrap();
        assert_eq!(decoded.to_bits(), val.to_bits());
    }

    #[test]
    fn f64_zero() {
        let val: f64 = 0.0;
        let expected = &hex::decode("0000000000000000").unwrap();
        CXS::check_encode(&val, expected);
        let decoded: f64 = CXS::decode(expected).unwrap();
        assert_eq!(decoded.to_bits(), val.to_bits());
    }

    #[test]
    fn f64_neg_zero() {
        let val: f64 = -0.0;
        let expected = &hex::decode("0000000000000080").unwrap();
        CXS::check_encode(&val, expected);
        let decoded: f64 = CXS::decode(expected).unwrap();
        assert_eq!(decoded.to_bits(), val.to_bits());
    }

    #[test]
    fn f64_infinity() {
        let val: f64 = f64::INFINITY;
        let expected = &hex::decode("000000000000f07f").unwrap();
        CXS::check_encode(&val, expected);
        let decoded: f64 = CXS::decode(expected).unwrap();
        assert_eq!(decoded.to_bits(), val.to_bits());
    }
```

- [ ] **Step 5: Run all float tests**

Run: `cargo test --workspace --all-features --test ints -- f32 f64 --nocapture 2>&1`
Expected: all float tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/read_xdr.rs src/write_xdr.rs tests/ints.rs
git commit -m "Add ReadXDR/WriteXDR impls for f32/f64 with round-trip tests"
```

---

### Task 5: Add f32/f64 to ConstWriters and ConstWriteXDR derive

**Files:**
- Modify: `src/cxs/const_writer.rs` (CXS ConstWriter)
- Modify: `src/txs/const_writer.rs` (TXS ConstWriter)
- Modify: `xdr-derive/src/generate_const.rs:717-761` (ConstWriteXDR derive type tables)

- [ ] **Step 1: Write the failing test**

Add to `tests/const_writer.rs`, after the last `#[test]` function:

```rust
// -- f32/f64 cross-validation --

const TXS_F32_ONE: [u8; 4] = {
    let mut w = xdrs::txs::ConstWriter::<4>::new();
    w.write_f32(1.0);
    w.into_bytes()
};

const TXS_F64_ONE: [u8; 8] = {
    let mut w = xdrs::txs::ConstWriter::<8>::new();
    w.write_f64(1.0);
    w.into_bytes()
};

const CXS_F32_ONE: [u8; 4] = {
    let mut w = xdrs::cxs::ConstWriter::<4>::new();
    w.write_f32(1.0);
    w.into_bytes()
};

const CXS_F64_ONE: [u8; 8] = {
    let mut w = xdrs::cxs::ConstWriter::<8>::new();
    w.write_f64(1.0);
    w.into_bytes()
};

#[test]
fn txs_cross_validate_f32() {
    assert_eq!(&TXS_F32_ONE, txs_runtime_encode(&1.0f32).as_slice());
}

#[test]
fn txs_cross_validate_f64() {
    assert_eq!(&TXS_F64_ONE, txs_runtime_encode(&1.0f64).as_slice());
}

#[test]
fn cxs_cross_validate_f32() {
    assert_eq!(&CXS_F32_ONE, cxs_runtime_encode(&1.0f32).as_slice());
}

#[test]
fn cxs_cross_validate_f64() {
    assert_eq!(&CXS_F64_ONE, cxs_runtime_encode(&1.0f64).as_slice());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --workspace --all-features --test const_writer -- f32 f64 --nocapture 2>&1 | head -20`
Expected: compilation failure — `ConstWriter` has no `write_f32`/`write_f64`.

- [ ] **Step 3: Add f32/f64 to TXS ConstWriter**

In `src/txs/const_writer.rs`, add after `write_bool`:

```rust
    /// Writes a 32-bit IEEE 754 float in big-endian format (4 bytes).
    pub const fn write_f32(&mut self, n: f32) {
        let b = n.to_be_bytes();
        self.put_bytes(&b);
    }

    /// Writes a 64-bit IEEE 754 double in big-endian format (8 bytes).
    pub const fn write_f64(&mut self, n: f64) {
        let b = n.to_be_bytes();
        self.put_bytes(&b);
    }
```

Add after `size_bool`:

```rust
    /// Returns the TXS encoded size of a 32-bit float (always 4).
    pub const fn size_f32(_n: f32) -> usize {
        4
    }

    /// Returns the TXS encoded size of a 64-bit double (always 8).
    pub const fn size_f64(_n: f64) -> usize {
        8
    }
```

- [ ] **Step 4: Add f32/f64 to CXS ConstWriter**

In `src/cxs/const_writer.rs`, add after `write_bool`:

```rust
    /// Writes a 32-bit IEEE 754 float as raw little-endian bytes (4 bytes).
    ///
    /// Unlike integers, floats are not compressed with CXS variable-width
    /// encoding — the bit pattern is written as-is. Little-endian byte order
    /// follows CXS convention (not RFC 4506 big-endian).
    pub const fn write_f32(&mut self, n: f32) {
        let b = n.to_le_bytes();
        self.put_bytes(&b);
    }

    /// Writes a 64-bit IEEE 754 double as raw little-endian bytes (8 bytes).
    ///
    /// Unlike integers, floats are not compressed with CXS variable-width
    /// encoding — the bit pattern is written as-is. Little-endian byte order
    /// follows CXS convention (not RFC 4506 big-endian).
    pub const fn write_f64(&mut self, n: f64) {
        let b = n.to_le_bytes();
        self.put_bytes(&b);
    }
```

Add after `size_bool`:

```rust
    /// Returns the CXS encoded size of a 32-bit float (always 4).
    pub const fn size_f32(_n: f32) -> usize {
        4
    }

    /// Returns the CXS encoded size of a 64-bit double (always 8).
    pub const fn size_f64(_n: f64) -> usize {
        8
    }
```

- [ ] **Step 5: Add f32/f64 to ConstWriteXDR derive type tables**

In `xdr-derive/src/generate_const.rs`, in the `primitive_size_fn` function (around line 726), add before the `_ => None` arm:

```rust
        "f32" => Some(quote! { size_f32 }),
        "f64" => Some(quote! { size_f64 }),
```

In the `primitive_write_method` function (around line 749), add before the `_ => None` arm:

```rust
        "f32" => Some(quote! { write_f32 }),
        "f64" => Some(quote! { write_f64 }),
```

- [ ] **Step 6: Run ConstWriter tests**

Run: `cargo test --workspace --all-features --test const_writer -- f32 f64 --nocapture 2>&1`
Expected: all 4 cross-validation tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/cxs/const_writer.rs src/txs/const_writer.rs xdr-derive/src/generate_const.rs tests/const_writer.rs
git commit -m "Add f32/f64 to ConstWriters and ConstWriteXDR derive"
```

---

### Task 6: Add HasTypeSpec/Declare for f32 and f64

**Files:**
- Modify: `xdr-repr/src/ty.rs:90-138` (schema trait macro tables)

- [ ] **Step 1: Write the failing test**

Add a new file `tests/repr/derive_floats.rs`:

```rust
use xdr_repr::*;
use xdrs::*;

#[derive(XDR)]
pub struct Measurement {
    pub temperature: f32,
    pub ratio: f64,
}

check_format! { derive_float_struct
    Measurement::def()
    =>
        "struct Measurement {"
        "	float temperature;"
        "	double ratio;"
        "};"
}
```

Add `mod derive_floats;` to `tests/repr/main.rs`.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --workspace --all-features --test repr derive_float_struct -- --nocapture 2>&1 | head -20`
Expected: compilation failure — `f32: HasTypeSpec` / `f32: Declare` not satisfied.

- [ ] **Step 3: Add f32/f64 to HasTypeSpec and Declare macro tables**

In `xdr-repr/src/ty.rs`, add `f32` and `f64` to the `impl_has_type_spec!` invocation (after `bool => TypeSpec::Bool,`):

```rust
    f32 => TypeSpec::Float,
    f64 => TypeSpec::Double,
```

Add `f32` and `f64` to the `impl_declare_named!` invocation (after `bool => TypeSpec::Bool,`):

```rust
    f32 => TypeSpec::Float,
    f64 => TypeSpec::Double,
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --workspace --all-features --test repr derive_float_struct -- --nocapture 2>&1`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add xdr-repr/src/ty.rs tests/repr/derive_floats.rs tests/repr/main.rs
git commit -m "Add HasTypeSpec/Declare for f32 and f64"
```

---

### Task 7: Add HasTypeSpec/Declare for String and &str

**Files:**
- Modify: `xdr-repr/src/ty.rs` (add String and &str schema impls)

- [ ] **Step 1: Write the failing test**

Add to `tests/repr/derive_structs.rs`, at the end:

```rust
pub mod string_fields {
    use super::*;

    #[derive(XDR)]
    pub struct Named {
        pub label: String,
    }

    check_format! { derive_string_field
        Named::def()
        =>
            "struct Named {"
            "	string label<>;"
            "};"
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --workspace --all-features --test repr derive_string_field -- --nocapture 2>&1 | head -20`
Expected: compilation failure — `String: Declare` not satisfied.

- [ ] **Step 3: Add Declare and HasTypeSpec for String**

In `xdr-repr/src/ty.rs`, find the existing `const _: ()` block that imports `Vec` (around line 316). Add the `String` impls inside it, after the `Vec<T>` impls:

```rust
    impl HasTypeSpec for alloc::string::String {
        fn type_spec() -> TypeSpec {
            TypeSpec::Struct(StructBody(alloc::vec![Declaration::String {
                name: ident("unnamed_0"),
                max: Value::Const(Constant::Unsigned(u32::MAX)),
            }]))
        }
    }

    impl Declare for alloc::string::String {
        fn declare(name: Ident) -> Declaration {
            Declaration::String {
                name,
                max: Value::Const(Constant::Unsigned(u32::MAX)),
            }
        }
    }
```

- [ ] **Step 4: Add Declare and HasTypeSpec for &str**

In `xdr-repr/src/ty.rs`, add after the `Declare for &[u8]` impl (around line 244):

```rust
impl HasTypeSpec for &str {
    fn type_spec() -> TypeSpec {
        TypeSpec::Struct(StructBody(alloc::vec![Declaration::String {
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

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test --workspace --all-features --test repr derive_string_field -- --nocapture 2>&1`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add xdr-repr/src/ty.rs tests/repr/derive_structs.rs
git commit -m "Add HasTypeSpec/Declare for String and &str"
```

---

### Task 8: Full test suite and final validation

**Files:**
- Modify: `tests/ints.rs` (NaN round-trip tests)
- Modify: `tests/repr/derive_floats.rs` (JSON schema test)

- [ ] **Step 1: Add NaN round-trip tests**

Add to `tests/ints.rs` inside `mod txs`:

```rust
    #[test]
    fn f32_nan_roundtrip() {
        let val: f32 = f32::NAN;
        let encoded = TXS::encode(&val).unwrap();
        let decoded: f32 = TXS::decode(&encoded).unwrap();
        assert!(decoded.is_nan());
    }

    #[test]
    fn f64_nan_roundtrip() {
        let val: f64 = f64::NAN;
        let encoded = TXS::encode(&val).unwrap();
        let decoded: f64 = TXS::decode(&encoded).unwrap();
        assert!(decoded.is_nan());
    }
```

Add to `tests/ints.rs` inside `mod cxs`:

```rust
    #[test]
    fn f32_nan_roundtrip() {
        let val: f32 = f32::NAN;
        let encoded = CXS::encode(&val).unwrap();
        let decoded: f32 = CXS::decode(&encoded).unwrap();
        assert!(decoded.is_nan());
    }

    #[test]
    fn f64_nan_roundtrip() {
        let val: f64 = f64::NAN;
        let encoded = CXS::encode(&val).unwrap();
        let decoded: f64 = CXS::decode(&encoded).unwrap();
        assert!(decoded.is_nan());
    }
```

- [ ] **Step 2: Add JSON schema test for floats**

Add to `tests/repr/derive_floats.rs`:

```rust
#[test]
fn float_json_schema() {
    let type_def = Measurement::type_def();
    if let TypeDef::Struct(_, body) = type_def {
        let json = TypeSpec::Struct(body).get_type_json();
        assert!(json.contains("\"type\": \"float\""), "expected float type in JSON: {json}");
        assert!(json.contains("\"type\": \"double\""), "expected double type in JSON: {json}");
    } else {
        panic!("expected struct TypeDef");
    }
}
```

- [ ] **Step 3: Run the full test suite**

Run: `cargo test --workspace --all-features 2>&1 | tail -20`
Expected: all tests pass, zero failures.

- [ ] **Step 4: Commit**

```bash
git add tests/ints.rs tests/repr/derive_floats.rs
git commit -m "Add NaN round-trip and JSON schema tests for float support"
```
