# ConstWriteXDR plain `&T` reference support

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Teach the `ConstWriteXDR` derive macro to recurse through plain `&T` references, unblocking sdk_rs contract-spec view types.

**Architecture:** Three symmetric insertions in `xdr-derive/src/generate_const.rs` — one per recursive emitter — that catch `syn::Type::Reference` after the existing specific-reference handlers and dereference into the referent. Plus a test file proving the new path works and matches runtime output.

**Tech Stack:** Rust, syn, quote, proc-macro2

**Spec:** `docs/superpowers/specs/2026-04-08-const-write-xdr-plain-ref-design.md`

---

### Task 1: Write failing test for plain `&T` in a struct

**Files:**
- Create: `tests/plain_ref.rs`

This test will fail to compile until we add `&T` support to the derive macro.

- [ ] **Step 1: Create the test file**

```rust
// tests/plain_ref.rs
mod common;
use common::*;

fn cxs_runtime_encode<T: xdrs::WriteXDR>(val: &T) -> Vec<u8> {
    xdrs::cxs::encode(val).unwrap()
}

fn txs_runtime_encode<T: xdrs::WriteXDR>(val: &T) -> Vec<u8> {
    xdrs::txs::encode(val).unwrap()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, xdrs::ConstWriteXDR, xdrs::WriteXDR)]
struct Leaf {
    code: u32,
}

const LEAF_A: Leaf = Leaf { code: 7 };
const LEAF_B: Leaf = Leaf { code: 42 };

// --- Plain &T in a struct ---

#[derive(Debug, PartialEq, Eq, xdrs::ConstWriteXDR, xdrs::WriteXDR)]
struct BorrowedLeaf<'a> {
    tag: u32,
    inner: &'a Leaf,
}

const BORROWED_LEAF: BorrowedLeaf<'static> = BorrowedLeaf {
    tag: 1,
    inner: &LEAF_A,
};

const BORROWED_LEAF_CXS: [u8; BORROWED_LEAF.const_cxs_size()] = {
    let mut w = xdrs::cxs::ConstWriter::<{ BORROWED_LEAF.const_cxs_size() }>::new();
    BORROWED_LEAF.const_write_cxs(&mut w);
    w.into_bytes()
};

const BORROWED_LEAF_TXS: [u8; BORROWED_LEAF.const_txs_size()] = {
    let mut w = xdrs::txs::ConstWriter::<{ BORROWED_LEAF.const_txs_size() }>::new();
    BORROWED_LEAF.const_write_txs(&mut w);
    w.into_bytes()
};

#[test]
fn struct_with_plain_ref_const_matches_runtime() {
    assert_eq!(
        &BORROWED_LEAF_CXS,
        cxs_runtime_encode(&BORROWED_LEAF).as_slice()
    );
    assert_eq!(
        &BORROWED_LEAF_TXS,
        txs_runtime_encode(&BORROWED_LEAF).as_slice()
    );
}

// --- Enum with &T variant fields (mirrors sdk_rs ContractTypeView) ---

#[derive(Debug, PartialEq, Eq, xdrs::ConstWriteXDR, xdrs::WriteXDR)]
#[repr(i32)]
enum TypeRef<'a> {
    None = 0,
    Single(&'a Leaf) = 1,
    Pair(&'a Leaf, &'a Leaf) = 2,
}

const TYPE_REF_SINGLE: TypeRef<'static> = TypeRef::Single(&LEAF_A);
const TYPE_REF_PAIR: TypeRef<'static> = TypeRef::Pair(&LEAF_A, &LEAF_B);

const TYPE_REF_SINGLE_CXS: [u8; TYPE_REF_SINGLE.const_cxs_size()] = {
    let mut w = xdrs::cxs::ConstWriter::<{ TYPE_REF_SINGLE.const_cxs_size() }>::new();
    TYPE_REF_SINGLE.const_write_cxs(&mut w);
    w.into_bytes()
};

const TYPE_REF_SINGLE_TXS: [u8; TYPE_REF_SINGLE.const_txs_size()] = {
    let mut w = xdrs::txs::ConstWriter::<{ TYPE_REF_SINGLE.const_txs_size() }>::new();
    TYPE_REF_SINGLE.const_write_txs(&mut w);
    w.into_bytes()
};

const TYPE_REF_PAIR_CXS: [u8; TYPE_REF_PAIR.const_cxs_size()] = {
    let mut w = xdrs::cxs::ConstWriter::<{ TYPE_REF_PAIR.const_cxs_size() }>::new();
    TYPE_REF_PAIR.const_write_cxs(&mut w);
    w.into_bytes()
};

const TYPE_REF_PAIR_TXS: [u8; TYPE_REF_PAIR.const_txs_size()] = {
    let mut w = xdrs::txs::ConstWriter::<{ TYPE_REF_PAIR.const_txs_size() }>::new();
    TYPE_REF_PAIR.const_write_txs(&mut w);
    w.into_bytes()
};

#[test]
fn enum_with_ref_variants_const_matches_runtime() {
    assert_eq!(
        &TYPE_REF_SINGLE_CXS,
        cxs_runtime_encode(&TYPE_REF_SINGLE).as_slice()
    );
    assert_eq!(
        &TYPE_REF_SINGLE_TXS,
        txs_runtime_encode(&TYPE_REF_SINGLE).as_slice()
    );
    assert_eq!(
        &TYPE_REF_PAIR_CXS,
        cxs_runtime_encode(&TYPE_REF_PAIR).as_slice()
    );
    assert_eq!(
        &TYPE_REF_PAIR_TXS,
        txs_runtime_encode(&TYPE_REF_PAIR).as_slice()
    );
}

// --- Option<&T> (the sdk_rs pattern) ---

#[derive(Debug, PartialEq, Eq, xdrs::ConstWriteXDR, xdrs::WriteXDR)]
struct OptionRef<'a> {
    maybe: Option<&'a Leaf>,
}

const OPT_REF_SOME: OptionRef<'static> = OptionRef {
    maybe: Some(&LEAF_A),
};
const OPT_REF_NONE: OptionRef<'static> = OptionRef { maybe: None };

const OPT_REF_SOME_CXS: [u8; OPT_REF_SOME.const_cxs_size()] = {
    let mut w = xdrs::cxs::ConstWriter::<{ OPT_REF_SOME.const_cxs_size() }>::new();
    OPT_REF_SOME.const_write_cxs(&mut w);
    w.into_bytes()
};

const OPT_REF_SOME_TXS: [u8; OPT_REF_SOME.const_txs_size()] = {
    let mut w = xdrs::txs::ConstWriter::<{ OPT_REF_SOME.const_txs_size() }>::new();
    OPT_REF_SOME.const_write_txs(&mut w);
    w.into_bytes()
};

const OPT_REF_NONE_CXS: [u8; OPT_REF_NONE.const_cxs_size()] = {
    let mut w = xdrs::cxs::ConstWriter::<{ OPT_REF_NONE.const_cxs_size() }>::new();
    OPT_REF_NONE.const_write_cxs(&mut w);
    w.into_bytes()
};

const OPT_REF_NONE_TXS: [u8; OPT_REF_NONE.const_txs_size()] = {
    let mut w = xdrs::txs::ConstWriter::<{ OPT_REF_NONE.const_txs_size() }>::new();
    OPT_REF_NONE.const_write_txs(&mut w);
    w.into_bytes()
};

#[test]
fn option_of_ref_const_matches_runtime() {
    assert_eq!(
        &OPT_REF_SOME_CXS,
        cxs_runtime_encode(&OPT_REF_SOME).as_slice()
    );
    assert_eq!(
        &OPT_REF_SOME_TXS,
        txs_runtime_encode(&OPT_REF_SOME).as_slice()
    );
    assert_eq!(
        &OPT_REF_NONE_CXS,
        cxs_runtime_encode(&OPT_REF_NONE).as_slice()
    );
    assert_eq!(
        &OPT_REF_NONE_TXS,
        txs_runtime_encode(&OPT_REF_NONE).as_slice()
    );
}
```

- [ ] **Step 2: Verify the test fails to compile**

Run: `cargo test --test plain_ref 2>&1 | head -20`

Expected: compile error — `ConstWriteXDR does not support this type form` on the `&'a Leaf` fields.

- [ ] **Step 3: Commit the failing test**

```bash
git add tests/plain_ref.rs
git commit -m "test: add failing tests for ConstWriteXDR plain &T support"
```

---

### Task 2: Add plain `&T` support to the three emitters

**Files:**
- Modify: `xdr-derive/src/generate_const.rs:729`, `:247`, `:367`

Three symmetric insertions, each placed after the `tuple_elems` handler and before the fallthrough.

- [ ] **Step 1: Add `&T` branch to `check_supported_const_shape`**

In `xdr-derive/src/generate_const.rs`, insert after line 729 (`return Ok(());` closing the `tuple_elems` block) and before line 731 (`if matches!(ty, syn::Type::Path(_))`):

```rust
    if let syn::Type::Reference(r) = ty {
        return check_supported_const_shape(&r.elem);
    }
```

- [ ] **Step 2: Add `&T` branch to `const_size_value_expr`**

Insert after line 247 (`return Ok(quote! { 0 #(+ #elem_sizes)* });` closing the `tuple_elems` block) and before line 249 (`Ok(quote! { (#expr_ref).#size_method() })`):

```rust
    if let syn::Type::Reference(r) = ty {
        return const_size_value_expr(quote! { *#expr_ref }, &r.elem, fmt);
    }
```

- [ ] **Step 3: Add `&T` branch to `const_write_value_expr`**

Insert after line 367 (`return Ok(quote! { #(#elem_writes)* });` closing the `tuple_elems` block) and before line 369 (`Ok(quote! { (#expr_ref).#method(w); })`):

```rust
    if let syn::Type::Reference(r) = ty {
        return const_write_value_expr(quote! { *#expr_ref }, &r.elem, fmt);
    }
```

- [ ] **Step 4: Run the new tests**

Run: `cargo test --test plain_ref 2>&1`

Expected: all three tests pass — `struct_with_plain_ref_const_matches_runtime`, `enum_with_ref_variants_const_matches_runtime`, `option_of_ref_const_matches_runtime`.

- [ ] **Step 5: Run the full test suite to confirm no regressions**

Run: `cargo test 2>&1`

Expected: all existing tests still pass.

- [ ] **Step 6: Commit**

```bash
git add xdr-derive/src/generate_const.rs tests/plain_ref.rs
git commit -m "feat: teach ConstWriteXDR to recurse through plain &T references"
```

---

### Task 3: Bump version to 0.5.6

**Files:**
- Modify: `Cargo.toml:12`

- [ ] **Step 1: Bump workspace version**

In `Cargo.toml`, change line 12:

```toml
version = "0.5.6"
```

- [ ] **Step 2: Commit**

```bash
git add Cargo.toml
git commit -m "bump to 0.5.6"
```
