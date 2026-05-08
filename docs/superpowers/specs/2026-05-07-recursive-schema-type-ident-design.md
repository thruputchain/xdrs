# Recursive Schema Type Identity

**Date**: 2026-05-07
**Issue**: [#201 - Recursive Schema Generation Through Boxed Self References Can Stack Overflow](https://github.com/groundhogchain/xdrs/issues/201)
**Severity**: Medium (schema-generation DoS / schema correctness)

## Problem

`#[derive(XDR)]` accepts recursive Rust shapes such as `Option<Box<Node>>` and
`Vec<Node>`, but schema generation can stack overflow when the recursive type's
metadata is requested.

The immediate cause is that schema field construction needs a named
`TypeSpec::Ident("Node")`, but the current trait path obtains that name by
building the full `Node::type_def()`:

- `Option<T>::declare` calls `T::type_spec()`.
- `Vec<T>::declare`, `[T; N]::declare`, and `VecM<T, MAX>::declare` call
  `T::type_spec()`.
- The blanket `HasTypeSpec for T: HasTypeDef` calls `T::type_def()` just to
  extract the type name.
- The blanket `Declare for T: HasTypeDef` also calls `T::type_def()` to produce
  a named field declaration.

For a recursive field, that re-enters the enclosing derived `type_def()` before
the outer struct definition has returned, so recursion has no terminating
name-only path.

## Goal

Recursive named schemas should be supported and should render finite named
references. The fix should cover the existing schema shapes that already rely on
`HasTypeSpec` or `Declare` for element/payload types, rather than rejecting
recursive types or special-casing only `Option<Box<Self>>`.

Examples that should terminate:

```rust
#[derive(xdrs::XDR)]
struct Node {
    value: u32,
    next: Option<Box<Node>>,
    children: Vec<Node>,
}
```

Expected schema:

```xdr
struct Node {
	unsigned int value;
	Node *next;
	Node children<>;
};
```

## Approach

Add a name-only schema identity path to `HasTypeDef` and route named references
through it.

`HasTypeDef` will gain two provided methods:

```rust
fn type_ident() -> Ident
fn named_declaration(name: Ident) -> Declaration
```

The default implementations preserve existing behavior:

- `type_ident()` derives the name from `Self::type_def()`.
- `named_declaration(name)` uses `Self::type_def()` to preserve current typedef
  and `void` handling for manual implementations.

Generated `#[derive(XDR)]` impls override both methods with static name-only
implementations. For derived structs, fieldless enums, and union enums, the
macro already knows the XDR type name, including any `#[xdr(name = "...")]`
override. Those generated impls can return that name without constructing the
full type definition.

## Changes

### 1. `xdr-repr/src/ty.rs` - extend `HasTypeDef`

Add the two provided methods to the trait:

```rust
pub trait HasTypeDef {
    fn type_def() -> TypeDef;

    fn type_ident() -> Ident {
        type_def_ident(Self::type_def())
    }

    fn named_declaration(name: Ident) -> Declaration {
        named_declaration_from_type_def(name, Self::type_def())
    }
}
```

The helper `named_declaration_from_type_def` should contain the existing logic
from the blanket `Declare for T: HasTypeDef` implementation, so the same
behavior remains available to manual implementations.

### 2. `xdr-repr/src/ty.rs` - route blanket traits through name-only hooks

Change the blanket `HasTypeSpec` implementation from:

```rust
let ident = type_def_ident(T::type_def());
TypeSpec::Ident(ident.clone())
```

to:

```rust
TypeSpec::Ident(T::type_ident())
```

Change the blanket `Declare for T: HasTypeDef` implementation to:

```rust
T::named_declaration(name)
```

This covers both schema paths:

- Container declarations that use `T::type_spec()`, such as `Option<T>`,
  `Vec<T>`, `[T; N]`, slices, and `VecM<T, MAX>`.
- Anonymous payload declarations that use `T::declare(name)`, such as
  `Result<T, E>` and tuple-generated fields.

### 3. `xdr-repr/src/ty.rs` - delegate pointer/reference wrappers

Update wrapper `HasTypeDef` impls so their name-only hooks delegate to the
wrapped type:

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

Apply the same pattern for `&T` and `&mut T`.

### 4. `xdr-repr/src/ty.rs` - preserve `void` behavior

Override `named_declaration` for `()` and `PhantomData<T>` so they continue to
render as `void` when used as fields:

```rust
fn named_declaration(_name: Ident) -> Declaration {
    Declaration::Void
}
```

Their `type_def()` behavior remains unchanged.

### 5. `xdr-derive/src/generate_type.rs` - emit static name hooks

For every generated `impl HasTypeDef`, add:

```rust
fn type_ident() -> __xdrs::Ident {
    __xdrs::ident(#type_name)
}

fn named_declaration(name: __xdrs::Ident) -> __xdrs::Declaration {
    __xdrs::Declaration::Named {
        name,
        ty: __xdrs::TypeSpec::Ident(__xdrs::ident(#type_name)),
    }
}
```

Use the same XDR name string that `type_def()` uses:

- Structs use the Rust type name or `#[xdr(name = "...")]`.
- Fieldless enums use the enum XDR name.
- Data-bearing enums use the union XDR name.

The generated `named_declaration` intentionally returns a named reference for
derived types. Derived structs/enums are named XDR definitions, so recursive
references should point at the name rather than trying to inline the definition.

## Tests

Add focused schema formatting tests under the existing repr integration tests.

### Recursive optional boxed self reference

```rust
#[derive(XDR)]
struct Node {
    value: u32,
    next: Option<Box<Node>>,
}
```

Expected:

```xdr
struct Node {
	unsigned int value;
	Node *next;
};
```

### Recursive owned vector

```rust
#[derive(XDR)]
struct RecursiveVecNode {
    children: Vec<RecursiveVecNode>,
}
```

Expected:

```xdr
struct RecursiveVecNode {
	RecursiveVecNode children<>;
};
```

### Additional recursive container coverage

Where current trait bounds allow the shapes, add tests for:

- `[Box<Node>; N]` rendering `Node field[N];`
- `VecM<Node, N>` rendering `Node field<N>;`
- `Vec<Option<Box<Node>>>` rendering an outer variable array whose element type
  is the existing embedded optional form containing `Node`.
- `Result<Box<Node>, ()>` or a similar anonymous payload shape that exercises
  the `Declare for T: HasTypeDef` path.

Direct `[Node; N]` self-fields are not a legal finite Rust shape, so direct
array element behavior remains covered by existing non-recursive named-type
array tests.

### Regression coverage

Run the existing repr tests that cover:

- Custom XDR names.
- Typedef-backed manual fields.
- Boxed fields.
- Tuples.
- Result schemas.
- Byte buffers.
- Primitive fields.

The expected output for existing tests should not change.

## Non-Goals

- Do not reject recursive schemas.
- Do not add thread-local recursion guards or schema-construction memoization.
- Do not add support for wrapper shapes that currently lack `Declare` or
  `HasTypeSpec` implementations, such as arbitrary `Box<Vec<T>>`, unless needed
  by the name-only fix itself.
- Do not change runtime `ReadXDR` or `WriteXDR` behavior.

## Compatibility

Adding provided trait methods is source-compatible for existing manual
`HasTypeDef` implementations. Manual implementations keep their current behavior
through the default methods. They can opt into recursion-safe named references
by overriding `type_ident()` and `named_declaration()`.

Derived implementations gain recursion-safe schema references automatically.

## Risk

The main risk is changing how named field declarations are produced. This is
controlled by keeping the default implementation equivalent to the current
blanket `Declare` logic and only using static name hooks for derived named
types.

A secondary risk is trait overlap or coherence surprises. The implementation
should avoid adding new broad trait impls; it only changes existing blanket impl
bodies and generated method bodies.
