#!/bin/bash

set -x
set -e

export RUSTFLAGS="--deny warnings"

cargo test --workspace
cargo test --workspace --all-features
cargo test -F extras,alloc,derive --benches

(cd xdr-repr && bash scripts/test.sh)

echo "CI-CLEAN!"
