# XDRS (XDR Serialization)

This package provides:

- Traits `ReadXDR` and `WriteXDR`, for converting between Rust values
  and the [XDR][RFC-4506] data model

- Derive-macros for generating `ReadXDR` and `WriteXDR` (see crate
  `xdr_derive`)

- Traits `serialize::Reader` and `serialize::Writer`, for converting
  between the XDR data model and some serialization format

- Implementations of `serialize::{Reader, Writer}`:
    - [CXS](CXS.md) (see module `cxs`)
    - TXS ("Traditional XDR Serialization") (see module `txs`)

The library is `no_std`, but a few extra things can be
enabled with feature `alloc`.

## TODOs

(See "TODO" comments in sources.)

- Address some memory-leaks that can happen around read failures

- Provide an out-parameter (i.e., in-place) decoding interface

- Limit stack depth when reading potentially-recursive datastructures

- Check handling of not-minimal encodings, with and without feature "permissive"

- cargo docs

[RFC-4506]: https://datatracker.ietf.org/doc/html/rfc4506
