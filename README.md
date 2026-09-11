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
python3 scripts/check-generated-coverage.py
```

The coverage workflow runs the same scripts and uploads `coverage` artifacts
containing HTML (`target/llvm-cov/html/index.html`), LCOV, and regression reports.
It merges the allocating workspace, no-alloc, compact-error, and permissive
integer tests. This is a baseline report, without a minimum percentage gate;
doctests and benchmarks are not included. Generated code remains instrumented.
The macro generator's own coverage and execution of its generated methods are
separate: generated methods are attributed to their invocation sites, including
those in integration tests.

### Generated code coverage

Generated methods are **excluded by default** from nightly coverage reports.
Downstream applications and SDKs do not need to enable or forward a Cargo
feature. To measure generated code itself, enable the default-off
`include-generated-coverage` feature on `xdrs` (or directly on `xdr-derive`).
This repo's coverage script enables it in every run. Release and dry-run
workflows exclude it from their cargo-hack feature powersets; the coverage
regression fixture tests it separately.

Rust's `coverage(off)` attribute still requires a nightly feature gate. In each
crate that invokes the derives, add:

```rust,ignore
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]
```

Register the cfg so ordinary builds accept both this gate and the generated
attributes without warnings:

```toml
[lints.rust]
unexpected_cfgs = { level = "warn", check-cfg = ["cfg(coverage_nightly)"] }
```

Then run `cargo +nightly llvm-cov` as usual. The generator emits
`#[cfg_attr(coverage_nightly, coverage(off))]` on readers, checked/unchecked
writers, schema methods, and const writers/size methods. The cfg is evaluated
in the consuming crate; cargo-llvm-cov sets it on nightly. Handwritten methods
remain instrumented. Normal builds and stable coverage retain their existing
instrumentation behavior; `--no-cfg-coverage-nightly` also disables exclusion.

The inclusion feature is additive: enabling it anywhere in a dependency graph
can enable generated-code coverage for all users of the same `xdr-derive`
instance. `--all-features` includes generated code as well. Exclusion is the
baseline behavior rather than a Cargo default feature, so it also works with
`default-features = false` (commonly used by no-alloc consumers).
