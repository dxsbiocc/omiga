#!/usr/bin/env python3
"""UniProtKB search Operator pilot with deterministic offline fixtures."""

from __future__ import annotations

import csv
import json
import sys
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Any

BASE_URL = "https://rest.uniprot.org"


def fail(message: str, outdir: Path | None = None) -> int:
    if outdir is not None:
        outdir.mkdir(parents=True, exist_ok=True)
        (outdir / "outputs.json").write_text(
            json.dumps({"status": "failed", "error": message}, ensure_ascii=False) + "\n"
        )
    print(message, file=sys.stderr)
    return 2


def read_fixture(path: str) -> tuple[list[dict[str, Any]], int | None]:
    raw = json.loads(Path(path).read_text())
    if isinstance(raw, dict) and isinstance(raw.get("records"), list):
        return [normalize_record(item) for item in raw["records"]], len(raw["records"])
    if isinstance(raw, dict) and isinstance(raw.get("results"), list):
        total = raw.get("total") if isinstance(raw.get("total"), int) else len(raw["results"])
        return [normalize_record(item) for item in raw["results"]], total
    if isinstance(raw, list):
        return [normalize_record(item) for item in raw], len(raw)
    return [], 0


def live_search(
    query: str,
    limit: int,
    organism: str,
    taxon_id: str,
    reviewed: str,
) -> tuple[list[dict[str, Any]], int | None]:
    effective_query = build_query(query, organism, taxon_id, reviewed)
    params = {
        "query": effective_query,
        "format": "json",
        "size": str(limit),
    }
    url = f"{BASE_URL}/uniprotkb/search?{urllib.parse.urlencode(params)}"
    request = urllib.request.Request(url, headers={"User-Agent": "Omiga-UniProt-Operator/0.1"})
    with urllib.request.urlopen(request, timeout=20) as response:
        total = response.headers.get("x-total-results")
        doc = json.loads(response.read().decode("utf-8"))
    total_count = int(total) if total and total.isdigit() else None
    records = [normalize_record(item) for item in doc.get("results", [])]
    return records, total_count


def build_query(query: str, organism: str, taxon_id: str, reviewed: str) -> str:
    parts = [query.strip()]
    if taxon_id.strip():
        parts.append(f"(organism_id:{taxon_id.strip()})")
    elif organism.strip():
        parts.append(f"(organism_name:{organism.strip()})")
    if reviewed.strip().lower() in {"true", "false"}:
        parts.append(f"(reviewed:{reviewed.strip().lower()})")
    return " AND ".join(part for part in parts if part)


def normalize_record(item: dict[str, Any]) -> dict[str, Any]:
    accession = str(item.get("primaryAccession") or item.get("accession") or "").strip()
    entry_name = str(item.get("uniProtkbId") or item.get("entryName") or "").strip()
    entry_type = str(item.get("entryType") or "").strip()
    organism = item.get("organism") if isinstance(item.get("organism"), dict) else {}
    sequence = item.get("sequence") if isinstance(item.get("sequence"), dict) else {}
    return {
        "accession": accession,
        "entryName": entry_name,
        "reviewed": "reviewed" in entry_type.lower() and "unreviewed" not in entry_type.lower(),
        "proteinName": protein_name(item),
        "geneNames": "; ".join(gene_names(item)),
        "organism": str(organism.get("scientificName") or "").strip(),
        "commonOrganism": str(organism.get("commonName") or "").strip(),
        "taxonId": organism.get("taxonId") or "",
        "length": sequence.get("length") or "",
        "mass": sequence.get("molWeight") or "",
        "function": function_comment(item),
        "url": f"https://www.uniprot.org/uniprotkb/{accession}/entry" if accession else "",
    }


def protein_name(item: dict[str, Any]) -> str:
    desc = item.get("proteinDescription")
    if not isinstance(desc, dict):
        return str(item.get("proteinName") or "").strip()
    recommended = desc.get("recommendedName")
    if isinstance(recommended, dict):
        full = recommended.get("fullName")
        if isinstance(full, dict) and full.get("value"):
            return str(full["value"]).strip()
    submission = desc.get("submissionNames")
    if isinstance(submission, list):
        for item in submission:
            if isinstance(item, dict):
                full = item.get("fullName")
                if isinstance(full, dict) and full.get("value"):
                    return str(full["value"]).strip()
    return ""


def gene_names(item: dict[str, Any]) -> list[str]:
    out: list[str] = []
    genes = item.get("genes")
    if not isinstance(genes, list):
        return out
    for gene in genes:
        if not isinstance(gene, dict):
            continue
        for key in ["geneName", "orderedLocusNames", "orfNames", "synonyms"]:
            value = gene.get(key)
            values = value if isinstance(value, list) else [value]
            for entry in values:
                if isinstance(entry, dict) and entry.get("value"):
                    name = str(entry["value"]).strip()
                    if name and name not in out:
                        out.append(name)
    return out


def function_comment(item: dict[str, Any]) -> str:
    comments = item.get("comments")
    if not isinstance(comments, list):
        return ""
    for comment in comments:
        if not isinstance(comment, dict):
            continue
        if str(comment.get("commentType") or "").upper() != "FUNCTION":
            continue
        texts = comment.get("texts")
        if isinstance(texts, list):
            values = [str(text.get("value") or "").strip() for text in texts if isinstance(text, dict)]
            return " ".join(value for value in values if value)
    return ""


def write_outputs(
    outdir: Path,
    query: str,
    effective_query: str,
    mode: str,
    records: list[dict[str, Any]],
    total: int | None,
) -> None:
    outdir.mkdir(parents=True, exist_ok=True)
    results_path = outdir / "uniprot-results.tsv"
    fields = [
        "accession",
        "entryName",
        "reviewed",
        "proteinName",
        "geneNames",
        "organism",
        "taxonId",
        "length",
        "mass",
        "url",
        "function",
    ]
    with results_path.open("w", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fields, delimiter="\t")
        writer.writeheader()
        for record in records:
            writer.writerow({field: record.get(field, "") for field in fields})
    summary = {
        "status": "succeeded",
        "query": query,
        "effectiveQuery": effective_query,
        "mode": mode,
        "count": len(records),
        "total": total,
        "results": "uniprot-results.tsv",
        "accessions": [record.get("accession", "") for record in records if record.get("accession")],
    }
    (outdir / "outputs.json").write_text(
        json.dumps({"summary": summary}, ensure_ascii=False, indent=2) + "\n"
    )
    print(f"UniProt search complete: {len(records)} records ({mode})")


def main(argv: list[str]) -> int:
    if len(argv) < 9:
        return fail(
            "usage: uniprot_search.py OUTDIR QUERY LIMIT MODE FIXTURE_JSON ORGANISM TAXON_ID REVIEWED"
        )
    outdir = Path(argv[1])
    query = argv[2].strip()
    try:
        limit = max(1, min(25, int(argv[3])))
    except ValueError:
        return fail("limit must be an integer", outdir)
    mode = (argv[4] or "auto").strip().lower()
    fixture_json = argv[5].strip()
    organism = argv[6].strip()
    taxon_id = argv[7].strip()
    reviewed = (argv[8] or "any").strip().lower()
    if not query:
        return fail("query must not be empty", outdir)
    if reviewed not in {"any", "true", "false"}:
        return fail("reviewed must be one of any, true, false", outdir)
    try:
        effective_query = build_query(query, organism, taxon_id, reviewed)
        if mode == "offline_fixture" or (mode == "auto" and fixture_json):
            if not fixture_json:
                return fail("offline_fixture mode requires fixture_json", outdir)
            records, total = read_fixture(fixture_json)
            records = records[:limit]
            actual_mode = "offline_fixture"
        elif mode in {"auto", "live"}:
            records, total = live_search(query, limit, organism, taxon_id, reviewed)
            actual_mode = "live"
        else:
            return fail(f"unsupported mode: {mode}", outdir)
        write_outputs(outdir, query, effective_query, actual_mode, records, total)
        return 0
    except Exception as exc:  # noqa: BLE001 - operator should surface structured failure
        return fail(f"uniprot search failed: {exc}", outdir)


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
