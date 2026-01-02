#!/bin/bash

test_xdr="${1:?}"; shift

verbose="${1}"; shift

if [ -z "$verbose" ]; then
    quiet="-q"
else
    quiet=""
fi

parse_file() {
    path="${1:?}"

    cargo $quiet run -Fns --bin parse -- "$path"
}

strip_blanks() {
    sed '/^[[:space:]]*$/d'
}

strip_comments() {
    sed 's|//.*||'
}

strip_int_separators() {
    perl -pe $'s/(\\d)\'/\\1/g'
}

exec diff $quiet -w \
    <(cat "$test_xdr" | strip_int_separators | strip_comments | strip_blanks) \
    <(parse_file "$test_xdr" | strip_blanks)
