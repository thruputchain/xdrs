#!/usr/bin/env bash
# Usage: crates-io-has.sh <crate> <version>
# Exits 0 when crates.io lists <crate>@<version> (yanked or not), 1 when it
# doesn't, and 2 when the index can't be read — so a release never mistakes
# an outage for "not published".
set -euo pipefail
name="$1"
version="$2"
lower="${name,,}"
case ${#lower} in
  1) path="1/$lower" ;;
  2) path="2/$lower" ;;
  3) path="3/${lower:0:1}/$lower" ;;
  *) path="${lower:0:2}/${lower:2:2}/$lower" ;;
esac
entries=$(mktemp)
trap 'rm -f "$entries"' EXIT
http_code=$(curl -sS -o "$entries" -w '%{http_code}' "https://index.crates.io/$path")
case "$http_code" in
  # Not grep -q: exiting at the first match can fail jq with SIGPIPE,
  # which pipefail would report as "not published".
  200) jq -r '.vers' "$entries" | grep -xF "$version" >/dev/null ;;
  # 404 means the crate has no published versions at all.
  404) exit 1 ;;
  *)
    echo "::error::crates.io index probe for '$name' failed (HTTP $http_code); cannot tell whether $name@$version is published" >&2
    exit 2
    ;;
esac
