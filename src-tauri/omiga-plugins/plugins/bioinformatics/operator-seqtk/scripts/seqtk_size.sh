#!/bin/sh
set -eu

reads="${1:?missing reads}"
outdir="${2:?missing outdir}"
raw="$outdir/seqtk-size.txt"

json_escape() {
  printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'
}

mkdir -p "$outdir"
if ! command -v seqtk >/dev/null 2>&1; then
  printf 'seqtk executable was not found on PATH\n' >&2
  exit 127
fi
seqtk size "$reads" > "$raw"
counts=$(awk 'NR == 1 { printf "%s\t%s", $1, $2; exit }' "$raw")
total_reads=${counts%%	*}
total_bases=${counts#*	}
case "$total_reads" in ''|*[!0-9]*) total_reads=0 ;; esac
case "$total_bases" in ''|*[!0-9]*) total_bases=0 ;; esac
printf '{"summary":{"reads":"%s","totalReads":%s,"totalBases":%s}}\n' \
  "$(json_escape "$reads")" "$total_reads" "$total_bases" > "$outdir/outputs.json"
printf 'seqtk size wrote %s reads and %s bases\n' "$total_reads" "$total_bases"
