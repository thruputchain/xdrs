#!/bin/bash

export RUSTFLAGS="--deny warnings"
export RUSTDOCFLAGS="--deny warnings"
set -e
set -x

readonly XDRS_NIGHTLY_TOOLCHAIN="nightly-2026-05-26"
rustup toolchain install \
    "${XDRS_NIGHTLY_TOOLCHAIN}" \
    --profile minimal \
    --component rust-src \
    --no-self-update

# check the root library without alloc
cargo test --locked -F int128

# run the same malformed-input corpus with compact ReadError diagnostics
cargo test --locked --no-default-features
cargo test --locked --no-default-features --features derive --test read_errors

# check derive macros without alloc (const writers for VecM/BytesM/StringM
# are only exercised with non-empty static data in no-alloc builds)
cargo test --locked -F derive,int128

# check feature "permissive"
cargo test --locked -F permissive,int128 --test ints

# check feature "int128"
cargo test --locked -F int128 --test ints

# check everything else
cargo test --locked --workspace --all-features

# Test on xdr-common with varying features
cargo test --locked -p xdr-common --no-default-features
cargo test --locked -p xdr-common --features alloc
cargo test --locked -p xdr-common --features serde
cargo test --locked -p xdr-common --features schemars

# Compile Serde support for a target with no standard library.
rustup target add thumbv7em-none-eabi
cargo check --locked -p xdr-common --no-default-features --features serde --target thumbv7em-none-eabi

cargo +"${XDRS_NIGHTLY_TOOLCHAIN}" build --locked -p xdr-common --no-default-features -Zbuild-std=core,alloc
cargo +"${XDRS_NIGHTLY_TOOLCHAIN}" build --locked -p xdr-common --features alloc -Zbuild-std=core,alloc

# check that the benchmarks compile
cargo test --locked --benches --all-features

# check that the docs build cleanly
cargo doc --locked --workspace --all-features

# docsrs docs
RUSTDOCFLAGS="--cfg docsrs" \
    cargo +"${XDRS_NIGHTLY_TOOLCHAIN}" doc --locked --workspace --all-features --no-deps

# Test Documentation Examples
cargo test --locked --doc -p xdr-repr --features "ns,int128"
cargo test --locked --doc --features "alloc,derive,repr,ns,int128"

cargo clippy --locked --workspace --all-features

# RPC crates (rpc/) deny clippy warnings outright
cargo clippy --all-targets --locked \
    -p rpc_api -p rpc_driver -p rpc-smoke -p rpc-api-fixture -p bench-compare \
    -- -D warnings

# xdrs linked into an rv32 binary with the thruput-sdk-rs contract config
bash scripts/rv32-size.sh

set +x

cargo fmt --check --quiet || {
  echo "Needs formatting!"
  exit 1
}

rg -i --glob '!/ci.sh' XXX && {
  echo "XXX found!"
  exit 1
}

echo "CI-CLEAN!"
