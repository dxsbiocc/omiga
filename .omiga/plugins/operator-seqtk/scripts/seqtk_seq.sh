#!/bin/sh
set -eu

reads="${1:?missing reads}"
outdir="${2:?missing outdir}"
output_format="${3:-fastq}"
reverse_complement="${4:-false}"
qual_threshold="${5:-0}"

json_escape() {
  printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'
}

mkdir -p "$outdir"
if ! command -v seqtk >/dev/null 2>&1; then
  printf 'seqtk executable was not found on PATH\n' >&2
  exit 127
fi
case "$output_format" in fasta|FASTA) output_format="fasta" ;; *) output_format="fastq" ;; esac
case "$qual_threshold" in ''|*[!0-9]*) qual_threshold=0 ;; esac
output_name="seqtk-seq.$output_format"
out="$outdir/$output_name"
set -- seq
if [ "$output_format" = "fasta" ]; then
  set -- "$@" -A
fi
case "$reverse_complement" in true|TRUE|1|yes|YES) set -- "$@" -r ;; esac
if [ "$qual_threshold" -gt 0 ]; then
  set -- "$@" -q "$qual_threshold"
fi
set -- "$@" "$reads"
seqtk "$@" > "$out"
reverse_json=false
case "$reverse_complement" in true|TRUE|1|yes|YES) reverse_json=true ;; esac
records=$(awk 'NR == 1 { first = substr($0, 1, 1) } first == ">" && /^>/ { count++ } END { if (first == "@") printf "%d", int(NR / 4); else printf "%d", count + 0 }' "$out")
bytes=$(wc -c < "$out" | tr -d ' ')
printf '{"summary":{"reads":"%s","outputFormat":"%s","reverseComplement":%s,"qualThreshold":%s,"records":%s,"bytes":%s,"output":"%s"}}\n' \
  "$(json_escape "$reads")" "$(json_escape "$output_format")" \
  "$reverse_json" "$qual_threshold" "$records" "$bytes" "$(json_escape "$output_name")" > "$outdir/outputs.json"
printf 'seqtk seq wrote %s (%s records)\n' "$out" "$records"
