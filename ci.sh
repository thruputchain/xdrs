#!/bin/bash

set -x
set -e

cargo test --workspace
cargo test --workspace --all-features
cargo test -F extras,alloc,derive --benches

echo "CI-CLEAN!"
