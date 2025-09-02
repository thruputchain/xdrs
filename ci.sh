#!/bin/bash

set -x
set -e

export RUSTFLAGS="--deny warnings"

cargo test --workspace
cargo test --workspace --all-features
cargo test -F extras,alloc,derive --benches

echo "CI-CLEAN!"
