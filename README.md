# XDRS (XDR Serialization)

This package provides:

- Traits `ReadXDR` and `WriteXDR`, for converting between Rust values
  and the [XDR][RFC-4506] data model

- Derive-macros `ReadXDR`, `WriteXDR`, `XDR`, and `ConstWriteXDR`
  (see crate `xdr_derive`)

- Traits `serialize::Reader` and `serialize::Writer`, for converting
  between the XDR data model and some serialization format

- Implementations of `serialize::{Reader, Writer}`:
    - [CXS](CXS.md) (see module `cxs`)
    - TXS ("Traditional XDR Serialization") (see module `txs`)

The library is `no_std`. Optional features:

- `detailed-errors` (enabled by default): retain precise `ReadError`
  variants and diagnostic payloads. Contract builds can use
  `default-features = false` to replace them with a single compact error
  value while preserving identical validation behavior
- `alloc`: allocation-based interfaces (`Vec`, `String`, growable
  `BufWriter`s, owned `encode()` functions)
- `permissive`: accept non-minimal CXS integer encodings and non-zero
  TXS padding when decoding
- `int128`: `i128`/`u128` support
- `derive`: derive macros `ReadXDR`, `WriteXDR`, `XDR`, and
  `ConstWriteXDR` (crate `xdr_derive`)
- `repr`: datatypes representing XDR specifications (crate `xdr_repr`;
  implies `alloc`)
- `ns`: C++-like namespaces for XDR specs in `xdr_repr` (implies `repr`)
- `heapless`: serialization support for `heapless` types, plus schema
  support when combined with `repr`
- `serde` / `schemars`: serde and JSON-schema support for `xdr_common`
  types

See the crate docs (`cargo doc --open`) for details on each feature and
the full API.

## ONC-RPC (`rpc/`)

`thruput-rpc-api` (the stable API boundary for generated bindings) and
`thruput-rpc-driver` (transports, pools, server) live in
[`rpc/`](rpc/README.md).

On crates.io every crate here is published with a `thruput-` prefix:
`thruput-xdrs`, `thruput-xdr-common`, `thruput-xdr-derive`, `thruput-xdr-repr`,
`thruput-rpc-api` and `thruput-rpc-driver`. The `xdr` crates keep their
library names (`xdrs::`); the rpc crates are imported by package name
(`thruput_rpc_api::`, `thruput_rpc_driver::`).

## Coverage

```sh
cargo +nightly-2026-05-26 install cargo-llvm-cov --version 0.9.1 --locked
rustup component add llvm-tools-preview --toolchain nightly-2026-05-26
bash scripts/coverage.sh
```

This writes HTML (`target/llvm-cov/html/index.html`) and LCOV, merging the
allocating, no-alloc, compact-error and permissive test runs. CI runs the same
script and uploads the reports; there is no minimum percentage. Doctests and
benchmarks are excluded.

Derive-generated methods are instrumented like any other code and are
attributed to the invocation site.

## TODOs

- Optional recursion depth limit for decoding recursive datastructures (see
  the `read_xdr` module docs for bounding stack usage today).

[RFC-4506]: https://datatracker.ietf.org/doc/html/rfc4506
