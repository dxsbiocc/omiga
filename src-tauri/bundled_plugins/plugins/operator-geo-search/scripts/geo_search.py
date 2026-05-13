#!/usr/bin/env python3
"""NCBI GEO search Operator pilot with deterministic offline fixtures."""

from __future__ import annotations

import csv
import json
import os
import re
import sys
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Any

EUTILS = "https://eutils.ncbi.nlm.nih.gov/entrez/eutils"


def fail(message: str, outdir: Path | None = None) -> int:
    if outdir is not None:
        outdir.mkdir(parents=True, exist_ok=True)
        (outdir / "outputs.json").write_text(
            json.dumps({"status": "failed", "error": message}, ensure_ascii=False) + "\n"
        )
    print(message, file=sys.stderr)
    return 2


def read_fixture(path: str) -> list[dict[str, Any]]:
    raw = json.loads(Path(path).read_text())
    if isinstance(raw, dict) and isinstance(raw.get("records"), list):
        return [normalize_record(item) for item in raw["records"]]
    result = raw.get("result", {}) if isinstance(raw, dict) else {}
    uids = result.get("uids", []) if isinstance(result, dict) else []
    records = []
    for uid in uids:
        item = result.get(str(uid), {})
        if isinstance(item, dict):
            records.append(normalize_record(item | {"uid": str(uid)}))
    return records


def live_search(query: str, limit: int, email: str, api_key: str) -> list[dict[str, Any]]:
    common = {"db": "gds", "retmode": "json", "tool": "omiga_geo_operator"}
    if email:
        common["email"] = email
    if api_key:
        common["api_key"] = api_key
    search_params = common | {"term": query, "retmax": str(limit), "sort": "relevance"}
    search_url = f"{EUTILS}/esearch.fcgi?{urllib.parse.urlencode(search_params)}"
    with urllib.request.urlopen(search_url, timeout=20) as response:
        search_doc = json.loads(response.read().decode("utf-8"))
    ids = search_doc.get("esearchresult", {}).get("idlist", [])[:limit]
    if not ids:
        return []
    summary_params = common | {"id": ",".join(ids)}
    summary_url = f"{EUTILS}/esummary.fcgi?{urllib.parse.urlencode(summary_params)}"
    with urllib.request.urlopen(summary_url, timeout=20) as response:
        summary_doc = json.loads(response.read().decode("utf-8"))
    result = summary_doc.get("result", {})
    return [normalize_record(result.get(str(uid), {}) | {"uid": str(uid)}) for uid in ids]


def normalize_record(item: dict[str, Any]) -> dict[str, Any]:
    uid = str(item.get("uid") or item.get("id") or "").strip()
    accession = str(item.get("accession") or item.get("Accession") or "").strip()
    if not accession:
        accession = accession_from_title(str(item.get("title") or ""))
    url = str(item.get("url") or "").strip()
    if not url:
        url = (
            f"https://www.ncbi.nlm.nih.gov/geo/query/acc.cgi?acc={accession}"
            if accession
            else f"https://www.ncbi.nlm.nih.gov/gds/?term={uid}"
        )
    return {
        "uid": uid,
        "accession": accession,
        "title": strip_html(str(item.get("title") or "")).strip(),
        "summary": strip_html(str(item.get("summary") or "")).strip(),
        "gdsType": str(item.get("gdsType") or item.get("gdstype") or "").strip(),
        "taxon": str(item.get("taxon") or item.get("taxonname") or "").strip(),
        "sampleCount": integer_or_blank(item.get("n_samples") or item.get("samples")),
        "platform": str(item.get("platform") or item.get("GPL") or item.get("gpl") or "").strip(),
        "publishedAt": str(item.get("pdat") or item.get("PDAT") or item.get("pubDate") or "").strip(),
        "url": url,
    }


def strip_html(value: str) -> str:
    return re.sub(r"<[^>]+>", "", value)


def accession_from_title(value: str) -> str:
    match = re.search(r"\bG(?:SE|SM|PL|DS)\d+\b", value, re.IGNORECASE)
    return match.group(0).upper() if match else ""


def integer_or_blank(value: Any) -> int | str:
    if value in (None, ""):
        return ""
    try:
        return int(value)
    except (TypeError, ValueError):
        return str(value)


def write_outputs(outdir: Path, query: str, mode: str, records: list[dict[str, Any]]) -> None:
    outdir.mkdir(parents=True, exist_ok=True)
    results_path = outdir / "geo-results.tsv"
    fields = [
        "uid",
        "accession",
        "title",
        "gdsType",
        "taxon",
        "sampleCount",
        "platform",
        "publishedAt",
        "url",
        "summary",
    ]
    with results_path.open("w", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fields, delimiter="\t")
        writer.writeheader()
        for record in records:
            writer.writerow({field: record.get(field, "") for field in fields})
    summary = {
        "status": "succeeded",
        "query": query,
        "mode": mode,
        "count": len(records),
        "results": "geo-results.tsv",
        "accessions": [record.get("accession", "") for record in records if record.get("accession")],
    }
    (outdir / "outputs.json").write_text(
        json.dumps({"summary": summary}, ensure_ascii=False, indent=2) + "\n"
    )
    print(f"GEO search complete: {len(records)} records ({mode})")


def main(argv: list[str]) -> int:
    if len(argv) < 7:
        return fail("usage: geo_search.py OUTDIR QUERY LIMIT MODE FIXTURE_JSON EMAIL")
    outdir = Path(argv[1])
    query = argv[2].strip()
    try:
        limit = max(1, min(100, int(argv[3])))
    except ValueError:
        return fail("limit must be an integer", outdir)
    mode = (argv[4] or "auto").strip().lower()
    fixture_json = argv[5].strip()
    email = argv[6].strip()
    api_key = os.environ.get("NCBI_API_KEY", "").strip()
    if not query:
        return fail("query must not be empty", outdir)
    try:
        if mode == "offline_fixture" or (mode == "auto" and fixture_json):
            if not fixture_json:
                return fail("offline_fixture mode requires fixture_json", outdir)
            records = read_fixture(fixture_json)[:limit]
            actual_mode = "offline_fixture"
        elif mode in {"auto", "live"}:
            records = live_search(query, limit, email, api_key)
            actual_mode = "live"
        else:
            return fail(f"unsupported mode: {mode}", outdir)
        write_outputs(outdir, query, actual_mode, records)
        return 0
    except Exception as exc:  # noqa: BLE001 - operator should surface structured failure
        return fail(f"geo search failed: {exc}", outdir)


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
