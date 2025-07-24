#!/bin/bash

set -x  # print commands
set -e  # fail on error

cargo test -F extras
cargo test -F extras,alloc
# cargo test -F alloc --benches
cargo test -F xdr_common
cargo test -F xdr_common,alloc
