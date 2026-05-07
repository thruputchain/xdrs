# Reject Implicit Discriminants with External Discriminant Type — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Emit a compile error when `#[xdr(discrim_type = "...")]` is used on an enum with implicit discriminants, preventing runtime/schema discriminant drift (#167).

**Architecture:** Add a `has_xdr_discrim_type()` helper in `util.rs` that inspects raw `syn` attributes, extend `check_variants()` with a `has_discrim_type: bool` parameter and a new validation clause, then update all four call sites. Test with a compile-fail `trybuild` test and a positive round-trip regression test.

**Tech Stack:** Rust, syn 2.x, darling, proc-macro2, trybuild (new dev-dependency)

---

### Task 1: Add `has_xdr_discrim_type` helper and extend `check_variants`

**Files:**
- Modify: `xdr-derive/src/util.rs:129-174`

- [ ] **Step 1: Write the `has_xdr_discrim_type` function**

Add this function above `check_variants` in `xdr-derive/src/util.rs` (after `first_field_is_i32`, before `check_variants`):

```rust
pub fn has_xdr_discrim_type(input: &syn::DeriveInput) -> bool {
    input.attrs.iter().any(|attr| {
        if !attr.path().is_ident("xdr") {
            return false;
        }
        let mut found = false;
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("discrim_type") {
                found = true;
            }
            Ok(())
        });
        found
    })
}
```

- [ ] **Step 2: Add `has_discrim_type` parameter to `check_variants` and add the new validation clause**

Change the signature from:

```rust
pub fn check_variants(input: &syn::DeriveInput, item: &syn::DataEnum) -> Result<()> {
```

to:

```rust
pub fn check_variants(
    input: &syn::DeriveInput,
    item: &syn::DataEnum,
    has_discrim_type: bool,
) -> Result<()> {
```

Then add this clause immediately after the `n_implicit` computation (line 136) and before the existing `if n_explicit > 0 && n_implicit > 0` check:

```rust
    if has_discrim_type && n_implicit > 0 {
        return err(
            input,
            "derive XDR: all payload variants must have explicit discriminants \
             when `discrim_type` is set \
             (e.g. `VariantName { ... } = ExternalEnum::VariantName as i32`)",
        );
    }
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p xdr-derive`
Expected: FAIL — four call sites now have the wrong arity. This confirms the signature change propagated.

- [ ] **Step 4: Commit**

```bash
git add xdr-derive/src/util.rs
git commit -m "Add has_xdr_discrim_type helper; extend check_variants with discrim_type gate"
```

---

### Task 2: Update all four `check_variants` call sites

**Files:**
- Modify: `xdr-derive/src/generate_type.rs:138`
- Modify: `xdr-derive/src/generate.rs:150,396`
- Modify: `xdr-derive/src/generate_const.rs:402`

- [ ] **Step 1: Update `generate_type.rs:from_enum`**

Change line 138 from:

```rust
    check_variants(input, item)?;
```

to:

```rust
    check_variants(input, item, has_xdr_discrim_type(input))?;
```

- [ ] **Step 2: Update `generate.rs:write_enum`**

Change line 150 from:

```rust
    check_variants(input, item)?;
```

to:

```rust
    check_variants(input, item, has_xdr_discrim_type(input))?;
```

- [ ] **Step 3: Update `generate.rs:read_enum`**

Change line 396 from:

```rust
    check_variants(input, item)?;
```

to:

```rust
    check_variants(input, item, has_xdr_discrim_type(input))?;
```

- [ ] **Step 4: Update `generate_const.rs:const_write_enum`**

Change line 402 from:

```rust
    check_variants(input, item)?;
```

to:

```rust
    check_variants(input, item, has_xdr_discrim_type(input))?;
```

- [ ] **Step 5: Verify it compiles and existing tests pass**

Run: `cargo test --workspace --all-features`
Expected: All existing tests PASS. No existing code uses `discrim_type` with implicit discriminants.

- [ ] **Step 6: Commit**

```bash
git add xdr-derive/src/generate_type.rs xdr-derive/src/generate.rs xdr-derive/src/generate_const.rs
git commit -m "Thread has_xdr_discrim_type through all check_variants call sites"
```

---

### Task 3: Add compile-fail test

**Files:**
- Modify: `Cargo.toml` (add trybuild dev-dependency)
- Create: `tests/compile_fail.rs`
- Create: `tests/compile_fail/implicit_discrim_with_external_type.rs`
- Create: `tests/compile_fail/implicit_discrim_with_external_type.stderr`

- [ ] **Step 1: Add trybuild dev-dependency**

In the root `Cargo.toml`, add to `[dev-dependencies]`:

```toml
trybuild = "1"
```

- [ ] **Step 2: Create the compile-fail test harness**

Create `tests/compile_fail.rs`:

```rust
#[test]
fn compile_fail() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile_fail/*.rs");
}
```

- [ ] **Step 3: Create the compile-fail test case**

Create `tests/compile_fail/implicit_discrim_with_external_type.rs`:

```rust
use xdrs::*;

#[derive(XDR)]
#[repr(i32)]
enum Status {
    Ready = 10,
    Failed = 11,
}

#[derive(ReadXDR, WriteXDR, XDR)]
#[xdr(discrim_type = "Status")]
#[repr(i32)]
enum Message {
    Ready { value: u32 },
    Failed { code: u32 },
}

fn main() {}
```

- [ ] **Step 4: Run the test once to capture the expected stderr**

Run: `TRYBUILD=overwrite cargo test --test compile_fail`
Expected: The test case fails to compile. trybuild captures the compiler error output into `tests/compile_fail/implicit_discrim_with_external_type.stderr`.

- [ ] **Step 5: Verify the stderr contains our error message**

Read `tests/compile_fail/implicit_discrim_with_external_type.stderr` and confirm it contains:
`"derive XDR: all payload variants must have explicit discriminants when \`discrim_type\` is set"`

- [ ] **Step 6: Run the compile-fail test again to confirm it passes**

Run: `cargo test --test compile_fail`
Expected: PASS — the error output matches the captured `.stderr` file.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml tests/compile_fail.rs tests/compile_fail/implicit_discrim_with_external_type.rs tests/compile_fail/implicit_discrim_with_external_type.stderr
git commit -m "Add compile-fail test for implicit discriminants with external discrim_type"
```

---

### Task 4: Add positive regression test

**Files:**
- Modify: `tests/enums.rs`

- [ ] **Step 1: Write a round-trip test for explicit discriminants with external `discrim_type`**

Append to `tests/enums.rs`:

```rust
#[test]
fn enum_external_discrim_explicit() {
    #[derive(Debug, PartialEq, Eq, xdrs::WriteXDR, xdrs::ReadXDR, xdrs::XDR)]
    #[repr(i32)]
    enum Status {
        Ready = 10,
        Failed = 11,
    }

    #[derive(Debug, PartialEq, Eq, xdrs::WriteXDR, xdrs::ReadXDR, xdrs::XDR)]
    #[xdr(discrim_type = "Status")]
    #[repr(i32)]
    enum Message {
        Ready { value: u32 } = Status::Ready as i32,
        Failed { code: u32 } = Status::Failed as i32,
    }

    // Status::Ready = 10 = 0x0a, value = 7
    let ready = Message::Ready { value: 7 };
    TXS::check_codec(&ready, &hex!("0000000a00000007"));
    CXS::check_codec(&ready, &hex!("0a07"));

    // Status::Failed = 11 = 0x0b, code = 3
    let failed = Message::Failed { code: 3 };
    TXS::check_codec(&failed, &hex!("0000000b00000003"));
    CXS::check_codec(&failed, &hex!("0b03"));
}
```

- [ ] **Step 2: Run the test**

Run: `cargo test --test enums enum_external_discrim_explicit -- --nocapture`
Expected: PASS — explicit discriminants with external `discrim_type` round-trip correctly.

- [ ] **Step 3: Run the full test suite**

Run: `cargo test --workspace --all-features`
Expected: All tests PASS.

- [ ] **Step 4: Commit**

```bash
git add tests/enums.rs
git commit -m "Add positive regression test for explicit discriminants with external discrim_type"
```
