#!/usr/bin/env bash
# Keep permissive decoding a separate run.
set -euo pipefail
cd "$(dirname "$0")/.."
coverage_toolchain="${XDRS_COVERAGE_TOOLCHAIN:-nightly-2026-05-26}"
# --locked: build exactly the committed Cargo.lock.
cov() { cargo +"$coverage_toolchain" llvm-cov "$@" --locked; }
cov clean --workspace
cov --no-report --workspace --lib --tests \
    --features alloc,derive,repr,ns,int128,heapless,serde,schemars
cov --no-report --lib --tests --features derive,int128
cov --no-report --lib --tests --no-default-features --features derive,int128
cov --no-report --features permissive,int128 --test ints
cov report -p thruput-xdrs -p thruput-xdr-common -p thruput-xdr-derive -p thruput-xdr-repr --html
cov report -p thruput-xdrs -p thruput-xdr-common -p thruput-xdr-derive -p thruput-xdr-repr --lcov --output-path target/llvm-cov/lcov.info
cov report -p thruput-xdrs -p thruput-xdr-common -p thruput-xdr-derive -p thruput-xdr-repr
