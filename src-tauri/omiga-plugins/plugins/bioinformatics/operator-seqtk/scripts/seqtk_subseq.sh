#!/bin/sh
set -eu

reads="${1:?missing reads}"
outdir="${2:?missing outdir}"
id_list="${3:?missing id_list}"
output_name="seqtk-subseq.fastq"
out="$outdir/$output_name"

json_escape() {
  printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'
}

mkdir -p "$outdir"
if ! command -v seqtk >/dev/null 2>&1; then
  printf 'seqtk executable was not found on PATH\n' >&2
  exit 127
fi
seqtk subseq "$reads" "$id_list" > "$out"
requested=$(awk 'NF > 0 { count++ } END { printf "%d", count + 0 }' "$id_list")
records=$(awk 'NR == 1 { first = substr($0, 1, 1) } first == ">" && /^>/ { count++ } END { if (first == "@") printf "%d", int(NR / 4); else printf "%d", count + 0 }' "$out")
bytes=$(wc -c < "$out" | tr -d ' ')
printf '{"summary":{"reads":"%s","idList":"%s","idsRequested":%s,"records":%s,"bytes":%s,"output":"%s"}}\n' \
  "$(json_escape "$reads")" "$(json_escape "$id_list")" "$requested" "$records" "$bytes" "$(json_escape "$output_name")" > "$outdir/outputs.json"
printf 'seqtk subseq wrote %s (%s records)\n' "$out" "$records"
