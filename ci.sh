#!/bin/bash

export RUSTFLAGS="--deny warnings"
set -e
set -x

# check the root library without alloc
cargo test -F int128,int128_xdr

# check feature "permissive"
cargo test -F permissive,int128 --test ints

# check feature "int128"
cargo test -F int128 --test ints

# check everything else
cargo test --workspace --all-features

# Test on xdr-common with varying features
cargo test -p xdr-common --no-default-features
cargo test -p xdr-common --features alloc
cargo test -p xdr-common --features std
cargo test -p xdr-common --features alloc,std
cargo test -p xdr-common --features serde
cargo test -p xdr-common --features serde,std

# check that the benchmarks compile
cargo test --benches --all-features

# check that the docs build cleanly
cargo doc --workspace --all-features

# docsrs docs
RUSTDOCFLAGS="--cfg docsrs" cargo +nightly doc --workspace --all-features --no-deps

set +x
echo "CI-CLEAN!"
