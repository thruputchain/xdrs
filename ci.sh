#!/bin/bash

export RUSTFLAGS="--deny warnings"
set -e
set -x

# check the root library without alloc
cargo test -F xdr_common

# check feature "permissive"
cargo test -F permissive --test ints

# check feature "int128"
cargo test -F int128 --test ints

# check feature "int128_xdr"
cargo test -F int128_xdr --test ints

additive_features="alloc,derive,repr,xdr_common,ns,int128"

# check everything else
cargo test --workspace -F "$additive_tests"

# check that the benchmarks compile
cargo test -F "$additive_features" --benches

# check that the docs build cleanly
cargo doc -F "$additive_features"

set +x
echo "CI-CLEAN!"
