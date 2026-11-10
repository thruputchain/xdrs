# xdrc: deferred work

The reasons behind these are in [DESIGN.md](DESIGN.md).

## Lower the AST to `xdr-repr`

Lower a parsed `.x` file to `xdr_repr::Specification`, dropping comments,
markers and positions, to get:

- **A round-trip check between codegen and derive.** Compare the lowered
  source with the spec that `derive(XDR)` rebuilds from the generated Rust
  under the `spec` feature, after normalizing both to their wire shape:
  resolve typedefs, replace named constants with their values, treat an
  enum as `int`, put inline types' bodies in place, and ignore names.
- **A canonical printed form** of any `.x` file, through xdr-repr's
  `Display`.

This has been implemented on branch `dbg/xdrc-repr-check` (`src/repr.rs`
and `check/tests/repr.rs`), but is not part of this branch.

## Closed enums

xdrc emits every XDR enum as an open newtype, `struct E(pub i32)`, whose
`XDRDiscriminant::from_discriminant` accepts any `i32`. A closed Rust enum
switching a union needs `from_discriminant` to return `Option<Self>`, so
that the default arm can reject a value that is not a member.

Branch `dbg/switch-newtype` has this parked: `derive(XDRDiscriminant)`, and
the fallible `from_discriminant`. The change breaks every implementation
written by hand, including those xdrc generates, so it should ship only
when Rust-first code needs a closed enum to switch a union. xdrc would then
change its impls in the same step, to `-> Option<Self> { Some(Self(d)) }`
or to the derive.

## Derived spec for enums

`derive(XDR)` does not yet read `#[xdr(discriminant)]` (that is xdrs#379),
so it describes each union's switch as an enum of its own, `{Name}Type`.
Once it does, a union switched on an XDR enum needs that enum described as
one, but xdrc emits it as an open newtype, whose members `derive(XDR)`
cannot see. An `#[xdr(enum)]` attribute listing the members, or a
`HasTypeDef` impl that xdrc emits, would fill the gap. Nothing builds the
spec of generated protocol types yet.

## Names in the derived spec

xdrc does not emit `#[xdr(discrim_var = ...)]`, so `derive(XDR)` names every
switch variable `which`. Both derives accept the attribute, so it can be
emitted, gated on `spec`, once a consumer needs the names. The same applies
to other names that the derived spec loses: typedefs, named bounds and enum
members.

## Unions

- **Default variant holding a case value.** A default variant built with a
  value that equals one of the cases encodes like that case and decodes as
  it. This is documented but not checked; xdrc could generate a checked
  constructor.
- **Aliases from another file.** The default variant's field gets the
  primitive type when the switch type is an alias of one defined in the
  same file. An alias from another file cannot be resolved, so the derived
  const writers fail on it.

## Elsewhere

- **`rpc/rpc-api-fixture`** is still a hand-written copy of the shape x23c
  emits for programs. Generate it from a `.x` fixture with xdrc.
- **Remove x23c's Rust backend** (`gen_rs.cc` and `--rust`) from hogd,
  once nothing generates Rust with it.
- **Publish the crate**, which is `publish = false` until xdrc replaces
  x23c's Rust backend.
