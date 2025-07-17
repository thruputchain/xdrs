#!/bin/bash

set -x  # print commands
set -e  # fail on error

cargo test -F extras
cargo test -F extras,alloc
