#!/bin/bash

test_xdr="${1:?}"

parse_file() {
    path="${1:?}"

    cargo -q run --bin parse -- "$path"
}

strip_blanks() {
    sed '/^[[:space:]]*$/d'
}

strip_comments() {
    sed 's|//.*||'
}

exec diff -q -w \
    <(cat "$test_xdr" | strip_comments | strip_blanks) \
    <(parse_file "$test_xdr" | strip_blanks)
