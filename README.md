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

The ONC-RPC stack built on `xdrs` — `rpc_api` (the stable API boundary for
generated bindings) and `rpc_driver` (transports, pools, server) — lives in
[`rpc/`](rpc/README.md). It was previously the `thruputchain/xdr_rpc`
repository. `rpc_driver` shares the workspace version with the `xdrs`
crates and releases under the same `v<version>` tag; `rpc_api` versions
independently under `rpc_api-v<version>`.

## TODOs

- Consider adding an optional recursion depth limit for decoding
  potentially-recursive datastructures (see `read_xdr` module docs
  for current guidance on bounding stack usage externally)

[RFC-4506]: https://datatracker.ietf.org/doc/html/rfc4506

## Coverage

Install the report tool and LLVM tools for the pinned coverage nightly:

```sh
cargo +nightly-2026-05-26 install cargo-llvm-cov --version 0.9.1 --locked
rustup component add llvm-tools-preview --toolchain nightly-2026-05-26
bash scripts/coverage.sh
```

The coverage workflow runs the same script and uploads `coverage` artifacts
containing HTML (`target/llvm-cov/html/index.html`) and LCOV.
It merges the allocating workspace, no-alloc, compact-error, and permissive
integer tests. This is a baseline report, without a minimum percentage gate;
doctests and benchmarks are not included. Generated code remains instrumented.
The macro generator's own coverage and execution of its generated methods are
separate: generated methods are attributed to their invocation sites, including
those in integration tests.

### Generated code coverage

Derive-generated methods are always instrumented, like any other code. The
derives emit no coverage attributes, so consuming crates need no nightly feature
gate and no `cfg` registration, and they see no `unexpected_cfgs` diagnostics.

Earlier versions emitted `#[cfg_attr(coverage_nightly, coverage(off))]` to keep
generated methods out of reports, behind an opt-out `include-generated-coverage`
feature. That feature is gone. Because `coverage(off)` is still unstable, the
attribute had to be gated on a `cfg` that only cargo-llvm-cov sets — which every
downstream crate then had to register to build warning-free. Excluding generated
code from a report is not worth imposing that on consumers.
