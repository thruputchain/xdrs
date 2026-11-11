# XDRS (XDR Serialization)

This package provides:

- Traits `ReadXDR` and `WriteXDR`, for converting between Rust values
  and the [XDR][RFC-4506] data model (defined in `thruput-xdr-common` and
  re-exported here; see [Versioning and releases](#versioning-and-releases))

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
library names (`xdrs::`, `xdr_common::`); the rpc crates are imported by
package name (`thruput_rpc_api::`, `thruput_rpc_driver::`).

## Versioning and releases

A crate whose public types derive XDR exposes some of this repository in its
own API: its types implement `ReadXDR`/`WriteXDR`, its fields are `VecM`s,
its RPC signatures take `impl EncodesAs<T>`. A breaking release of whatever
crate defines those items is a breaking release for that crate too. So the
items live in boundary crates that version independently and release only
when they change themselves:

| Crate | Holds | Tag |
|---|---|---|
| `thruput-xdr-common` | `VecM`, `BytesM`, `StringM`; `ReadXDR`, `WriteXDR`, `EncodesAs`, `XDRDiscriminant`; the `Reader`/`Writer` interfaces and their errors; `cxs`/`txs` `ConstMaxSize` and `ConstWriter` | `xdr_common-v<version>` |
| `thruput-rpc-api` | the RPC sender/handler traits and response/error types | `rpc_api-v<version>` |

Everything else (`thruput-xdrs`'s codecs, `thruput-xdr-derive`,
`thruput-xdr-repr`, `thruput-rpc-driver`) shares the workspace version and the
`v<version>` tag. `thruput-xdrs` re-exports `xdr_common` at the paths these
items have always had (`xdrs::ReadXDR`, `xdrs::serialize::reader::Reader`,
`xdrs::txs::ConstMaxSize`, ...), so depending crates need no changes.

What this buys: a breaking `thruput-xdrs` release (say 0.19 → 0.20) that
keeps the same `thruput-xdr-common` is not a breaking change for a crate whose
types derive XDR. Its public API names only `xdr_common` items, which are the
same items whichever `thruput-xdrs` a downstream crate uses, so it can move to
the new codecs in a patch release, or not at all. The derive macro's version never
matters: proc-macro output is not part of a crate's public API.

`xdr-common/fixture` keeps the boundary honest: its types derive every macro
against `xdr_common` alone (`#[xdr(crate = "xdr_common")]`), so a derive that
starts emitting a codec-only path fails to compile there.

Releasing:

- A change to a boundary crate is a release of that crate: bump the version
  in its `Cargo.toml` and the matching requirement in the root
  `[workspace.dependencies]`. Additive changes are patch releases (`0.19.x`);
  a breaking change (a new required trait item, a changed signature) is a
  minor release and is breaking for every crate that derives XDR, so batch
  those.
- A workspace release bumps `[workspace.package] version` and the `=` pins of
  `thruput-xdr-derive` and `thruput-xdr-repr`. It does not touch the boundary
  crates.
- `publish.yml` releases every group in one pass, skipping a boundary crate
  whose version is already tagged and published.

## TODOs

- Optional recursion depth limit for decoding recursive datastructures (see
  the `read_xdr` module docs for bounding stack usage today).

[RFC-4506]: https://datatracker.ietf.org/doc/html/rfc4506
