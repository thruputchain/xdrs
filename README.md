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

### Excluding generated code in applications

`exclude-generated-coverage` is **off by default**. Enable it on your `xdrs`
dependency (alongside `derive`), or on `xdr-derive` when using it directly:

```toml
[dependencies]
xdrs = { registry = "groundhog", version = "0.9", features = ["derive", "exclude-generated-coverage"] }
```

In each crate that invokes the derives, add:

```rust,ignore
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]
```

Register that cfg so ordinary builds also accept the crate attribute:

```toml
[lints.rust]
unexpected_cfgs = { level = "warn", check-cfg = ["cfg(coverage_nightly)"] }
```

Then run `cargo +nightly llvm-cov` as usual. The feature makes the generator emit
`#[coverage(off)]` on its generated methods only when the consuming crate is built
with `coverage_nightly`, which cargo-llvm-cov sets on nightly. This includes
readers, checked/unchecked writers, schema methods, and const writers/size
methods. Handwritten methods remain instrumented. Normal builds and stable
coverage builds are unchanged; `--no-cfg-coverage-nightly` also disables the
exclusion.

The opt-in avoids silently reducing existing reports or requiring nightly
attributes in every consumer. Cargo features are additive and can be unified
across dependencies: enabling this feature affects all users of the same
`xdr-derive` instance, and every consuming crate then needs the feature gate
above for nightly coverage. SDKs that re-export these derives can forward the
feature. Leave it disabled when measuring generated code itself, as this repo's
coverage script does; `--all-features` would enable the exclusion.
