#!/bin/bash

set -x  # print commands
set -e  # fail on error

additive_features="extras,xdr_common"

# test the root library
cargo test -F "alloc,${additive_features}"

# check that the benchmarks compile
cargo test -F "alloc,${additive_features}" --benches

# test the xdr-repr crate
cargo test -p xdr-repr -F derive,ns,xdr_common

if [ "${1}" == "all" ]; then
    # test with various feature combinations

    # root library without alloc
    cargo test -F "${additive_features}"

    # xdr-repr crate without derive
    cargo test -F ns,xdr_common -p xdr-repr
fi
