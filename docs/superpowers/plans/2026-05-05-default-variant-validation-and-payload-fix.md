# Default Variant Validation and Payload Round-Trip Fix

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix `#[derive(ReadXDR, WriteXDR)]` so that default enum variants with payload fields round-trip correctly, and reject fieldless default variants at compile time.

**Architecture:** Three changes in `xdr-derive`: (1) add shape validation in `check_variants` to reject fieldless defaults and require the first field to be `i32`, (2) fix writer/reader codegen to serialize/deserialize payload fields beyond field 0, (3) fix schema generation to emit actual payload declarations instead of unconditional `void`.

**Tech Stack:** Rust proc-macro (`xdr-derive` crate), `syn`, `quote`, `proc_macro2`

---

### Task 1: Write Failing Round-Trip Test for Default-With-Payload

**Files:**
- Modify: `tests/enums.rs:86` (append new test after `enum_default_variant_encode`)

- [ ] **Step 1: Write the failing test**

Add this test to `tests/enums.rs` after line 86:

```rust
#[test]
fn enum_default_variant_with_payload_roundtrip() {
    #[derive(Debug, PartialEq, Eq, xdrs::WriteXDR, xdrs::ReadXDR)]
    #[repr(i32)]
    enum UnnamedPayload {
        Known = 0,
        Default(i32, u32),
    }

    let original = UnnamedPayload::Default(7, 9);
    TXS::check_codec(&original, &hex!("0000000700000009"));

    let cxs_encoded = xdrs::cxs::encode(&original).unwrap();
    let cxs_decoded: UnnamedPayload = xdrs::cxs::decode(&cxs_encoded).unwrap();
    assert_eq!(cxs_decoded, original);

    #[derive(Debug, PartialEq, Eq, xdrs::WriteXDR, xdrs::ReadXDR)]
    #[repr(i32)]
    enum NamedPayload {
        Known { val: u32 } = 0,
        Default { code: i32, data: u32 },
    }

    let original = NamedPayload::Default { code: 7, data: 9 };
    TXS::check_codec(&original, &hex!("0000000700000009"));

    let cxs_encoded = xdrs::cxs::encode(&original).unwrap();
    let cxs_decoded: NamedPayload = xdrs::cxs::decode(&cxs_encoded).unwrap();
    assert_eq!(cxs_decoded, original);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --workspace --all-features --test enums enum_default_variant_with_payload_roundtrip -- --nocapture`

Expected: FAIL — the TXS encode will produce only 4 bytes (just the discriminant) instead of the expected 8 bytes, because `write_variant` skips payload fields for default arms.

- [ ] **Step 3: Commit the failing test**

```bash
git add tests/enums.rs
git commit -m "test: add failing round-trip test for default variant with payload (#54)"
```

---

### Task 2: Fix Writer Codegen to Emit Payload Fields for Default Arms

**Files:**
- Modify: `xdr-derive/src/generate.rs:210-215`

- [ ] **Step 1: Fix `write_variant` to write payload fields 1..N**

In `xdr-derive/src/generate.rs`, replace lines 210-215:

```rust
    let write_bindings = if is_default_arm {
        // Write only the discriminant
        quote! {}
    } else {
        token_reduce(&v.fields, write_binding)
    };
```

with:

```rust
    let write_bindings = if is_default_arm {
        // Field 0 is the discriminant (already written above); write payload fields 1..N
        token_reduce(&v.fields, |i, field| {
            if i == 0 {
                quote! {}
            } else {
                write_binding(i, field)
            }
        })
    } else {
        token_reduce(&v.fields, write_binding)
    };
```

- [ ] **Step 2: Run test to verify it still fails (reader not yet fixed)**

Run: `cargo test --workspace --all-features --test enums enum_default_variant_with_payload_roundtrip -- --nocapture`

Expected: FAIL — encoding now produces the correct 8 bytes, but decoding still fills all fields with `d` so the round-trip assertion fails.

---

### Task 3: Fix Reader Codegen to Read and Init Payload Fields for Default Arms

**Files:**
- Modify: `xdr-derive/src/generate.rs:438-446` (`read_variant`)
- Modify: `xdr-derive/src/generate.rs:463-465` (`init_fields`)

- [ ] **Step 1: Fix `read_variant` to read payload fields 1..N**

In `xdr-derive/src/generate.rs`, replace lines 438-446:

```rust
    let read_fields = token_reduce(&variant.fields, |i, field| {
        if is_default {
            return quote! {};
        }

        let bind = tmp_binding(i);
        let ty = &field.ty;
        quote! { let #bind: #ty = __xdrs::read_xdr(r)?; }
    });
```

with:

```rust
    let read_fields = token_reduce(&variant.fields, |i, field| {
        if is_default && i == 0 {
            // Field 0 is the discriminant; initialized from `d` in init_fields
            return quote! {};
        }

        let bind = tmp_binding(i);
        let ty = &field.ty;
        quote! { let #bind: #ty = __xdrs::read_xdr(r)?; }
    });
```

- [ ] **Step 2: Fix `init_fields` so only field 0 gets `d`**

In `xdr-derive/src/generate.rs`, replace lines 463-465:

```rust
        let init_val = if is_default {
            // The value is the discriminant
            quote! { d }
```

with:

```rust
        let init_val = if is_default && i == 0 {
            // Field 0 is the discriminant
            quote! { d }
```

- [ ] **Step 3: Run test to verify it passes**

Run: `cargo test --workspace --all-features --test enums enum_default_variant_with_payload_roundtrip -- --nocapture`

Expected: PASS

- [ ] **Step 4: Run existing enum tests to check for regressions**

Run: `cargo test --workspace --all-features --test enums -- --nocapture`

Expected: All tests pass, including the existing `enum_foo` (which tests `Default(3)`) and `enum_default_variant_encode` (which tests `Default { d: discriminant }`).

- [ ] **Step 5: Commit**

```bash
git add xdr-derive/src/generate.rs
git commit -m "fix: serialize/deserialize all payload fields on default enum arms (#54)"
```

---

### Task 4: Add Validation for Default Variant Shape in `check_variants`

**Files:**
- Modify: `xdr-derive/src/util.rs:120-153` (`check_variants`)

- [ ] **Step 1: Add a helper to check for `i32` type**

In `xdr-derive/src/util.rs`, add this function before `check_variants` (e.g. after `find_default_variant` at line 118):

```rust
fn first_field_is_i32(variant: &syn::Variant) -> bool {
    variant
        .fields
        .iter()
        .next()
        .map(|f| matches!(&f.ty, syn::Type::Path(p) if p.path.is_ident("i32")))
        .unwrap_or(false)
}
```

- [ ] **Step 2: Add validation after the trailing-position check**

In `xdr-derive/src/util.rs`, replace lines 137-149:

```rust
    } else if let Some(dv) = default_variant {
        // RFC 4506 requires the default arm of a union to be trailing.
        // A non-trailing Default would generate a wildcard match arm that
        // shadows later concrete variants.
        if !variants.iter().next_back().is_some_and(variant_is_default) {
            err(
                &dv.ident,
                "derive XDR: Default variant must be the last variant \
                 (RFC 4506 requires the default arm of a union to be trailing)",
            )
        } else {
            Ok(())
        }
```

with:

```rust
    } else if let Some(dv) = default_variant {
        // RFC 4506 requires the default arm of a union to be trailing.
        // A non-trailing Default would generate a wildcard match arm that
        // shadows later concrete variants.
        if !variants.iter().next_back().is_some_and(variant_is_default) {
            err(
                &dv.ident,
                "derive XDR: Default variant must be the last variant \
                 (RFC 4506 requires the default arm of a union to be trailing)",
            )
        } else if dv.fields.is_empty() {
            err(
                &dv.ident,
                "derive XDR: Default variant must have at least one i32 field \
                 to hold the unmatched discriminant value",
            )
        } else if !first_field_is_i32(dv) {
            err(
                &dv.ident,
                "derive XDR: Default variant's first field must be i32 \
                 (it holds the unmatched discriminant value)",
            )
        } else {
            Ok(())
        }
```

- [ ] **Step 3: Update the error message for mixed discriminants to be accurate**

In `xdr-derive/src/util.rs`, replace lines 132-136:

```rust
            "derive XDR: enum discriminants must be all implicit or \
             all explicit, and may optionally also include a default \
             variant (with no explicit discriminant) named \"Default\", \
             which must have one i32 field",
```

with:

```rust
            "derive XDR: enum discriminants must be all implicit or \
             all explicit, and may optionally also include a default \
             variant (with no explicit discriminant) named \"Default\", \
             whose first field must be i32",
```

- [ ] **Step 4: Run all tests to verify no regressions**

Run: `cargo test --workspace --all-features -- --nocapture`

Expected: All tests pass. Existing tests use valid default variant shapes (single `i32` field), so the new validation does not reject them.

- [ ] **Step 5: Commit**

```bash
git add xdr-derive/src/util.rs
git commit -m "fix: reject fieldless default variants and require first field to be i32 (#54)"
```

---

### Task 5: Fix Schema Generation for Default Variant Payload

**Files:**
- Modify: `xdr-derive/src/generate_type.rs:285-289` (`union_typedef`)
- Modify: `xdr-derive/src/generate_type.rs` (add `default_variant_payload_decl` function)
- Modify: `tests/repr/derive_enums.rs` (add schema test for default-with-payload)

- [ ] **Step 1: Write failing schema test**

In `tests/repr/derive_enums.rs`, add a new module after `default_only` (after line 241):

```rust
pub mod default_with_payload {
    use super::*;

    #[derive(XDR)]
    #[repr(i32)]
    pub enum DefaultPayload {
        Alpha { a: u32 } = 10,
        Beta = 20,
        Default { code: i32, data: u32 },
    }

    check_format! { derive_default_payload
        DefaultPayload::spec()
        =>
            "enum DefaultPayloadType {"
            "	Alpha = 10,"
            "	Beta = 20,"
            "};"
            ""
            "union DefaultPayload switch (DefaultPayloadType which)"
            "{"
            "case Alpha:"
            "	unsigned int a;"
            "case Beta:"
            "	void;"
            "default:"
            "	unsigned int data;"
            "};"
            ""
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --workspace --all-features --test repr derive_default_payload -- --nocapture`

Expected: FAIL — the schema currently emits `void` for the default arm instead of `unsigned int data`.

- [ ] **Step 3: Add `default_variant_payload_decl` function**

In `xdr-derive/src/generate_type.rs`, add this function after `variant_fields_decl` (after line 359):

```rust
fn default_variant_payload_decl(variant: &syn::Variant) -> TokenStream {
    match &variant.fields {
        syn::Fields::Named(fields_named) => {
            let payload_fields: Vec<_> = fields_named.named.iter().skip(1).collect();
            if payload_fields.is_empty() {
                return quote! { __xdrs::declare_void() };
            }
            let variant_name = variant.ident.to_string().to_lowercase();
            let decls: Vec<_> = payload_fields
                .iter()
                .map(|field| {
                    let field_ty = &field.ty;
                    let field_name = field
                        .ident
                        .as_ref()
                        .map(|i| i.to_string())
                        .unwrap_or("inner".to_string());
                    quote! { <#field_ty as __xdrs::Declare>::declare(__xdrs::ident(#field_name)) }
                })
                .collect();
            if decls.len() == 1 {
                decls.into_iter().next().unwrap()
            } else {
                quote! {
                    __xdrs::declare_type(#variant_name,
                        __xdrs::struct_type(__xdrs::vec![ #(#decls),* ]))
                }
            }
        }
        syn::Fields::Unnamed(_) | syn::Fields::Unit => {
            quote! { __xdrs::declare_void() }
        }
    }
}
```

- [ ] **Step 4: Wire up `default_variant_payload_decl` in `union_typedef`**

In `xdr-derive/src/generate_type.rs`, replace lines 285-289:

```rust
    let default_decl = option_tok(default_variant.map(|_v| {
        quote! {
            __xdrs::declare_void()
        }
    }));
```

with:

```rust
    let default_decl = option_tok(default_variant.map(default_variant_payload_decl));
```

- [ ] **Step 5: Run schema test to verify it passes**

Run: `cargo test --workspace --all-features --test repr derive_default_payload -- --nocapture`

Expected: PASS

- [ ] **Step 6: Run the full test suite**

Run: `RUSTFLAGS="--deny warnings" cargo test --workspace --all-features`

Expected: All tests pass with no warnings.

- [ ] **Step 7: Commit**

```bash
git add xdr-derive/src/generate_type.rs tests/repr/derive_enums.rs
git commit -m "fix: emit actual payload declarations for default variant in XDR schema (#54)"
```
