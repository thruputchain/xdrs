#!/bin/bash

set -x  # print commands
set -e  # fail on error

export RUSTFLAGS="--deny warnings"

additive_features="derive,repr,ns,extras,xdr_common"

# test the root library
cargo test -F "alloc,${additive_features}"

# test alternate 128-bit-int encodings
cargo test -F "alloc,extras,int128_xdr" --test ints -- u128 i128

# check that the benchmarks compile
cargo test -F "alloc,${additive_features}" --benches

if [ "${1}" == "all" ]; then
    # test the xdr-repr crate alone
    cargo test -p xdr-repr --all-features

    # root library without alloc
    cargo test -F "${additive_features}"

    # xdr-repr crate without derive
    cargo test -F "ns,xdr_common" -p xdr-repr
fi
