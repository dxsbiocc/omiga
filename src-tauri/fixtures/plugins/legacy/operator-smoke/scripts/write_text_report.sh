#!/bin/sh
set -eu

outdir="${1:?missing outdir}"
message="${2:-hello from Omiga operator}"
repeat="${3:-1}"

case "$repeat" in
  ''|*[!0-9]*) repeat=1 ;;
esac
if [ "$repeat" -lt 1 ]; then repeat=1; fi
if [ "$repeat" -gt 20 ]; then repeat=20; fi

mkdir -p "$outdir"
: > "$outdir/operator-report.txt"
i=0
while [ "$i" -lt "$repeat" ]; do
  printf '%s\n' "$message" >> "$outdir/operator-report.txt"
  i=$((i + 1))
done

printf 'wrote %s line(s) to %s\n' "$repeat" "$outdir/operator-report.txt"
