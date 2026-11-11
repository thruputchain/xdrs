# XDRS core

The stable trait layer of [XDRS](../README.md): `ReadXDR`, `WriteXDR`,
`EncodesAs`, `XDRDiscriminant`, the `Reader`/`Writer` interfaces and their
errors, and the per-format `ConstMaxSize` bounds and `ConstWriter`s that the
const derives target.

These are the items that a crate's public API exposes when its types derive
XDR. They are versioned separately from the codecs in `thruput-xdrs`, which
re-exports this crate at the same paths, so that a breaking codec release is
not a breaking release for every crate with XDR types. Depend on
`thruput-xdrs` to encode and decode.

See "Versioning and releases" in the [repository README](../README.md).
