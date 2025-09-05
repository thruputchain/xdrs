#!/bin/bash

if [[ -z $XDRC ]]; then
  printf "usage: XDRC=<x23c> %s\n" "${0}" >&2
  exit 1
fi

echo "-- Using XDRC=${XDRC} --" >&2

$XDRC --rust -o "rpc_driver/src/rpc_msg.rs" "xdr/rpc_msg.x"
$XDRC --rust -o "rpc_driver/tests/echo.rs" "xdr/echo.x"


