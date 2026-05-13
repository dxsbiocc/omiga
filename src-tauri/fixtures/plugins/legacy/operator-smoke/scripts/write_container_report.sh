#!/bin/sh
set -eu

outdir="${1:?missing outdir}"
message="${2:-hello from Omiga container operator}"
repeat="${3:-1}"

case "$repeat" in
  ''|*[!0-9]*) repeat=1 ;;
esac
if [ "$repeat" -lt 1 ]; then repeat=1; fi
if [ "$repeat" -gt 20 ]; then repeat=20; fi

mkdir -p "$outdir"
report="$outdir/container-operator-report.txt"
: > "$report"

i=0
while [ "$i" -lt "$repeat" ]; do
  printf '%s\n' "$message" >> "$report"
  i=$((i + 1))
done

printf 'container smoke runtime: %s\n' "$(uname -s 2>/dev/null || printf unknown)" >> "$report"
printf 'wrote %s line(s) plus runtime marker to %s\n' "$repeat" "$report"
