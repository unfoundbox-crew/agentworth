#!/usr/bin/env bash
# A stand-in for `gh api`, for apps/cli/tests/action_e2e.rs. Each route is a file under
# $FAKE_GH_DIR/routes named after "<METHOD> <path>" with every character outside
# [A-Za-z0-9._-] replaced by "_". "<key>" holds the response body; "<key>.fail" holds the
# stderr of a failed call. Every call is logged; every --input body is kept as posted-N.json.
set -u
[ "${1:-}" = api ] || { echo "fake gh: only 'gh api' is supported" >&2; exit 2; }
shift
method=GET input="" path=""
while [ $# -gt 0 ]; do
  case "$1" in
    -X | --method) method="$2"; shift 2 ;;
    --input) input="$2"; shift 2 ;;
    --paginate) shift ;;
    -*) echo "fake gh: unsupported flag $1" >&2; exit 2 ;;
    *) path="$1"; shift ;;
  esac
done
echo "$method $path" >>"$FAKE_GH_DIR/calls.log"
if [ -n "$input" ]; then
  n=$(find "$FAKE_GH_DIR" -maxdepth 1 -name 'posted-*.json' | wc -l | tr -d ' ')
  cp "$input" "$FAKE_GH_DIR/posted-$((n + 1)).json"
fi
key="$(printf '%s %s' "$method" "$path" | tr -c 'A-Za-z0-9._-' '_')"
if [ -f "$FAKE_GH_DIR/routes/$key.fail" ]; then
  cat "$FAKE_GH_DIR/routes/$key.fail" >&2
  exit 1
fi
if [ -f "$FAKE_GH_DIR/routes/$key" ]; then
  cat "$FAKE_GH_DIR/routes/$key"
  exit 0
fi
echo "gh: Not Found (HTTP 404) $method $path" >&2
exit 1
