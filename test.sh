#!/bin/bash

set -x  # print commands
set -e  # fail on error

export RUSTFLAGS="--deny warnings"

noalloc_features="derive,ns,extras,xdr_common"
additive_features="${noalloc_features},alloc,repr"

# root library with no extra features
cargo test

# root library without alloc
cargo test -F "${noalloc_features}"

# root library with most features
cargo test -F "${additive_features}"

# alternate 128-bit-int encodings
cargo test -F "alloc,extras,int128_xdr" --test ints -- u128 i128

# path-aware decoding
cargo test -F derive,alloc --test state

# xdr-repr crate
cargo test -p xdr-repr --all-features

if [ "${1}" == "all" ]; then
    # check that the benchmarks compile
    cargo test -F "alloc,${additive_features}" --benches

    # xdr-repr crate without derive
    cargo test -F "ns,xdr_common" -p xdr-repr

    cargo doc --all-features

    ./ci.sh
fi
