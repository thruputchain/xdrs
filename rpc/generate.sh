#!/bin/bash

if [[ -z $XDRC ]]; then
  printf "usage: XDRC=<x23c> %s\n" "${0}" >&2
  exit 1
fi

echo "-- Using XDRC=${XDRC} --" >&2

$XDRC --rust \
    --rust-feature xdrs \
    --rust-no-feature std \
    --rust-no-feature serde \
    --rust-no-feature spec \
    --rust-no-feature schema \
    -o "rpc_driver/src/rpc_msg.rs" \
    "xdr/rpc_msg.x"
