#!/bin/sh
set -eu

output=$(svelte-check --tsconfig ./tsconfig.json --output machine 2>&1 || true)
count=$(printf '%s\n' "$output" | awk '/COMPLETED/ {print $5}')
baseline=$(cat .svelte-check-baseline)

case "$count" in
  ''|*[!0-9]*)
    printf '%s\n' "$output"
    echo "svelte-check did not report a valid error count" >&2
    exit 1
    ;;
esac

echo "svelte-check errors: $count (baseline $baseline)"
test "$count" -le "$baseline"
