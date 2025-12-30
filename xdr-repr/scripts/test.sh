#!/bin/bash

set -e

test_example() {
    input="example-data/${1:?}"

    if bash scripts/test_parse.sh "$input";
    then
        echo "Parse OK: $input"
    else
        echo "Bad parse: $input"
        return 1
    fi
}

test_example types.x
