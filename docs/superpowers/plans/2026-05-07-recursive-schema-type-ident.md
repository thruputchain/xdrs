# Recursive Schema Type Identity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Support finite schema generation for recursive derived named XDR types such as `Option<Box<Node>>`, `Vec<Node>`, and nested container shapes.

**Architecture:** Add name-only reference hooks to `HasTypeDef`, route blanket `HasTypeSpec` and `Declare` through those hooks, and have `#[derive(XDR)]` override the hooks with static XDR names. Existing manual `HasTypeDef` implementations keep default behavior, while derived named types avoid re-entering `type_def()` when only a field type name is needed.

**Tech Stack:** Rust, `xdr-repr`, `xdr-derive`, `syn`, `quote`, repr integration tests.

---

## File Structure

- Modify `xdr-repr/src/ty.rs`: add `HasTypeDef::type_ident`, add `HasTypeDef::named_declaration`, route blanket traits through the new hooks, and delegate pointer/reference wrappers.
- Modify `xdr-derive/src/generate_type.rs`: generate static name-only hook overrides for all derived `HasTypeDef` impls.
- Create `tests/repr/recursive_schemas.rs`: focused recursive schema formatting tests.
- Modify `tests/repr/main.rs`: include the new recursive schema test module.

## Task 1: Add Red Recursive Schema Tests

**Files:**
- Create: `tests/repr/recursive_schemas.rs`
- Modify: `tests/repr/main.rs`

- [ ] **Step 1: Create focused recursive schema tests**

Create `tests/repr/recursive_schemas.rs` with this full content:

```rust
#![allow(dead_code)]

use xdrs::xdr_repr::*;
use xdrs::*;

pub mod optional_boxed_self {
    use super::*;

    #[derive(XDR)]
    struct Node {
        value: u32,
        next: Option<Box<Node>>,
    }

    check_format! { optional_boxed_self_reference_renders_finitely
        Node::def()
        =>
            "struct Node {"
            "\tunsigned int value;"
            "\tNode *next;"
            "};"
    }
}

pub mod owned_vec_self {
    use super::*;

    #[derive(XDR)]
    struct RecursiveVecNode {
        children: Vec<RecursiveVecNode>,
    }

    check_format! { owned_vec_self_reference_renders_finitely
        RecursiveVecNode::def()
        =>
            "struct RecursiveVecNode {"
            "\tRecursiveVecNode children<>;"
            "};"
    }
}

pub mod fixed_array_of_boxed_self {
    use super::*;

    #[derive(XDR)]
    struct ArrayNode {
        children: [Box<ArrayNode>; 2],
    }

    check_format! { fixed_array_of_boxed_self_reference_renders_finitely
        ArrayNode::def()
        =>
            "struct ArrayNode {"
            "\tArrayNode children[2];"
            "};"
    }
}

pub mod bounded_vec_self {
    use super::*;

    #[derive(XDR)]
    struct BoundedVecNode {
        children: VecM<BoundedVecNode, 3>,
    }

    check_format! { bounded_vec_self_reference_renders_finitely
        BoundedVecNode::def()
        =>
            "struct BoundedVecNode {"
            "\tBoundedVecNode children<3>;"
            "};"
    }
}

pub mod nested_optional_vec_self {
    use super::*;

    #[derive(XDR)]
    struct NestedOptionalVecNode {
        nested: Vec<Option<Box<NestedOptionalVecNode>>>,
    }

    check_format! { nested_optional_vec_self_reference_renders_finitely
        NestedOptionalVecNode::def()
        =>
            "struct NestedOptionalVecNode {"
            "\tstruct {"
            "\t\tNestedOptionalVecNode *unnamed_0;"
            "\t} nested<>;"
            "};"
    }
}

pub mod result_boxed_self {
    use super::*;

    #[derive(XDR)]
    struct ResultNode {
        payload: Result<Box<ResultNode>, ()>,
    }

    check_format! { result_boxed_self_payload_renders_finitely
        ResultNode::def()
        =>
            "struct ResultNode {"
            "\tunion switch (int which)"
            "\t{"
            "\tcase 0:"
            "\t\tResultNode ok;"
            "\tcase 1:"
            "\t\tvoid;"
            "\t} payload;"
            "};"
    }
}
```

- [ ] **Step 2: Register the new repr module**

Add this line to `tests/repr/main.rs`:

```rust
pub mod recursive_schemas;
```

Place it after the existing `pub mod derive_results;` line so the file reads:

```rust
pub mod derive_arrays;
pub mod derive_enums;
pub mod derive_floats;
pub mod derive_refs;
pub mod derive_results;
pub mod recursive_schemas;
pub mod derive_structs;
```

- [ ] **Step 3: Verify the tests fail before implementation**

Run:

```bash
cargo test --workspace --all-features --test repr optional_boxed_self_reference_renders_finitely -- --nocapture --test-threads=1
```

Expected: FAIL before implementation. The test process should report a stack overflow or abort while evaluating `Node::def()`, showing the test captures the current bug.

Do not commit yet. These red tests should be committed with the implementation that makes them pass.

## Task 2: Add Name-Only Hooks to `HasTypeDef`

**Files:**
- Modify: `xdr-repr/src/ty.rs`

- [ ] **Step 1: Extend `HasTypeDef` with provided name-only methods**

Replace the current trait block:

```rust
pub trait HasTypeDef {
    /// Returns the XDR type definition for this type.
    fn type_def() -> TypeDef;
}
```

with:

```rust
pub trait HasTypeDef {
    /// Returns the XDR type definition for this type.
    fn type_def() -> TypeDef;

    /// Returns the XDR identifier for this named type.
    fn type_ident() -> Ident {
        type_def_ident(Self::type_def())
    }

    /// Creates a declaration that references this named type.
    fn named_declaration(name: Ident) -> Declaration {
        named_declaration_from_type_def(name, Self::type_def())
    }
}
```

- [ ] **Step 2: Add a helper for default named declarations**

Add this helper immediately after `decl_ident`:

```rust
fn named_declaration_from_type_def(name: Ident, type_def: TypeDef) -> Declaration {
    let declare_named = |ty_ident| Declaration::Named {
        name,
        ty: TypeSpec::Ident(ty_ident),
    };

    match type_def {
        TypeDef::Def(decl) => match decl.name() {
            Some(ty_ident) => declare_named(ty_ident.clone()),
            None => decl,
        },
        TypeDef::Struct(ty_ident, _) => declare_named(ty_ident),
        TypeDef::Enum(ty_ident, _) => declare_named(ty_ident),
        TypeDef::Union(ty_ident, _) => declare_named(ty_ident),
    }
}
```

- [ ] **Step 3: Route the blanket `HasTypeSpec` impl through `type_ident`**

Replace:

```rust
impl<T: HasTypeDef> HasTypeSpec for T {
    fn type_spec() -> TypeSpec {
        let ident = type_def_ident(T::type_def());
        TypeSpec::Ident(ident.clone())
    }
}
```

with:

```rust
impl<T: HasTypeDef> HasTypeSpec for T {
    fn type_spec() -> TypeSpec {
        TypeSpec::Ident(T::type_ident())
    }
}
```

- [ ] **Step 4: Route the blanket `Declare` impl through `named_declaration`**

Replace the current `impl<T> Declare for T where T: HasTypeDef` block:

```rust
impl<T> Declare for T
where
    T: HasTypeDef,
{
    fn declare(name: Ident) -> Declaration {
        let declare_named = |ty_ident| Declaration::Named {
            name,
            ty: TypeSpec::Ident(ty_ident),
        };

        match T::type_def() {
            TypeDef::Def(decl) => match decl.name() {
                Some(ty_ident) => declare_named(ty_ident.clone()),
                None => decl,
            },
            TypeDef::Struct(ty_ident, _) => declare_named(ty_ident),
            TypeDef::Enum(ty_ident, _) => declare_named(ty_ident),
            TypeDef::Union(ty_ident, _) => declare_named(ty_ident),
        }
    }
}
```

with:

```rust
impl<T> Declare for T
where
    T: HasTypeDef,
{
    fn declare(name: Ident) -> Declaration {
        T::named_declaration(name)
    }
}
```

- [ ] **Step 5: Delegate `Box<T>` name-only hooks**

Replace the current boxed `HasTypeDef` impl:

```rust
impl<T: HasTypeDef> HasTypeDef for Box<T> {
    fn type_def() -> TypeDef {
        T::type_def()
    }
}
```

with:

```rust
impl<T: HasTypeDef> HasTypeDef for Box<T> {
    fn type_def() -> TypeDef {
        T::type_def()
    }

    fn type_ident() -> Ident {
        T::type_ident()
    }

    fn named_declaration(name: Ident) -> Declaration {
        T::named_declaration(name)
    }
}
```

- [ ] **Step 6: Preserve `void` declaration behavior**

Replace:

```rust
impl HasTypeDef for () {
    fn type_def() -> TypeDef {
        TypeDef::Def(Declaration::Void)
    }
}

impl<T> HasTypeDef for core::marker::PhantomData<T> {
    fn type_def() -> TypeDef {
        TypeDef::Def(Declaration::Void)
    }
}
```

with:

```rust
impl HasTypeDef for () {
    fn type_def() -> TypeDef {
        TypeDef::Def(Declaration::Void)
    }

    fn named_declaration(_name: Ident) -> Declaration {
        Declaration::Void
    }
}

impl<T> HasTypeDef for core::marker::PhantomData<T> {
    fn type_def() -> TypeDef {
        TypeDef::Def(Declaration::Void)
    }

    fn named_declaration(_name: Ident) -> Declaration {
        Declaration::Void
    }
}
```

- [ ] **Step 7: Delegate reference name-only hooks**

Replace:

```rust
impl<T: HasTypeDef> HasTypeDef for &T {
    fn type_def() -> TypeDef {
        T::type_def()
    }
}

impl<T: HasTypeDef> HasTypeDef for &mut T {
    fn type_def() -> TypeDef {
        T::type_def()
    }
}
```

with:

```rust
impl<T: HasTypeDef> HasTypeDef for &T {
    fn type_def() -> TypeDef {
        T::type_def()
    }

    fn type_ident() -> Ident {
        T::type_ident()
    }

    fn named_declaration(name: Ident) -> Declaration {
        T::named_declaration(name)
    }
}

impl<T: HasTypeDef> HasTypeDef for &mut T {
    fn type_def() -> TypeDef {
        T::type_def()
    }

    fn type_ident() -> Ident {
        T::type_ident()
    }

    fn named_declaration(name: Ident) -> Declaration {
        T::named_declaration(name)
    }
}
```

- [ ] **Step 8: Verify `xdr-repr` still compiles**

Run:

```bash
cargo check -p xdr-repr --all-features
```

Expected: PASS.

Do not commit yet. The recursive tests still need derived type overrides before they pass.

## Task 3: Generate Static Name Hooks in `#[derive(XDR)]`

**Files:**
- Modify: `xdr-derive/src/generate_type.rs`

- [ ] **Step 1: Add a helper for generated named-reference methods**

Add this helper after `from_input` and before `from_struct`:

```rust
fn named_type_reference_methods(ty_name: String) -> TokenStream {
    quote! {
        fn type_ident() -> __xdrs::Ident {
            __xdrs::ident(#ty_name)
        }

        fn named_declaration(name: __xdrs::Ident) -> __xdrs::Declaration {
            __xdrs::Declaration::Named {
                name,
                ty: __xdrs::TypeSpec::Ident(__xdrs::ident(#ty_name)),
            }
        }
    }
}
```

- [ ] **Step 2: Add the helper output to derived struct impls**

In `from_struct`, add:

```rust
let type_reference_methods = named_type_reference_methods(ty_name.clone());
```

immediately after:

```rust
let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
```

Then change the generated impl from:

```rust
impl #impl_generics __xdrs::HasTypeDef for #ty_ident #ty_generics #where_clause {
    fn type_def() -> __xdrs::TypeDef {
        #struct_typedef
    }
}
```

to:

```rust
impl #impl_generics __xdrs::HasTypeDef for #ty_ident #ty_generics #where_clause {
    fn type_def() -> __xdrs::TypeDef {
        #struct_typedef
    }

    #type_reference_methods
}
```

- [ ] **Step 3: Add the helper output to fieldless enum impls**

In `from_enum_fieldless`, add:

```rust
let ty_name = attrs
    .name
    .clone()
    .unwrap_or_else(|| ty_ident.to_string());
let type_reference_methods = named_type_reference_methods(ty_name.clone());
```

immediately after:

```rust
let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
```

Then change the generated impl from:

```rust
impl #impl_generics __xdrs::HasTypeDef for #ty_ident #ty_generics #where_clause {
    fn type_def() -> __xdrs::TypeDef {
        #enum_typedef
    }
}
```

to:

```rust
impl #impl_generics __xdrs::HasTypeDef for #ty_ident #ty_generics #where_clause {
    fn type_def() -> __xdrs::TypeDef {
        #enum_typedef
    }

    #type_reference_methods
}
```

- [ ] **Step 4: Add the helper output to data-bearing enum impls**

In `from_enum_with_fields`, add:

```rust
let ty_name = attrs
    .name
    .clone()
    .unwrap_or_else(|| ty_ident.to_string());
let type_reference_methods = named_type_reference_methods(ty_name.clone());
```

immediately after:

```rust
let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
```

Then change the first pushed generated impl from:

```rust
impl #impl_generics __xdrs::HasTypeDef for #ty_ident #ty_generics #where_clause {
    fn type_def() -> __xdrs::TypeDef {
        #union_typedef
    }
}
```

to:

```rust
impl #impl_generics __xdrs::HasTypeDef for #ty_ident #ty_generics #where_clause {
    fn type_def() -> __xdrs::TypeDef {
        #union_typedef
    }

    #type_reference_methods
}
```

- [ ] **Step 5: Run the targeted recursive tests**

Run:

```bash
cargo test --workspace --all-features --test repr recursive_schemas -- --nocapture --test-threads=1
```

Expected: PASS. All six recursive schema tests should render finite schemas.

- [ ] **Step 6: Commit the red tests and implementation**

Run:

```bash
git add xdr-repr/src/ty.rs xdr-derive/src/generate_type.rs tests/repr/main.rs tests/repr/recursive_schemas.rs
git commit -m "Support recursive schema type identity"
```

Expected: commit succeeds.

## Task 4: Run Regression Tests

**Files:**
- No file changes.

- [ ] **Step 1: Run the repr integration test suite**

Run:

```bash
cargo test --workspace --all-features --test repr -- --nocapture --test-threads=1
```

Expected: PASS. Existing repr formatting tests should retain their expected output.

- [ ] **Step 2: Run focused result tests**

Run:

```bash
cargo test --workspace --all-features --test repr derive_results -- --nocapture --test-threads=1
```

Expected: PASS. `Result<BytesM<4>, ()>` and `Result<BytesM<4>, PhantomData<u32>>` should still render `void` error arms.

- [ ] **Step 3: Run the full workspace suite**

Run:

```bash
cargo test --workspace --all-features
```

Expected: PASS.

- [ ] **Step 4: Check final diff**

Run:

```bash
git status --short --branch
git diff --stat HEAD
```

Expected: `git status --short --branch` prints only the branch header and `git diff --stat HEAD` prints no file changes.
