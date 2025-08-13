#!/bin/bash

set -x  # print commands
set -e  # fail on error

export RUSTFLAGS="--deny warnings"

additive_features="derive,ns,extras,xdr_common"

# test the root library
cargo test -F "alloc,${additive_features}"

# test alternate 128-bit-int encodings
cargo test -F alloc,extras,int128_xdr --test ints -- u128 i128

# check that the benchmarks compile
cargo test -F "alloc,${additive_features}" --benches

# test the xdr-repr crate
cargo test -p xdr-repr --all-features

if [ "${1}" == "all" ]; then
    # test with various feature combinations

    # root library without alloc
    cargo test -F "${additive_features}"

    # xdr-repr crate without derive
    cargo test -F ns,xdr_common -p xdr-repr
fi
