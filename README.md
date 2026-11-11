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

## Versioning and releases

The crates release in three groups, each with its own version and tag:

| Group | Crates | Tag |
|---|---|---|
| workspace | `thruput-xdrs`, `thruput-xdr-common`, `thruput-xdr-derive`, `thruput-xdr-repr` | `v<version>` |
| rpc_api | `thruput-rpc-api` | `rpc_api-v<version>` |
| rpc_driver | `thruput-rpc-driver` | `rpc_driver-v<version>` |

A driver release doesn't publish new xdrs crates, and the driver depends on
`thruput-xdrs` and `thruput-rpc-api` with caret requirements, so an xdrs patch
release doesn't need a driver release either.

Every crate whose types derive XDR (all of protocol-rs) exposes the xdrs
traits in its public API, so a breaking xdrs release is a breaking release for
those crates too. Pick the bump accordingly. Pre-1.0, the middle number is the
breaking level:

- **Patch** (`0.18.0` → `0.18.1`) for anything that doesn't break a caller:
  new items, new trait impls, new trait methods with defaults, new derive
  attributes, fixes, performance. Dependents that ask for `"0.18.0"` pick it
  up with `cargo update`; protocol-rs doesn't need to release. This should be
  the usual case.
- **Minor** (`0.18.x` → `0.19.0`) for a breaking change: a removed or renamed
  item or feature, a new required trait item, a changed signature, or a
  change to what an existing type encodes to. protocol-rs then has to bump
  every crate, so batch breaking changes into as few releases as possible.

`publish.yml` (and the "Release dry run checks" workflow) runs
cargo-semver-checks on each group against that group's previous release tag,
so a patch bump over an API break fails before anything is tagged. It
cannot see a changed wire encoding behind an unchanged API (like encoding a
union's discriminant by its switch type); that one is on the reviewer.

To release: bump the group's version (the workspace group:
`[workspace.package] version` plus the `=` pins and the `xdrs` entry under
`[workspace.dependencies]`; rpc_api and rpc_driver: their own `Cargo.toml`
plus their `[workspace.dependencies]` entry) and dispatch `publish.yml`. It
releases every group whose version moved and skips the others; a group that
changed without a bump fails the run.

## TODOs

- Optional recursion depth limit for decoding recursive datastructures (see
  the `read_xdr` module docs for bounding stack usage today).

[RFC-4506]: https://datatracker.ietf.org/doc/html/rfc4506
