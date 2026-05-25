#!/bin/sh
set -eu

reads="${1:?missing reads}"
outdir="${2:?missing outdir}"
output_name="seqtk-comp.tsv"
out="$outdir/$output_name"

json_escape() {
  printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'
}

mkdir -p "$outdir"
if ! command -v seqtk >/dev/null 2>&1; then
  printf 'seqtk executable was not found on PATH\n' >&2
  exit 127
fi
seqtk comp "$reads" > "$out"
stats=$(awk 'BEGIN { FS = "\t" } NF >= 2 { records++; bases += $2 + 0 } END { printf "%d\t%d", records + 0, bases + 0 }' "$out")
records=${stats%%	*}
total_bases=${stats#*	}
bytes=$(wc -c < "$out" | tr -d ' ')
printf '{"summary":{"reads":"%s","records":%s,"totalBases":%s,"bytes":%s,"output":"%s"}}\n' \
  "$(json_escape "$reads")" "$records" "$total_bases" "$bytes" "$(json_escape "$output_name")" > "$outdir/outputs.json"
printf 'seqtk comp wrote %s (%s records)\n' "$out" "$records"
