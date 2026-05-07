#!/bin/sh
set -eu

reads="${1:?missing reads}"
outdir="${2:?missing outdir}"
sample_size="${3:-0.1}"
seed="${4:-11}"
output_name="${5:-seqtk-sampled.fastq}"

mkdir -p "$outdir"
case "$output_name" in
  */*|''|.*) output_name="seqtk-sampled.fastq" ;;
esac
out="$outdir/$output_name"
if ! command -v seqtk >/dev/null 2>&1; then
  printf 'seqtk executable was not found on PATH\n' >&2
  exit 127
fi
case "$seed" in ''|*[!0-9]*) seed=11 ;; esac
seqtk sample -s "$seed" "$reads" "$sample_size" > "$out"
records=$(awk 'END { if (NR > 0) printf "%d", int(NR / 4); else printf "0" }' "$out" 2>/dev/null || printf '0')
bytes=$(wc -c < "$out" | tr -d ' ')
printf '{"summary":{"reads":"%s","sampleSize":"%s","seed":%s,"records":%s,"bytes":%s,"output":"%s"}}\n' \
  "$(printf '%s' "$reads" | sed 's/\\/\\\\/g; s/"/\\"/g')" \
  "$(printf '%s' "$sample_size" | sed 's/\\/\\\\/g; s/"/\\"/g')" \
  "$seed" "$records" "$bytes" \
  "$(printf '%s' "$output_name" | sed 's/\\/\\\\/g; s/"/\\"/g')" > "$outdir/outputs.json"
printf 'seqtk sample wrote %s (%s bytes)\n' "$out" "$bytes"
