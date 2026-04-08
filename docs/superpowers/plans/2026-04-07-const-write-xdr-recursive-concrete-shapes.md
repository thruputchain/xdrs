# ConstWriteXDR Recursive Concrete Shapes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend `#[derive(ConstWriteXDR)]` to recursively support concrete container shapes such as `Option<T>`, `Result<T, E>`, `&[T]`, `[T; N]`, and tuples, while allowing lifetime and const generics and continuing to reject type parameters.

**Architecture:** Keep `ConstWriteXDR` as inherent-method codegen on the derived type. Replace the current flat outermost field matcher in `xdr-derive/src/generate_const.rs` with recursive helpers that walk `syn::Type` and inline const write/size logic for concrete shapes. Cover the change with red/green tests in the existing `const_writer` integration test and internal `xdr-derive` unit tests that exercise the macro directly.

**Tech Stack:** Rust 2024, `proc-macro2`, `quote`, `syn`, `darling`, existing `cargo test` integration and proc-macro unit tests

**Execution Note:** Per user instruction, execute in the current workspace and current branch. Do not create a new worktree.

---

## File Structure

- Modify: `tests/const_writer.rs`
  - Add end-to-end red/green coverage for recursive concrete shapes, including lifetime-only and const-generic derived types.
- Modify: `xdr-derive/src/generate_const.rs`
  - Replace the flat field/binding matcher with recursive size/write emitters.
  - Add internal unit tests for generics acceptance/rejection and recursive shape parsing.
- Modify: `xdr-derive/src/lib.rs`
  - Update the `ConstWriteXDR` docs to describe the new supported boundary accurately.

### Task 1: Write The Failing Tests

**Files:**
- Modify: `tests/const_writer.rs`
- Modify: `xdr-derive/src/generate_const.rs`

- [ ] **Step 1: Add a recursive concrete-shape integration test block to `tests/const_writer.rs`**

Append this block near the end of `tests/const_writer.rs`, after the existing `link_section_pattern` test:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, xdrs::ConstWriteXDR, xdrs::WriteXDR)]
struct Leaf {
    code: u32,
}

const LEAF_A: Leaf = Leaf { code: 7 };
const LEAF_B: Leaf = Leaf { code: 42 };
const LEAF_ITEMS: [Leaf; 2] = [LEAF_A, LEAF_B];
const LEAF_SLICE: &[Leaf] = &LEAF_ITEMS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, xdrs::ConstWriteXDR, xdrs::WriteXDR)]
struct RecursiveShapes<'a, const N: usize> {
    maybe: Option<Leaf>,
    nested: Option<Option<Leaf>>,
    choice: Result<Leaf, Leaf>,
    items: &'a [Leaf],
    array: [Leaf; N],
    pair: (Leaf, Leaf),
}

const RECURSIVE: RecursiveShapes<'static, 2> = RecursiveShapes {
    maybe: Some(LEAF_A),
    nested: Some(Some(LEAF_B)),
    choice: Err(LEAF_A),
    items: LEAF_SLICE,
    array: LEAF_ITEMS,
    pair: (LEAF_A, LEAF_B),
};

const RECURSIVE_TXS: [u8; RECURSIVE.const_txs_size()] = {
    let mut w = xdrs::txs::ConstWriter::<{ RECURSIVE.const_txs_size() }>::new();
    RECURSIVE.const_write_txs(&mut w);
    w.into_bytes()
};

const RECURSIVE_CXS: [u8; RECURSIVE.const_cxs_size()] = {
    let mut w = xdrs::cxs::ConstWriter::<{ RECURSIVE.const_cxs_size() }>::new();
    RECURSIVE.const_write_cxs(&mut w);
    w.into_bytes()
};

#[test]
fn derive_recursive_concrete_shapes() {
    assert_eq!(&RECURSIVE_TXS, txs_runtime_encode(&RECURSIVE).as_slice());
    assert_eq!(&RECURSIVE_CXS, cxs_runtime_encode(&RECURSIVE).as_slice());
    assert_eq!(RECURSIVE.const_txs_size(), txs_runtime_encode(&RECURSIVE).len());
    assert_eq!(RECURSIVE.const_cxs_size(), cxs_runtime_encode(&RECURSIVE).len());
}
```

- [ ] **Step 2: Add macro-unit tests to `xdr-derive/src/generate_const.rs`**

Append this test module at the bottom of `xdr-derive/src/generate_const.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::const_write_xdr;

    #[test]
    fn lifetime_only_generics_are_allowed() {
        let input: syn::DeriveInput = syn::parse_quote! {
            struct Borrowed<'a> {
                items: &'a [Leaf],
                maybe: Option<Leaf>,
            }
        };

        let toks = const_write_xdr(&input).unwrap().to_string();
        assert!(toks.contains("const_write_txs"));
        assert!(toks.contains("const_cxs_size"));
    }

    #[test]
    fn const_generics_are_allowed() {
        let input: syn::DeriveInput = syn::parse_quote! {
            struct Fixed<const N: usize> {
                bytes: [u8; N],
                pair: (u32, u32),
            }
        };

        let toks = const_write_xdr(&input).unwrap().to_string();
        assert!(toks.contains("const_write_cxs"));
        assert!(toks.contains("const_txs_size"));
    }

    #[test]
    fn type_generics_are_rejected() {
        let input: syn::DeriveInput = syn::parse_quote! {
            struct Bad<T> {
                value: Option<T>,
            }
        };

        let err = const_write_xdr(&input).unwrap_err();
        assert!(err.to_string().contains("type parameters"));
    }
}
```

- [ ] **Step 3: Run the unit tests to verify the new acceptance/rejection checks fail**

Run:

```bash
cargo test -p xdr-derive --lib
```

Expected: FAIL because `const_write_xdr(&input).unwrap()` still errors with the current blanket "does not support generic types" behavior.

- [ ] **Step 4: Run the integration test target to verify the recursive shape support fails before implementation**

Run:

```bash
cargo test --test const_writer --features alloc,derive
```

Expected: FAIL at compile time because the derive still rejects generics and/or falls through to missing `const_write_*` methods on wrapper types such as `Option<Leaf>` and slices.

- [ ] **Step 5: Commit the red tests**

```bash
git add tests/const_writer.rs xdr-derive/src/generate_const.rs
git commit -m "test: cover recursive concrete ConstWriteXDR shapes"
```

### Task 2: Implement Recursive Concrete-Shape Expansion

**Files:**
- Modify: `xdr-derive/src/generate_const.rs`

- [ ] **Step 1: Replace the blanket generic rejection with an explicit supported-generics check**

Near the top of `xdr-derive/src/generate_const.rs`, replace the current blanket `input.generics.params.is_empty()` gate with:

```rust
fn check_supported_generics(input: &syn::DeriveInput) -> Result<()> {
    for param in &input.generics.params {
        match param {
            syn::GenericParam::Lifetime(_) | syn::GenericParam::Const(_) => {}
            syn::GenericParam::Type(param) => {
                return err(
                    param,
                    "ConstWriteXDR does not support type parameters in encoded shapes.",
                );
            }
        }
    }
    Ok(())
}

fn const_write_data(input: &syn::DeriveInput) -> Result<TokenStream> {
    check_supported_generics(input)?;

    match &input.data {
        syn::Data::Struct(item) => const_write_struct(input, item),
        syn::Data::Enum(item) => const_write_enum(input, item),
        _ => err(
            input,
            "ConstWriteXDR can only be derived for structs and enums",
        ),
    }
}
```

- [ ] **Step 2: Add recursive type-shape helpers and validation**

Add helper functions above the struct/enum codegen to classify the supported shapes and reject unsupported ones early:

```rust
fn option_inner(ty: &syn::Type) -> Option<&syn::Type> {
    let syn::Type::Path(path) = ty else { return None };
    let seg = path.path.segments.last()?;
    if seg.ident != "Option" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &seg.arguments else {
        return None;
    };
    match args.args.first()? {
        syn::GenericArgument::Type(inner) => Some(inner),
        _ => None,
    }
}

fn result_inners(ty: &syn::Type) -> Option<(&syn::Type, &syn::Type)> {
    let syn::Type::Path(path) = ty else { return None };
    let seg = path.path.segments.last()?;
    if seg.ident != "Result" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &seg.arguments else {
        return None;
    };
    let mut tys = args.args.iter().filter_map(|arg| {
        if let syn::GenericArgument::Type(inner) = arg {
            Some(inner)
        } else {
            None
        }
    });
    Some((tys.next()?, tys.next()?))
}

fn slice_ref_inner(ty: &syn::Type) -> Option<&syn::Type> {
    let syn::Type::Reference(r) = ty else { return None };
    let syn::Type::Slice(slice) = &*r.elem else { return None };
    Some(&slice.elem)
}

fn array_inner(ty: &syn::Type) -> Option<(&syn::Type, &syn::Expr)> {
    let syn::Type::Array(arr) = ty else { return None };
    Some((&arr.elem, &arr.len))
}

fn tuple_elems(ty: &syn::Type) -> Option<impl Iterator<Item = &syn::Type>> {
    let syn::Type::Tuple(tuple) = ty else { return None };
    Some(tuple.elems.iter())
}

fn contains_type_param(ty: &syn::Type) -> bool {
    match ty {
        syn::Type::Path(path) => {
            if path.qself.is_none()
                && path.path.segments.len() == 1
                && matches!(
                    path.path.segments[0].arguments,
                    syn::PathArguments::None
                )
            {
                return true;
            }

            path.path.segments.iter().any(|seg| match &seg.arguments {
                syn::PathArguments::None => false,
                syn::PathArguments::AngleBracketed(args) => args.args.iter().any(|arg| {
                    matches!(arg, syn::GenericArgument::Type(inner) if contains_type_param(inner))
                }),
                syn::PathArguments::Parenthesized(args) => {
                    args.inputs.iter().any(contains_type_param)
                        || args.output.to_token_stream().to_string() != ""
                }
            })
        }
        syn::Type::Reference(r) => contains_type_param(&r.elem),
        syn::Type::Slice(s) => contains_type_param(&s.elem),
        syn::Type::Array(a) => contains_type_param(&a.elem),
        syn::Type::Tuple(t) => t.elems.iter().any(contains_type_param),
        _ => false,
    }
}

fn check_supported_const_shape(ty: &syn::Type) -> Result<()> {
    if contains_type_param(ty) {
        return err(
            ty,
            "ConstWriteXDR does not support type parameters in encoded shapes.",
        );
    }

    if primitive_size_fn(ty).is_some()
        || is_i128(ty)
        || is_u128(ty)
        || is_u8_array(ty)
        || is_u8_slice_ref(ty)
        || is_str_ref(ty)
    {
        return Ok(());
    }

    if let Some(inner) = option_inner(ty) {
        return check_supported_const_shape(inner);
    }

    if let Some((ok, err_ty)) = result_inners(ty) {
        check_supported_const_shape(ok)?;
        return check_supported_const_shape(err_ty);
    }

    if let Some(inner) = slice_ref_inner(ty) {
        return check_supported_const_shape(inner);
    }

    if let Some((inner, _)) = array_inner(ty) {
        return check_supported_const_shape(inner);
    }

    if let Some(elems) = tuple_elems(ty) {
        for elem in elems {
            check_supported_const_shape(elem)?;
        }
        return Ok(());
    }

    if matches!(ty, syn::Type::Path(_)) {
        return Ok(());
    }

    err(ty, "ConstWriteXDR does not support this type form.")
}
```

- [ ] **Step 3: Add recursive size/write emitters that inline the concrete shape**

Replace the current flat `const_*_field_expr` and `const_*_binding_expr` logic with two recursive emitters that accept a reference expression:

```rust
fn const_size_value_expr(expr_ref: TokenStream, ty: &syn::Type, fmt: Format) -> Result<TokenStream> {
    check_supported_const_shape(ty)?;

    let mod_path = match fmt {
        Format::Cxs => quote! { cxs::const_writer },
        Format::Txs => quote! { txs::const_writer },
    };
    let size_method = match fmt {
        Format::Cxs => quote! { const_cxs_size },
        Format::Txs => quote! { const_txs_size },
    };

    if let Some(size_fn) = primitive_size_fn(ty) {
        return Ok(quote! { #mod_path::#size_fn(*#expr_ref) });
    }
    if is_i128(ty) {
        return Ok(quote! {
            #mod_path::size_i64((*#expr_ref >> 64) as i64)
                + #mod_path::size_u64((*#expr_ref & 0xffffffffffffffff) as u64)
        });
    }
    if is_u128(ty) {
        return Ok(quote! {
            #mod_path::size_u64((*#expr_ref >> 64) as u64)
                + #mod_path::size_u64((*#expr_ref & 0xffffffffffffffff) as u64)
        });
    }
    if is_u8_array(ty) {
        return Ok(quote! { #mod_path::size_opaque_fixed((#expr_ref).len()) });
    }
    if is_u8_slice_ref(ty) {
        return Ok(quote! { #mod_path::size_opaque_variable((*#expr_ref).len()) });
    }
    if is_str_ref(ty) {
        return Ok(quote! { #mod_path::size_opaque_variable((*#expr_ref).as_bytes().len()) });
    }
    if let Some(inner) = option_inner(ty) {
        let inner_size = const_size_value_expr(quote! { inner }, inner, fmt)?;
        return Ok(quote! {
            match #expr_ref {
                Some(inner) => #mod_path::size_i32(1) + #inner_size,
                None => #mod_path::size_i32(0),
            }
        });
    }
    if let Some((ok, err_ty)) = result_inners(ty) {
        let ok_size = const_size_value_expr(quote! { inner }, ok, fmt)?;
        let err_size = const_size_value_expr(quote! { inner }, err_ty, fmt)?;
        return Ok(quote! {
            match #expr_ref {
                Ok(inner) => #mod_path::size_i32(0) + #ok_size,
                Err(inner) => #mod_path::size_i32(1) + #err_size,
            }
        });
    }
    if let Some(inner) = slice_ref_inner(ty) {
        let inner_size = const_size_value_expr(quote! { &slice[i] }, inner, fmt)?;
        return Ok(quote! {
            {
                let slice = *#expr_ref;
                let mut total = #mod_path::size_u32(slice.len() as u32);
                let mut i = 0;
                while i < slice.len() {
                    total += #inner_size;
                    i += 1;
                }
                total
            }
        });
    }
    if let Some((inner, _)) = array_inner(ty) {
        let inner_size = const_size_value_expr(quote! { &arr[i] }, inner, fmt)?;
        return Ok(quote! {
            {
                let arr = #expr_ref;
                let mut total = 0;
                let mut i = 0;
                while i < arr.len() {
                    total += #inner_size;
                    i += 1;
                }
                total
            }
        });
    }
    if let Some(elems) = tuple_elems(ty) {
        let elem_sizes = elems.enumerate().map(|(i, elem)| {
            const_size_value_expr(quote! { &#expr_ref.#i }, elem, fmt)
        }).collect::<Result<Vec<_>>>()?;
        return Ok(quote! { 0 #(+ #elem_sizes)* });
    }

    Ok(quote! { (#expr_ref).#size_method() })
}

fn const_write_value_expr(expr_ref: TokenStream, ty: &syn::Type, fmt: Format) -> Result<TokenStream> {
    check_supported_const_shape(ty)?;

    let write_method = match fmt {
        Format::Cxs => quote! { const_write_cxs },
        Format::Txs => quote! { const_write_txs },
    };

    if let Some(writer_method) = primitive_write_method(ty) {
        return Ok(quote! { w.#writer_method(*#expr_ref); });
    }
    if is_i128(ty) {
        return Ok(quote! {
            w.write_i64((*#expr_ref >> 64) as i64);
            w.write_u64((*#expr_ref & 0xffffffffffffffff) as u64);
        });
    }
    if is_u128(ty) {
        return Ok(quote! {
            w.write_u64((*#expr_ref >> 64) as u64);
            w.write_u64((*#expr_ref & 0xffffffffffffffff) as u64);
        });
    }
    if is_u8_array(ty) {
        return Ok(quote! { w.write_opaque_fixed(*#expr_ref); });
    }
    if is_u8_slice_ref(ty) {
        return Ok(quote! { w.write_opaque_variable(*#expr_ref); });
    }
    if is_str_ref(ty) {
        return Ok(quote! { w.write_opaque_variable((*#expr_ref).as_bytes()); });
    }
    if let Some(inner) = option_inner(ty) {
        let inner_write = const_write_value_expr(quote! { inner }, inner, fmt)?;
        return Ok(quote! {
            match #expr_ref {
                Some(inner) => {
                    w.write_i32(1);
                    #inner_write
                }
                None => w.write_i32(0),
            }
        });
    }
    if let Some((ok, err_ty)) = result_inners(ty) {
        let ok_write = const_write_value_expr(quote! { inner }, ok, fmt)?;
        let err_write = const_write_value_expr(quote! { inner }, err_ty, fmt)?;
        return Ok(quote! {
            match #expr_ref {
                Ok(inner) => {
                    w.write_i32(0);
                    #ok_write
                }
                Err(inner) => {
                    w.write_i32(1);
                    #err_write
                }
            }
        });
    }
    if let Some(inner) = slice_ref_inner(ty) {
        let inner_write = const_write_value_expr(quote! { &slice[i] }, inner, fmt)?;
        return Ok(quote! {
            {
                let slice = *#expr_ref;
                w.write_u32(slice.len() as u32);
                let mut i = 0;
                while i < slice.len() {
                    #inner_write
                    i += 1;
                }
            }
        });
    }
    if let Some((inner, _)) = array_inner(ty) {
        let inner_write = const_write_value_expr(quote! { &arr[i] }, inner, fmt)?;
        return Ok(quote! {
            {
                let arr = #expr_ref;
                let mut i = 0;
                while i < arr.len() {
                    #inner_write
                    i += 1;
                }
            }
        });
    }
    if let Some(elems) = tuple_elems(ty) {
        let elem_writes = elems.enumerate().map(|(i, elem)| {
            const_write_value_expr(quote! { &#expr_ref.#i }, elem, fmt)
        }).collect::<Result<Vec<_>>>()?;
        return Ok(quote! { #(#elem_writes)* });
    }

    Ok(quote! { (#expr_ref).#write_method(w); })
}
```

- [ ] **Step 4: Rewire the field and enum binding entry points to the recursive helpers**

Update the field and binding helpers to delegate into the new emitters:

```rust
fn const_size_field(index: usize, field: &syn::Field, fmt: Format) -> Result<TokenStream> {
    let attrs = Attrs::from_field(field)?;
    attrs.check_no_conversion(field)?;

    let fa = FieldAccess::new(index, field);
    const_size_value_expr(quote! { &self.#fa }, &field.ty, fmt)
}

fn const_write_field(index: usize, field: &syn::Field, fmt: Format) -> Result<TokenStream> {
    let attrs = Attrs::from_field(field)?;
    attrs.check_no_conversion(field)?;

    let fa = FieldAccess::new(index, field);
    const_write_value_expr(quote! { &self.#fa }, &field.ty, fmt)
}

fn const_size_binding_expr(bind: &syn::Ident, ty: &syn::Type, fmt: Format) -> Result<TokenStream> {
    const_size_value_expr(quote! { #bind }, ty, fmt)
}

fn const_write_binding_expr(bind: &syn::Ident, ty: &syn::Type, fmt: Format) -> Result<TokenStream> {
    const_write_value_expr(quote! { #bind }, ty, fmt)
}
```

Then update the enum call sites so the `.map(...)` closures collect `Result<Vec<_>>>()?` instead of raw `Vec<_>`.

- [ ] **Step 5: Run the tests to verify the implementation turns green**

Run:

```bash
cargo test -p xdr-derive --lib
cargo test --test const_writer --features alloc,derive
```

Expected: PASS.

- [ ] **Step 6: Commit the macro implementation**

```bash
git add xdr-derive/src/generate_const.rs tests/const_writer.rs
git commit -m "feat: add recursive concrete-shape support to ConstWriteXDR"
```

### Task 3: Update Docs And Run Broad Verification

**Files:**
- Modify: `xdr-derive/src/lib.rs`

- [ ] **Step 1: Update the `ConstWriteXDR` derive docs in `xdr-derive/src/lib.rs`**

Replace the current limitations block with:

```rust
/// # Supported Shapes
///
/// Supports recursive const encoding for concrete shapes, including:
/// - primitives
/// - `i128` / `u128`
/// - `&str`
/// - `&[u8]`
/// - `[u8; N]`
/// - `Option<T>`
/// - `Result<T, E>`
/// - `&[T]`
/// - `[T; N]`
/// - tuples
/// - other concrete types that also derive `ConstWriteXDR`
///
/// # Limitations
///
/// - Does not support `#[xdrs(into_type)]` or `#[xdrs(via)]` attributes
/// - Does not support type parameters
/// - Does support lifetime parameters and const generics
/// - Unsupported type forms produce a derive-time error
```

- [ ] **Step 2: Run the broader verification pass**

Run:

```bash
cargo test --features alloc,derive
```

Expected: PASS.

- [ ] **Step 3: Commit the docs update and verification pass**

```bash
git add xdr-derive/src/lib.rs
git commit -m "docs: describe recursive ConstWriteXDR support"
```
