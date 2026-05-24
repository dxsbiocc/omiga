#!/bin/sh
set -eu

reads="${1:?missing reads}"
outdir="${2:?missing outdir}"
qual_threshold="${3:-0}"
raw_name="seqtk-fqchk.txt"
raw="$outdir/$raw_name"

json_escape() {
  printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'
}

mkdir -p "$outdir"
if ! command -v seqtk >/dev/null 2>&1; then
  printf 'seqtk executable was not found on PATH\n' >&2
  exit 127
fi
case "$qual_threshold" in ''|*[!0-9]*) qual_threshold=0 ;; esac
set -- fqchk
if [ "$qual_threshold" -gt 0 ]; then
  set -- "$@" -q "$qual_threshold"
fi
set -- "$@" "$reads"
seqtk "$@" > "$raw"
awk -v reads="$reads" -v qual="$qual_threshold" -v raw_name="$raw_name" '
function esc(s) { gsub(/\\/, "\\\\", s); gsub(/"/, "\\\"", s); return s }
function value(s) {
  if (s ~ /^-?[0-9]+([.][0-9]+)?([eE][-+]?[0-9]+)?$/) return s
  return "\"" esc(s) "\""
}
BEGIN {
  FS = "[ \t]+"
  printf "{\"summary\":{\"reads\":\"%s\",\"qualThreshold\":%s,\"rawOutput\":\"%s\",\"columns\":[", esc(reads), qual, esc(raw_name)
}
NF > 0 && !header_seen && (tolower($1) == "pos" || $1 == "POS") {
  header_seen = 1
  cols = NF
  for (i = 1; i <= cols; i++) {
    if (i > 1) printf ","
    col[i] = $i
    printf "\"%s\"", esc($i)
  }
  printf "],\"rows\":["
  next
}
header_seen && NF > 0 {
  if (rows > 0) printf ","
  printf "{"
  for (i = 1; i <= cols; i++) {
    if (i > 1) printf ","
    printf "\"%s\":%s", esc(col[i]), value($i)
  }
  printf "}"
  rows++
}
END {
  if (!header_seen) printf "],\"rows\":["
  printf "],\"rowCount\":%d}}\n", rows + 0
}
' "$raw" > "$outdir/outputs.json"
printf 'seqtk fqchk wrote %s\n' "$raw"
