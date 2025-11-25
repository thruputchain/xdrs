#!/bin/bash

export RUSTFLAGS="--deny warnings"
set -e
set -x

# check the root library without alloc
cargo test -F xdr_common

# check everything else
cargo test --workspace --all-features

# check that the benchmarks compile
cargo test --all-features --benches

# check that the docs build cleanly
cargo doc --all-features

set +x
echo "CI-CLEAN!"
