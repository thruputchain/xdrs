# ConstWriteXDR: plain `&T` reference support

## Problem

The `ConstWriteXDR` derive macro handles `&[u8]`, `&str`, and `&[T]` references
but rejects plain `&T` references. This blocks the sdk_rs contract-spec view
types from deriving `ConstWriteXDR`, since they contain fields and enum variants
like:

```rust
enum ContractTypeView<'a> {
    Option(&'a ContractTypeView<'a>),
    Vec(&'a ContractTypeView<'a>),
    Tuple(&'a [ContractTypeView<'a>]),
    Named(&'a str),
    // ...
}
```

`Tuple` and `Named` already work (`&[T]` and `&str`). `Option` and `Vec` fail
because their single field is `&'a ContractTypeView<'a>` — a plain `&T` that
none of the three recursive emitters recognise.

## Solution

Add one branch per emitter in `xdr-derive/src/generate_const.rs` that catches
any `syn::Type::Reference` not already handled by the specific reference checks
(`is_u8_slice_ref`, `is_str_ref`, `slice_ref_inner`). Each branch dereferences
once and recurses into the referent type, mirroring the runtime blanket impl at
`src/write_xdr.rs:133`:

```rust
impl<T: WriteXDR> WriteXDR for &T {
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
        (*self).write_xdr(w)
    }
}
```

### Changes

All changes are in `xdr-derive/src/generate_const.rs`.

#### 1. `check_supported_const_shape` (after tuple_elems, before Path fallthrough)

```rust
if let syn::Type::Reference(r) = ty {
    return check_supported_const_shape(&r.elem);
}
```

#### 2. `const_size_value_expr` (after tuple_elems, before fallthrough)

```rust
if let syn::Type::Reference(r) = ty {
    return const_size_value_expr(quote! { *#expr_ref }, &r.elem, fmt);
}
```

#### 3. `const_write_value_expr` (after tuple_elems, before fallthrough)

```rust
if let syn::Type::Reference(r) = ty {
    return const_write_value_expr(quote! { *#expr_ref }, &r.elem, fmt);
}
```

### Why `*expr_ref` is correct

`expr_ref` is always a reference to the field value. For a field `foo: &'a Bar`:

- `expr_ref` = `&self.foo` has type `&&'a Bar`
- `*expr_ref` = `self.foo` has type `&'a Bar`

The recursive call sees inner type `Bar` and expression `self.foo` (which is
`&Bar`). The user-defined fallthrough generates `(self.foo).const_cxs_size()`,
which Rust auto-derefs through `&Bar` to call `Bar::const_cxs_size(&self)`.

For nested references like `&&T`, the handler fires twice, peeling one layer
each time.

### Ordering safety

The specific reference handlers (`is_u8_slice_ref`, `is_str_ref`,
`slice_ref_inner`) all appear earlier in the check chain and short-circuit
before the general `Type::Reference` branch is reached. No existing behaviour
changes.

## Tests

Extend `tests/borrowed_slices.rs` with:

1. **Struct with `&'a Leaf` field** — the minimal case, proving the deref path
   works for user-defined types.

2. **Enum mirroring `ContractTypeView`** — variants with `&'a Leaf` fields
   (both tuple and named), confirming the sdk_rs pattern compiles and encodes
   correctly.

3. **Const round-trip** — compile-time encode via `ConstWriter`, compare against
   runtime `WriteXDR` output for both CXS and TXS.

## Scope

- One file changed: `xdr-derive/src/generate_const.rs` (~9 lines)
- One test file extended: `tests/borrowed_slices.rs`
- No new dependencies, no API changes, no breaking changes
- Version bump: patch (0.5.5 -> 0.5.6)
