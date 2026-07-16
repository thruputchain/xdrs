#!/bin/bash

export RUSTFLAGS="--deny warnings"
export RUSTDOCFLAGS="--deny warnings"
set -e
set -x

# check the root library without alloc
cargo test -F int128

# run the same malformed-input corpus with compact ReadError diagnostics
cargo test --no-default-features
cargo test --no-default-features --features derive --test read_errors

# check derive macros without alloc (const writers for VecM/BytesM/StringM
# are only exercised with non-empty static data in no-alloc builds)
cargo test -F derive,int128

# check feature "permissive"
cargo test -F permissive,int128 --test ints

# check feature "int128"
cargo test -F int128 --test ints

# check everything else
cargo test --workspace --all-features

# Test on xdr-common with varying features
cargo test -p xdr-common --no-default-features
cargo test -p xdr-common --features alloc
cargo test -p xdr-common --features serde
cargo test -p xdr-common --features schemars

cargo +nightly build -p xdr-common --no-default-features -Zbuild-std=core,alloc
cargo +nightly build -p xdr-common --features alloc -Zbuild-std=core,alloc

# check that the benchmarks compile
cargo test --benches --all-features

# check that the docs build cleanly
cargo doc --workspace --all-features

# docsrs docs
RUSTDOCFLAGS="--cfg docsrs" \
    cargo +nightly doc --workspace --all-features --no-deps

# Test Documentation Examples
cargo test --doc -p xdr-repr --features "ns,int128"
cargo test --doc --features "alloc,derive,repr,ns,int128"

cargo clippy --workspace --all-features
cargo +nightly clippy --all-features -p xdr-common -p xdr-repr -p xdr-derive -p xdrs

set +x

cargo fmt --check --quiet || {
  echo "Needs formatting!"
  exit 1
}

rg -i --glob '!/ci.sh' XXX && {
  echo "XXX found!"
  exit 1
}

echo "CI-CLEAN!"
