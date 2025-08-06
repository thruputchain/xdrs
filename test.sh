#!/bin/bash

set -x  # print commands
set -e  # fail on error

cargo test --workspace
cargo test -F extras
cargo test -F extras,alloc
cargo test -F extras,alloc --benches
cargo test -F xdr_common
cargo test -F xdr_common,alloc
