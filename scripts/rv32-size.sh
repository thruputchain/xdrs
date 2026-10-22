#!/usr/bin/env bash
# Builds rv32-size-test the way thruput-sdk-rs builds contracts, from its
# committed Cargo.lock, and prints the ELF's on-chain-relevant section sizes
# (the same fields as thruput-sdk-rs's sdk_codesizes_rv32.sh).
set -euo pipefail
cd "$(dirname "$0")/../rv32-size-test"

# A RUSTFLAGS variable would replace the target rustflags in
# rv32-size-test/.cargo/config.toml, which are the point of this build.
unset RUSTFLAGS

# The toolchain rv32-size-test/rust-toolchain.toml pins.
rustup toolchain install --no-self-update
cargo build --locked --release --target riscv32im-unknown-none-elf \
    -Zbuild-std=core,alloc -Zbuild-std-features=std/optimize_for_size

elf=target/riscv32im-unknown-none-elf/release/rv32-size-test
readelf="$(command -v llvm-readelf || command -v readelf)"
section_size() {
  local hex
  hex=$("$readelf" -SW "$elf" | sed 's/^ *\[ *[0-9]*\] *//' | awk -v s="$1" '$1 == s { print $5; exit }')
  echo $((16#${hex:-0}))
}
text=$(section_size .text)
rodata=$(section_size .rodata)
data=$(section_size .data)
bss=$(section_size .bss)
echo "rv32-size-test text=$text rodata=$rodata data=$data bss=$bss data_image=$((rodata + data)) file=$(wc -c < "$elf")"
