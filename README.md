# CXS (Compact XDR Serialization)

This package provides:

- Traits `xdr::Reader` and `xdr::Writer`, for converting
between the XDR data model and some serialization format

- [CXS](CXS.md) implementations of `xdr::Reader` and
`xdr::Writer` (see module `xdr_cxs`)

- Traits `ReadXDR` and `WriteXDR`, for converting between
Rust values and the XDR data model.

- Derive-macros for generating `ReadXDR` and `WriteXDR` (see
crate `xdr_derive`)

The library is `no_std`, but a few extra things can be
enabled with feature `alloc`.
