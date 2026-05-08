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

Please see `cargo doc` for the detailed API.

## TODOs

(See "TODO" comments in sources.)

- Provide an out-parameter (i.e., in-place) decoding interface

- Consider adding an optional recursion depth limit for decoding
  potentially-recursive datastructures (see `read_xdr` module docs
  for current guidance on bounding stack usage externally)

[RFC-4506]: https://datatracker.ietf.org/doc/html/rfc4506
