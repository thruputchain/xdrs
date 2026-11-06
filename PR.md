Let a closed XDR enum switch a union: derive(XDRDiscriminant)

A union can name its switch type with `#[xdr(discriminant = "T")]`, and `T`
must implement `XDRDiscriminant`. xdrs implements it for `i32`, `u32` and
`bool`. For an enum, the impl had to be written by hand. x23c writes one for
each XDR enum it emits (since hogd#4724), and that is how protocol-rs
switches its unions on enums. A closed Rust enum could not implement the
trait correctly, because `from_discriminant` had no way to reject a value
that is none of its members.

This PR adds a derive for the impl and makes `from_discriminant` fallible.

(This is independent of #379, which makes `derive(XDR)` describe the switch.)

### Changes

- **`#[derive(XDRDiscriminant)]`**, re-exported as `xdrs::XDRDiscriminant`.
  Either shape is encoded as a signed `int`:
  - **A fieldless `#[repr(i32)]` enum** is closed: its tags are its variants'
    values.
  - **A newtype over `i32`**, the shape x23c emits for XDR enums, is open:
    every `i32` is a value. The derive produces the same impl that x23c
    writes by hand.

  The derive rejects an enum with fields, any other struct, a Rust `union`,
  and an enum that has `#[xdr(discriminant)]` itself, since that makes it a
  union.
- **`XDRDiscriminant::from_discriminant` now returns `Option<Self>`**, because
  a closed enum has no value for a tag outside its members. `bool` returns
  `None` for anything but 0 and 1.

  **This is a breaking change.** Every impl written by hand stops compiling,
  including the ones x23c generates. Checking protocol-rs against this branch
  fails with E0053 ("method `from_discriminant` has an incompatible type for
  trait"), 18 times in `thruput-xdr` alone. Before protocol-rs moves to this
  release, x23c must emit either
  `fn from_discriminant(d: i32) -> Option<Self> { Some(Self(d)) }` or
  `derive(XDRDiscriminant)` in place of its impl.
- **A union's default arm** converts the wire discriminant to the switch type
  before it reads the payload. When there is no such value, decoding fails
  with `ReadError::bad_discriminator(d)`. With a closed enum, a value that is
  none of its members is therefore rejected rather than stored.
- **Docs:** `WriteXDR` now documents `discriminant`. The new derive's docs
  include round-trip examples for both shapes.

### Tests

Four new tests in `tests/enums.rs`:

- An open newtype switches a union. They check the exact TXS and CXS bytes,
  including unknown and negative members held in the default arm.
- A closed enum switches a union. A member that no case names lands in the
  default arm, and a value that is no member fails with `BadDiscriminator`.
- A closed enum switches a union that has no default arm.
- `discriminant` and `from_discriminant` are called directly for implicit
  and explicit tags, for the newtype, and for `i32`, `u32` and `bool`.

### Not included

Schema output (`derive(XDR)`) is unchanged. On main, `derive(XDR)` ignores
`discriminant` and describes the switch as a generated `{Name}Type` enum.

After #379, the switch is described by its type, and each case is labelled
through `SwitchType`. That works for a closed enum that derives `XDR`. A
newtype's `derive(XDR)` does not describe an enum, so its labels would panic
with "not an enum". Fixing that needs the newtype's members, which a later
`#[xdr(enum)]` attribute could supply. (Protocol types do not derive `XDR`
today.)
