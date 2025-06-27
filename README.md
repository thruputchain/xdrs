# XDRS (XDR Serialization)

This package provides:

- Traits `ReadXDR` and `WriteXDR`, for converting between
Rust values and the XDR data model.

- Derive-macros for generating `ReadXDR` and `WriteXDR` (see
crate `xdr_derive`)

- Traits `serialize::Reader` and `serialize::Writer`, for converting
between the XDR data model and some serialization format

- [CXS](CXS.md) implementations of `serialize::Reader` and
`serialize::Writer` (see module `serialize_cxs`)

The library is `no_std`, but a few extra things can be
enabled with feature `alloc`.
