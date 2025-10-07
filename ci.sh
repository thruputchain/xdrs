#!/bin/bash

set -x
set -e

cargo test --workspace
cargo test --workspace --all-features
cargo test -F extras,alloc,derive --benches

cargo test -F int128_xdr >& /dev/null && {
    echo "Feature \"int128_xdr\" incorrectly allowed without feature \"extra\""
    exit 1
}

set +x
echo "CI-CLEAN!"
