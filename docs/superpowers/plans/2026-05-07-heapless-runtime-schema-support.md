# Heapless Runtime and Schema Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add generic `heapless::Vec<T, N>` runtime XDR codecs and matching heapless schema trait support for issue #169.

**Architecture:** Keep byte containers byte-opaque: `heapless::Vec<u8, N>` remains XDR variable opaque and `heapless::String<N>` remains XDR string. Add generic `heapless::Vec<T, N>` support only for real XDR element types via `T: ReadXDR` / `T: WriteXDR` at runtime and `T: HasTypeSpec` in `xdr-repr`. Wire the root `heapless` feature into `xdr-repr?/heapless` so `xdrs` users get runtime and schema support together when `repr` is enabled.

**Tech Stack:** Rust 2024, Cargo workspace, `heapless` 0.9.2, existing CXS/TXS test helpers

---

## File Structure

- Modify `tests/arrays.rs`: runtime regression tests for generic heapless vectors and existing byte/string behavior.
- Modify `src/heapless.rs`: root crate `ReadXDR` and `WriteXDR` impls for `heapless::Vec<T, N>` while preserving `Vec<u8, N>` and `String<N>` special cases.
- Modify `Cargo.toml`: forward the root `heapless` feature to `xdr-repr?/heapless`.
- Modify `xdr-repr/Cargo.toml`: add optional `heapless` dependency and feature.
- Modify `tests/repr/derive_arrays.rs`: derive-level schema regression test through the public `xdrs` path.
- Create `xdr-repr/tests/heapless_types.rs`: direct schema trait tests for `xdr-repr` with its `heapless` feature.
- Modify `xdr-repr/src/ty.rs`: feature-gated `Declare` and `HasTypeSpec` impls for heapless types.

---

### Task 1: Add Failing Runtime Tests for Generic `heapless::Vec<T, N>`

**Files:**
- Modify: `tests/arrays.rs`

- [ ] **Step 1: Add the generic heapless runtime tests**

In `tests/arrays.rs`, add these tests immediately after the existing `heapless_vec` test:

```rust
#[cfg(feature = "heapless")]
#[test]
fn heapless_vec_generic_u32() {
    let mut v = heapless::Vec::<u32, 4>::new();
    v.extend_from_slice(&[1, 2, 3]).unwrap();

    CXS::check_codec(&v, &hex!("03010203"));
    TXS::check_codec(&v, &hex!("00000003000000010000000200000003"));
}

#[cfg(feature = "heapless")]
#[test]
fn heapless_vec_generic_rejects_overlength_decode() {
    let txs_err =
        TXS::decode::<heapless::Vec<u32, 2>>(&hex!("00000003000000010000000200000003"))
            .unwrap_err();
    assert!(matches!(txs_err, ReadError::MaxLengthExceeded));

    let cxs_err = CXS::decode::<heapless::Vec<u32, 2>>(&hex!("03010203")).unwrap_err();
    assert!(matches!(cxs_err, ReadError::MaxLengthExceeded));
}
```

- [ ] **Step 2: Run the focused test to verify it fails**

Run:

```bash
cargo test --workspace --all-features --test arrays heapless_vec_generic_u32 -- --nocapture
```

Expected: compile failure saying `heapless::Vec<u32, 4>` does not implement `ReadXDR` and/or `WriteXDR`.

- [ ] **Step 3: Commit the failing runtime test**

```bash
git add tests/arrays.rs
git commit -m "Add generic heapless Vec runtime coverage"
```

---

### Task 2: Implement Generic Heapless Runtime Codecs

**Files:**
- Modify: `src/heapless.rs`
- Test: `tests/arrays.rs`

- [ ] **Step 1: Replace `src/heapless.rs` with the expanded impls**

Replace the full contents of `src/heapless.rs` with:

```rust
use crate::read_xdr::ReadXDR;
use crate::serialize::reader::{ReadError, ReadResult, Reader};
use crate::serialize::writer::{WriteError, WriteResult, Writer};
use crate::valid_u32::*;
use crate::write_xdr::WriteXDR;

use heapless::String;
use heapless::Vec;

impl<'r, const N: usize> ReadXDR<'r> for Vec<u8, N> {
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<Self> {
        let () = ValidU32::<N>::OK;

        let bytes = r.read_opaque_variable(N as u32)?;

        let mut s = Vec::new();
        s.extend_from_slice(bytes)
            .map_err(|_| ReadError::MaxLengthExceeded)?;

        Ok(s)
    }
}

impl<const N: usize> WriteXDR for Vec<u8, N> {
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
        let () = ValidU32::<N>::OK;

        if self.len() > N {
            return Err(WriteError::InvalidArrayLength);
        }

        w.write_opaque_variable(self.as_slice())
    }
}

impl<'r, T, const N: usize> ReadXDR<'r> for Vec<T, N>
where
    T: ReadXDR<'r>,
{
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<Self> {
        let () = ValidU32::<N>::OK;

        let len = r.read_u32()? as usize;
        if len > N {
            return Err(ReadError::MaxLengthExceeded);
        }

        let mut vec = Vec::new();
        for _ in 0..len {
            vec.push(<T as ReadXDR>::read_xdr(r)?)
                .map_err(|_| ReadError::MaxLengthExceeded)?;
        }

        Ok(vec)
    }
}

impl<T, const N: usize> WriteXDR for Vec<T, N>
where
    T: WriteXDR,
{
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
        let () = ValidU32::<N>::OK;

        if self.len() > N {
            return Err(WriteError::InvalidArrayLength);
        }

        w.write_u32(self.len() as u32)?;
        for elt in self {
            elt.write_xdr(w)?;
        }
        Ok(())
    }
}

impl<'r, const N: usize> ReadXDR<'r> for String<N> {
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<Self> {
        let () = ValidU32::<N>::OK;

        let bytes = r.read_opaque_variable(N as u32)?;
        let slice = str::from_utf8(bytes).map_err(|_| ReadError::BadUTF8)?;

        let mut s = String::new();
        s.push_str(slice).map_err(|_| ReadError::MaxLengthExceeded)?;

        Ok(s)
    }
}

impl<const N: usize> WriteXDR for String<N> {
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
        let () = ValidU32::<N>::OK;

        if self.len() > N {
            return Err(WriteError::InvalidArrayLength);
        }

        w.write_opaque_variable(self.as_bytes())
    }
}
```

- [ ] **Step 2: Run the runtime tests**

Run:

```bash
cargo test --workspace --all-features --test arrays heapless -- --nocapture
```

Expected: all heapless tests in `tests/arrays.rs` pass, including existing `heapless_vec` and `heapless_string`.

- [ ] **Step 3: Commit runtime support**

```bash
git add src/heapless.rs tests/arrays.rs
git commit -m "Add generic heapless Vec runtime codecs"
```

---

### Task 3: Wire `heapless` Into `xdr-repr`

**Files:**
- Modify: `Cargo.toml`
- Modify: `xdr-repr/Cargo.toml`

- [ ] **Step 1: Add optional heapless dependency and feature to `xdr-repr`**

In `xdr-repr/Cargo.toml`, change the dependency and feature sections to include `heapless`:

```toml
[dependencies]
xdr-derive = { workspace = true, optional = true }
xdr-common = { workspace = true }
heapless = { workspace = true, optional = true }

[lints.rust]
unsafe_code = "forbid"

[features]
default = []
derive = ["dep:xdr-derive"] # derive the XDR trait
heapless = ["dep:heapless"] # schema traits for types from the `heapless` package
ns = [] # C++-like namespaces for XDR specs
int128 = []
```

- [ ] **Step 2: Forward root heapless feature to `xdr-repr`**

In the root `Cargo.toml`, replace the current `heapless` feature line with:

```toml
heapless = ["dep:heapless", "xdr-repr?/heapless"] # serialization and schema support for types from the `heapless` package
```

- [ ] **Step 3: Verify feature wiring compiles before schema tests**

Run:

```bash
cargo check --workspace --all-features
```

Expected: command succeeds. No schema tests have been added yet.

- [ ] **Step 4: Commit feature wiring**

```bash
git add Cargo.toml xdr-repr/Cargo.toml
git commit -m "Wire heapless feature into xdr-repr"
```

---

### Task 4: Add Failing Heapless Schema Tests

**Files:**
- Modify: `tests/repr/derive_arrays.rs`
- Create: `xdr-repr/tests/heapless_types.rs`

- [ ] **Step 1: Add public derive schema coverage**

In `tests/repr/derive_arrays.rs`, add this module after the existing `vec_variable` module:

```rust
#[cfg(feature = "heapless")]
pub mod heapless_variable {
    use super::*;

    #[derive(ReadXDR, WriteXDR, XDR)]
    struct HeaplessPacket {
        bytes: heapless::Vec<u8, 8>,
        text: heapless::String<16>,
        nums: heapless::Vec<u32, 4>,
    }

    check_format! { derive_heapless_packet
        HeaplessPacket::def()
        =>
            "struct HeaplessPacket {"
            "	opaque bytes<8>;"
            "	string text<16>;"
            "	unsigned int nums<4>;"
            "};"
    }
}
```

- [ ] **Step 2: Add direct `xdr-repr` heapless trait tests**

Create `xdr-repr/tests/heapless_types.rs` with:

```rust
#![cfg(feature = "heapless")]

use xdr_repr::*;

fn only_field_type_json<T: HasTypeSpec>() -> String {
    let TypeSpec::Struct(body) = T::type_spec() else {
        panic!("expected embedded struct type spec");
    };
    assert_eq!(body.fields().len(), 1);
    body.fields()[0].get_type_json()
}

#[test]
fn heapless_vec_u8_declares_bounded_opaque() {
    let decl = <heapless::Vec<u8, 8> as Declare>::declare(ident("bytes"));
    assert_eq!(decl.to_string(), "opaque bytes<8>");
    assert_eq!(
        decl.get_type_json(),
        r#"{ "type": "variable opaque", "max": 8 }"#,
    );
    assert_eq!(
        only_field_type_json::<heapless::Vec<u8, 8>>(),
        r#"{ "type": "variable opaque", "max": 8 }"#,
    );
}

#[test]
fn heapless_string_declares_bounded_string() {
    let decl = <heapless::String<16> as Declare>::declare(ident("text"));
    assert_eq!(decl.to_string(), "string text<16>");
    assert_eq!(
        decl.get_type_json(),
        r#"{ "type": "string", "max": 16 }"#,
    );
    assert_eq!(
        only_field_type_json::<heapless::String<16>>(),
        r#"{ "type": "string", "max": 16 }"#,
    );
}

#[test]
fn heapless_vec_generic_declares_bounded_array() {
    let decl = <heapless::Vec<u32, 4> as Declare>::declare(ident("nums"));
    assert_eq!(decl.to_string(), "unsigned int nums<4>");
    assert_eq!(
        decl.get_type_json(),
        r#"{ "type": "variable array", "element_type": { "type": "unsigned int" }, "max": 4 }"#,
    );
    assert_eq!(
        only_field_type_json::<heapless::Vec<u32, 4>>(),
        r#"{ "type": "variable array", "element_type": { "type": "unsigned int" }, "max": 4 }"#,
    );
}
```

- [ ] **Step 3: Run direct schema tests to verify they fail**

Run:

```bash
cargo test -p xdr-repr --features heapless --test heapless_types -- --nocapture
```

Expected: compile failure saying `heapless::Vec<u8, 8>`, `heapless::String<16>`, and `heapless::Vec<u32, 4>` do not implement `Declare` and/or `HasTypeSpec`.

- [ ] **Step 4: Run public derive schema test to verify it fails**

Run:

```bash
cargo test --workspace --all-features --test repr heapless_variable::derive_heapless_packet -- --nocapture
```

Expected: compile failure from `derive(XDR)` saying the heapless field types do not implement `Declare`.

- [ ] **Step 5: Commit the failing schema tests**

```bash
git add tests/repr/derive_arrays.rs xdr-repr/tests/heapless_types.rs
git commit -m "Add heapless schema coverage"
```

---

### Task 5: Implement Heapless Schema Traits in `xdr-repr`

**Files:**
- Modify: `xdr-repr/src/ty.rs`
- Test: `xdr-repr/tests/heapless_types.rs`
- Test: `tests/repr/derive_arrays.rs`

- [ ] **Step 1: Add feature-gated heapless schema impls**

In `xdr-repr/src/ty.rs`, insert this block after the existing `xdr_common` `const _: () = { ... };` blocks and before the `alloc::vec::Vec` block:

```rust
#[cfg(feature = "heapless")]
const _: () = {
    use heapless::{String, Vec};

    fn heapless_max_value<const N: usize>() -> Value {
        const { assert!(N <= u32::MAX as usize, "N exceeds u32::MAX") }
        Value::Const(Constant::Unsigned(N as u32))
    }

    impl<const N: usize> HasTypeSpec for Vec<u8, N> {
        fn type_spec() -> TypeSpec {
            TypeSpec::Struct(StructBody::new(alloc::vec![Declaration::VariableOpaque {
                name: ident("unnamed_0"),
                max: heapless_max_value::<N>(),
            }]))
        }
    }

    impl<const N: usize> Declare for Vec<u8, N> {
        fn declare(name: Ident) -> Declaration {
            Declaration::VariableOpaque {
                name,
                max: heapless_max_value::<N>(),
            }
        }
    }

    impl<T, const N: usize> HasTypeSpec for Vec<T, N>
    where
        T: HasTypeSpec,
    {
        fn type_spec() -> TypeSpec {
            TypeSpec::Struct(StructBody::new(alloc::vec![Declaration::VariableArray {
                name: ident("unnamed_0"),
                elt_ty: T::type_spec(),
                max: heapless_max_value::<N>(),
            }]))
        }
    }

    impl<T, const N: usize> Declare for Vec<T, N>
    where
        T: HasTypeSpec,
    {
        fn declare(name: Ident) -> Declaration {
            Declaration::VariableArray {
                name,
                elt_ty: T::type_spec(),
                max: heapless_max_value::<N>(),
            }
        }
    }

    impl<const N: usize> HasTypeSpec for String<N> {
        fn type_spec() -> TypeSpec {
            TypeSpec::Struct(StructBody::new(alloc::vec![Declaration::String {
                name: ident("unnamed_0"),
                max: heapless_max_value::<N>(),
            }]))
        }
    }

    impl<const N: usize> Declare for String<N> {
        fn declare(name: Ident) -> Declaration {
            Declaration::String {
                name,
                max: heapless_max_value::<N>(),
            }
        }
    }
};
```

- [ ] **Step 2: Run direct schema tests**

Run:

```bash
cargo test -p xdr-repr --features heapless --test heapless_types -- --nocapture
```

Expected: all tests in `xdr-repr/tests/heapless_types.rs` pass.

- [ ] **Step 3: Run public derive schema test**

Run:

```bash
cargo test --workspace --all-features --test repr heapless_variable::derive_heapless_packet -- --nocapture
```

Expected: `heapless_variable::derive_heapless_packet` passes and formats the schema with bounded opaque, string, and variable-array declarations.

- [ ] **Step 4: Commit schema impls**

```bash
git add xdr-repr/src/ty.rs xdr-repr/tests/heapless_types.rs tests/repr/derive_arrays.rs
git commit -m "Add heapless schema traits"
```

---

### Task 6: Run Full Verification

**Files:**
- Verify only

- [ ] **Step 1: Build all workspace tests without running**

Run:

```bash
cargo test --workspace --all-features --no-run
```

Expected: build succeeds.

- [ ] **Step 2: Run all tests with all features**

Run:

```bash
cargo test --workspace --all-features
```

Expected: all tests pass.

- [ ] **Step 3: Check git status**

Run:

```bash
git status --short
```

Expected: only unrelated pre-existing untracked docs remain, or the worktree is clean if those files were handled outside this task.
