#!/usr/bin/env python3
"""Public pathway retrieval plugin for Omiga's local JSONL protocol."""

from __future__ import annotations

import html
import json
import os
import re
import sys
import time
import urllib.parse
from pathlib import Path
from typing import Any, Dict, Iterable, List, Optional, Tuple

RESOURCE_UTILS = Path(__file__).resolve().parents[2] / "utils"
if RESOURCE_UTILS.is_dir() and str(RESOURCE_UTILS) not in sys.path:
    sys.path.insert(0, str(RESOURCE_UTILS))

from retrieval_http import fetch_json, fetch_json_with_headers, fetch_text, fetch_text_with_headers

PROTOCOL_VERSION = 1
SOURCES = [
    {"category": "knowledge", "id": "reactome", "capabilities": ["search", "query", "fetch"]},
    {"category": "knowledge", "id": "gene_ontology", "capabilities": ["search", "query", "fetch"]},
    {"category": "knowledge", "id": "msigdb", "capabilities": ["search", "query", "fetch"]},
    {"category": "knowledge", "id": "kegg", "capabilities": ["search", "query", "fetch"]},
]
PLUGIN_NAME = os.environ.get("OMIGA_RETRIEVAL_PLUGIN_NAME", "resource-pathways")
USER_AGENT = "Omiga pathway-sources retrieval plugin/0.1"

REACTOME_CONTENT = "https://reactome.org/ContentService"
REACTOME_ANALYSIS = "https://reactome.org/AnalysisService"
QUICKGO = "https://www.ebi.ac.uk/QuickGO/services"
MSIGDB = "https://www.gsea-msigdb.org/gsea/msigdb"
KEGG = "https://rest.kegg.jp"

FAVICONS = {
    "reactome": "https://reactome.org/favicon.ico",
    "gene_ontology": "https://www.ebi.ac.uk/QuickGO/favicon.ico",
    "msigdb": "https://www.gsea-msigdb.org/gsea/images/favicon.ico",
    "kegg": "https://www.kegg.jp/favicon.ico",
}
VALIDATION_IDS = {
    "reactome": "R-HSA-109581",
    "gene_ontology": "GO:0006915",
    "msigdb": "HALLMARK_APOPTOSIS",
    "kegg": "hsa04210",
}


def configured_source_ids() -> Optional[set[str]]:
    raw = os.environ.get("OMIGA_RETRIEVAL_SOURCE_IDS", "")
    values = {value.strip().lower().replace("-", "_") for value in raw.split(",") if value.strip()}
    return values or None


def configured_sources() -> List[Dict[str, Any]]:
    allowed = configured_source_ids()
    if allowed is None:
        return SOURCES
    return [source for source in SOURCES if source.get("id") in allowed]


def normalize_source(source: str) -> str:
    source = (source or "").strip().lower().replace("-", "_").replace(" ", "_")
    aliases = {
        "go": "gene_ontology",
        "quickgo": "gene_ontology",
        "geneontology": "gene_ontology",
        "molecular_signatures_database": "msigdb",
        "gsea_msigdb": "msigdb",
        "kegg_pathway": "kegg",
        "reactome_pathway": "reactome",
    }
    return aliases.get(source, source)


def source_is_allowed(source: str) -> bool:
    allowed = configured_source_ids()
    return allowed is None or source in allowed


def write(message: Dict[str, Any]) -> None:
    print(json.dumps(message, separators=(",", ":"), ensure_ascii=False), flush=True)


def error(message_id: str, code: str, message: str) -> Dict[str, Any]:
    return {"id": message_id, "type": "error", "error": {"code": code, "message": message}}


def request_params(request: Dict[str, Any]) -> Dict[str, Any]:
    params = request.get("params")
    return params if isinstance(params, dict) else {}


def max_results(request: Dict[str, Any], default: int = 5, ceiling: int = 25) -> int:
    params = request_params(request)
    value = request.get("maxResults") or request.get("max_results") or params.get("limit") or params.get("retmax") or params.get("rows") or default
    try:
        parsed = int(value)
    except (TypeError, ValueError):
        parsed = default
    return max(1, min(parsed, ceiling))


def is_validation(request: Dict[str, Any]) -> bool:
    return bool(request_params(request).get("omigaValidation"))


def str_param(request: Dict[str, Any], names: Iterable[str]) -> Optional[str]:
    params = request_params(request)
    for name in names:
        value = request.get(name) if name in request else params.get(name)
        if value is not None and str(value).strip():
            return str(value).strip()
    return None


def query_text(request: Dict[str, Any]) -> str:
    return str_param(request, ("query", "term", "q", "text", "name")) or ""


def identifier_text(request: Dict[str, Any]) -> str:
    for value in (str_param(request, ("id", "accession", "stable_id", "term_id", "pathway_id", "gene_set", "geneset", "uid")),):
        if value:
            return value
    result = request.get("result")
    if isinstance(result, dict):
        for key in ("id", "accession", "stable_id", "term_id", "pathway_id", "gene_set", "geneset", "url", "link"):
            value = result.get(key)
            if value is not None and str(value).strip():
                return str(value).strip()
        metadata = result.get("metadata")
        if isinstance(metadata, dict):
            for key in ("id", "accession", "st_id", "go_id", "kegg_id", "gene_set", "standard_name"):
                value = metadata.get(key)
                if value is not None and str(value).strip():
                    return str(value).strip()
    url = request.get("url")
    if isinstance(url, str) and url.strip():
        return url.strip()
    return ""


def urlopen_text(url: str, timeout: int = 25, accept: str = "*/*") -> Tuple[str, Dict[str, str]]:
    last_error: Optional[Exception] = None
    for attempt in range(3):
        try:
            return fetch_text_with_headers(url, timeout=timeout, user_agent=USER_AGENT, accept=accept)
        except Exception as exc:
            last_error = exc
            if attempt == 2:
                break
            time.sleep(0.4 * (attempt + 1))
    raise last_error or RuntimeError(f"request failed for {url}")


def urlopen_json(url: str, timeout: int = 25) -> Tuple[Any, Dict[str, str]]:
    last_error: Optional[Exception] = None
    for attempt in range(3):
        try:
            return fetch_json_with_headers(url, timeout=timeout, user_agent=USER_AGENT)
        except Exception as exc:
            last_error = exc
            if attempt == 2:
                break
            time.sleep(0.4 * (attempt + 1))
    raise last_error or RuntimeError(f"request failed for {url}")


def post_text_json(url: str, data: str, timeout: int = 30) -> Any:
    import urllib.request
    import urllib.error

    req = urllib.request.Request(
        url,
        data=data.encode("utf-8"),
        headers={"User-Agent": USER_AGENT, "Accept": "application/json", "Content-Type": "text/plain"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=timeout) as response:
            return json.loads(response.read().decode("utf-8", errors="replace"))
    except urllib.error.HTTPError as exc:
        body = exc.read().decode("utf-8", errors="replace")[:500]
        raise RuntimeError(f"HTTP {exc.code} from {url}: {body}") from exc


def base_response(
    request: Dict[str, Any],
    source: str,
    operation: str,
    *,
    items: Optional[List[Dict[str, Any]]] = None,
    detail: Optional[Dict[str, Any]] = None,
    total: Optional[int] = None,
    notes: Optional[List[str]] = None,
    raw: Optional[Dict[str, Any]] = None,
) -> Dict[str, Any]:
    response: Dict[str, Any] = {
        "ok": True,
        "operation": operation,
        "category": request.get("category", "knowledge"),
        "source": source,
        "effectiveSource": source,
        "items": items or [],
        "total": total,
        "notes": notes or [f"{PLUGIN_NAME} plugin"],
        "raw": raw or {"plugin": PLUGIN_NAME},
    }
    if detail is not None:
        response["detail"] = detail
    return response


def strip_markup(value: Any) -> str:
    text = str(value or "")
    text = re.sub(r"<br\s*/?>", " ", text, flags=re.I)
    text = re.sub(r"<[^>]+>", "", text)
    return html.unescape(re.sub(r"\s+", " ", text)).strip()


def list_from_value(value: Any) -> List[str]:
    if isinstance(value, list):
        out: List[str] = []
        for item in value:
            out.extend(list_from_value(item) if isinstance(item, (list, tuple)) else [str(item).strip()])
        return [item for item in out if item]
    if value is None:
        return []
    return [part.strip() for part in re.split(r"[\n,;\s]+", str(value)) if part.strip()]


def validation_item(source: str, operation: str) -> Dict[str, Any]:
    accession = VALIDATION_IDS.get(source, f"{source}-validation")
    return {
        "id": accession,
        "accession": accession,
        "title": f"Validation {source} {operation} result",
        "url": f"https://example.test/{source}/{urllib.parse.quote(accession)}",
        "favicon": FAVICONS.get(source),
        "snippet": "Offline validation result from pathway retrieval plugin.",
        "content": "This fixture response is returned only for Omiga validation smoke calls.",
        "metadata": {"source": source, "validation": True},
        "raw": {"validation": True},
    }


def validation_result(message_id: str, request: Dict[str, Any]) -> Dict[str, Any]:
    operation = request.get("operation", "query")
    source = normalize_source(str(request.get("source", "reactome")))
    response = base_response(request, source, operation, items=[validation_item(source, operation)], total=1, notes=["offline validation response"], raw={"validation": True})
    if operation == "fetch":
        response["detail"] = response["items"][0]
        response["items"] = []
    return {"id": message_id, "type": "result", "response": response}


# Reactome

def reactome_entry_item(record: Dict[str, Any]) -> Dict[str, Any]:
    st_id = str(record.get("stId") or record.get("id") or record.get("stableIdentifier", {}).get("identifier") or record.get("dbId") or "")
    title = strip_markup(record.get("displayName") or record.get("name") or st_id)
    schema = record.get("schemaClass") or record.get("type") or record.get("exactType")
    species_value = record.get("species")
    species: List[str] = []
    if isinstance(species_value, list):
        for item in species_value:
            if isinstance(item, dict):
                name = item.get("displayName") or item.get("name")
            else:
                name = item
            if name:
                species.append(strip_markup(name))
    elif species_value:
        species.append(strip_markup(species_value))
    summary = strip_markup(record.get("summation") or record.get("description") or "")
    if not summary and isinstance(record.get("summation"), list) and record["summation"]:
        first = record["summation"][0]
        if isinstance(first, dict):
            summary = strip_markup(first.get("text"))
    url = f"https://reactome.org/content/detail/{urllib.parse.quote(st_id)}" if st_id else "https://reactome.org/"
    return {
        "id": st_id,
        "accession": st_id,
        "title": title,
        "url": url,
        "favicon": FAVICONS["reactome"],
        "snippet": " · ".join(part for part in [schema, ", ".join(species), summary] if part)[:800],
        "content": json.dumps(record, ensure_ascii=False, indent=2)[:20000],
        "metadata": {"source": "reactome", "st_id": st_id, "schema_class": schema, "species": species, "summary": summary, "source_specific": record},
        "raw": record,
    }


def reactome_search(request: Dict[str, Any], term: str) -> Tuple[List[Dict[str, Any]], int, Dict[str, Any]]:
    params = request_params(request)
    species = str(params.get("species") or params.get("organism") or "Homo sapiens")
    rows = max_results(request)
    query = urllib.parse.urlencode({"query": term, "species": species, "types": "Pathway", "rows": str(rows)})
    data, _ = urlopen_json(f"{REACTOME_CONTENT}/search/query?{query}")
    entries: List[Dict[str, Any]] = []
    if isinstance(data, dict):
        for group in data.get("results", []) or []:
            if isinstance(group, dict):
                entries.extend(item for item in group.get("entries", []) or [] if isinstance(item, dict))
    return [reactome_entry_item(entry) for entry in entries[:rows]], len(entries), {"reactome": data}


def reactome_analysis_items(result: Dict[str, Any], limit: int) -> List[Dict[str, Any]]:
    out = []
    token = (((result.get("summary") or {}) if isinstance(result.get("summary"), dict) else {}).get("token"))
    for pathway in (result.get("pathways") or [])[:limit]:
        if not isinstance(pathway, dict):
            continue
        st_id = str(pathway.get("stId") or pathway.get("id") or "")
        name = strip_markup(pathway.get("name") or pathway.get("displayName") or st_id)
        entities = pathway.get("entities") if isinstance(pathway.get("entities"), dict) else {}
        p_value = entities.get("pValue")
        fdr = entities.get("fdr")
        url = f"https://reactome.org/PathwayBrowser/#{st_id}"
        if token:
            url += f"&DTAB=AN&ANALYSIS={urllib.parse.quote(str(token))}"
        out.append({
            "id": st_id,
            "accession": st_id,
            "title": name,
            "url": url,
            "favicon": FAVICONS["reactome"],
            "snippet": f"found {entities.get('found')}/{entities.get('total')} entities · p={p_value} · FDR={fdr}",
            "content": json.dumps(pathway, ensure_ascii=False, indent=2)[:12000],
            "metadata": {"source": "reactome", "st_id": st_id, "analysis_token": token, "p_value": p_value, "fdr": fdr, "entities": entities, "source_specific": pathway},
            "raw": pathway,
        })
    return out


def handle_reactome(message_id: str, request: Dict[str, Any]) -> Dict[str, Any]:
    operation = request.get("operation", "query")
    params = request_params(request)
    if operation in ("search", "query"):
        identifiers = list_from_value(params.get("identifiers") or params.get("genes") or params.get("gene_list"))
        term = query_text(request)
        if identifiers or params.get("analysis"):
            if not identifiers:
                identifiers = list_from_value(term)
            if not identifiers:
                return error(message_id, "missing_identifiers", "Reactome analysis requires identifiers/genes or query text containing identifiers")
            endpoint = "identifiers/projection/" if params.get("projection") else "identifiers/"
            raw = post_text_json(f"{REACTOME_ANALYSIS}/{endpoint}", "\n".join(identifiers))
            items = reactome_analysis_items(raw if isinstance(raw, dict) else {}, max_results(request, default=10, ceiling=50))
            response = base_response(request, "reactome", operation, items=items, total=len(items), notes=["Reactome AnalysisService overrepresentation analysis", "Analysis tokens are valid for 7 days."], raw={"reactome": raw})
            return {"id": message_id, "type": "result", "response": response}
        if not term:
            return error(message_id, "missing_query", "Reactome search/query requires query text or identifiers")
        if re.match(r"^R-[A-Z]{3}-\d+", term, re.I):
            request = {**request, "id": term}
            operation = "fetch"
        else:
            items, total, raw = reactome_search(request, term)
            response = base_response(request, "reactome", operation, items=items, total=total, raw=raw)
            return {"id": message_id, "type": "result", "response": response}
    if operation == "fetch":
        identifier = identifier_text(request)
        token = params.get("token")
        if token:
            raw, _ = urlopen_json(f"{REACTOME_ANALYSIS}/token/{urllib.parse.quote(str(token))}")
            items = reactome_analysis_items(raw if isinstance(raw, dict) else {}, max_results(request, default=10, ceiling=50))
            response = base_response(request, "reactome", operation, items=items, total=len(items), raw={"reactome": raw})
            return {"id": message_id, "type": "result", "response": response}
        identifier = normalize_reactome_id(identifier)
        if not identifier:
            return error(message_id, "missing_identifier", "Reactome fetch requires a stable ID, URL, result, or params.token")
        raw, _ = urlopen_json(f"{REACTOME_CONTENT}/data/query/{urllib.parse.quote(identifier)}")
        detail = reactome_entry_item(raw if isinstance(raw, dict) else {"stId": identifier, "raw": raw})
        response = base_response(request, "reactome", operation, detail=detail, total=1, raw={"reactome": raw})
        return {"id": message_id, "type": "result", "response": response}
    return error(message_id, "unsupported_operation", f"Reactome does not support operation {operation}")


def normalize_reactome_id(value: str) -> str:
    value = (value or "").strip().rstrip("/")
    if not value:
        return ""
    match = re.search(r"R-[A-Z]{3}-\d+", value, re.I)
    return match.group(0).upper() if match else value


# Gene Ontology / QuickGO

def go_term_item(record: Dict[str, Any]) -> Dict[str, Any]:
    go_id = str(record.get("id") or "")
    name = strip_markup(record.get("name") or go_id)
    definition = record.get("definition") if isinstance(record.get("definition"), dict) else {}
    definition_text = strip_markup(definition.get("text") or record.get("definition") or "")
    aspect = record.get("aspect") or record.get("ontology")
    url = f"https://www.ebi.ac.uk/QuickGO/term/{urllib.parse.quote(go_id)}" if go_id else "https://www.ebi.ac.uk/QuickGO/"
    return {
        "id": go_id,
        "accession": go_id,
        "title": name,
        "url": url,
        "favicon": FAVICONS["gene_ontology"],
        "snippet": " · ".join(part for part in [aspect, definition_text] if part)[:800],
        "content": json.dumps(record, ensure_ascii=False, indent=2)[:20000],
        "metadata": {"source": "gene_ontology", "go_id": go_id, "aspect": aspect, "definition": definition_text, "is_obsolete": record.get("isObsolete"), "source_specific": record},
        "raw": record,
    }


def handle_gene_ontology(message_id: str, request: Dict[str, Any]) -> Dict[str, Any]:
    operation = request.get("operation", "query")
    if operation in ("search", "query"):
        term = query_text(request)
        if not term:
            return error(message_id, "missing_query", "Gene Ontology search/query requires query text or GO ID")
        if re.match(r"^GO:\d{7}$", term.strip(), re.I):
            request = {**request, "id": term}
            operation = "fetch"
        else:
            params = urllib.parse.urlencode({"query": term, "limit": str(max_results(request)), "page": "1"})
            data, _ = urlopen_json(f"{QUICKGO}/ontology/go/search?{params}")
            records = data.get("results", []) if isinstance(data, dict) else []
            items = [go_term_item(record) for record in records if isinstance(record, dict)]
            total = int(data.get("numberOfHits", len(items))) if isinstance(data, dict) else len(items)
            response = base_response(request, "gene_ontology", operation, items=items, total=total, raw={"quickgo": data})
            return {"id": message_id, "type": "result", "response": response}
    if operation == "fetch":
        go_id = normalize_go_id(identifier_text(request))
        if not go_id:
            return error(message_id, "missing_identifier", "Gene Ontology fetch requires a GO:nnnnnnn ID, URL, or result")
        data, _ = urlopen_json(f"{QUICKGO}/ontology/go/terms/{urllib.parse.quote(go_id)}")
        records = data.get("results", []) if isinstance(data, dict) else []
        if not records:
            return error(message_id, "not_found", f"GO term not found: {go_id}")
        detail = go_term_item(records[0])
        response = base_response(request, "gene_ontology", operation, detail=detail, total=1, raw={"quickgo": data})
        return {"id": message_id, "type": "result", "response": response}
    return error(message_id, "unsupported_operation", f"Gene Ontology does not support operation {operation}")


def normalize_go_id(value: str) -> str:
    match = re.search(r"GO:\d{7}", value or "", re.I)
    return match.group(0).upper() if match else (value or "").strip()


# MSigDB

def msigdb_species(request: Dict[str, Any]) -> str:
    species = (str_param(request, ("species", "namespace")) or "human").strip().lower()
    return "mouse" if species in {"mouse", "mmu", "mus_musculus", "mus musculus"} else "human"


def normalize_msigdb_name(value: str) -> str:
    value = (value or "").strip().rstrip("/")
    if value.startswith("http://") or value.startswith("https://"):
        path = urllib.parse.urlparse(value).path.rstrip("/")
        value = path.rsplit("/", 1)[-1]
    value = re.sub(r"\.(html|json)$", "", value, flags=re.I)
    return urllib.parse.unquote(value).strip().upper()


def msigdb_set_item(name: str, species: str, record: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    url = f"{MSIGDB}/{species}/geneset/{urllib.parse.quote(name)}.html"
    genes: List[str] = []
    description = ""
    systematic = None
    pmid = None
    collection = None
    if record:
        genes = list_from_value(record.get("geneSymbols") or record.get("genes"))
        description = strip_markup(record.get("descriptionBrief") or record.get("descriptionFull") or record.get("description") or "")
        systematic = record.get("systematicName")
        pmid = record.get("pmid")
        collection = record.get("collection")
    snippet = description or f"MSigDB gene set {name}"
    if genes:
        snippet += f" · {len(genes)} genes: {', '.join(genes[:12])}"
    return {
        "id": name,
        "accession": name,
        "title": name,
        "url": url,
        "favicon": FAVICONS["msigdb"],
        "snippet": snippet[:800],
        "content": json.dumps(record or {"name": name}, ensure_ascii=False, indent=2)[:20000],
        "metadata": {"source": "msigdb", "standard_name": name, "species": species, "systematic_name": systematic, "pmid": pmid, "collection": collection, "gene_count": len(genes), "genes": genes, "description": description, "source_specific": record},
        "raw": record or {"name": name},
    }


def handle_msigdb(message_id: str, request: Dict[str, Any]) -> Dict[str, Any]:
    operation = request.get("operation", "query")
    species = msigdb_species(request)
    if operation in ("search", "query"):
        term = query_text(request)
        if not term:
            return error(message_id, "missing_query", "MSigDB search/query requires a gene-set name or keyword")
        exact_name = normalize_msigdb_name(term)
        if re.match(r"^[A-Z0-9_:.+-]+$", exact_name) and any(exact_name.startswith(prefix) for prefix in ("HALLMARK_", "REACTOME_", "KEGG_", "GOBP_", "GOCC_", "GOMF_", "WP_", "BIOCARTA_")):
            request = {**request, "id": exact_name}
            operation = "fetch"
        else:
            params = request_params(request)
            query = {"geneSetName": term}
            if params.get("collection"):
                query["collection"] = str(params["collection"])
            url = f"{MSIGDB}/{species}/genesets.jsp?{urllib.parse.urlencode(query)}"
            body, _ = urlopen_text(url, accept="text/html")
            names = []
            for match in re.finditer(r"msigdb/(?:human|mouse)/geneset/([^\"'>]+?)\.html", body, flags=re.I):
                name = normalize_msigdb_name(match.group(1))
                if name and name not in names:
                    names.append(name)
            items = [msigdb_set_item(name, species) for name in names[:max_results(request)]]
            response = base_response(request, "msigdb", operation, items=items, total=len(names), raw={"url": url, "matched_names": names[:100]})
            return {"id": message_id, "type": "result", "response": response}
    if operation == "fetch":
        name = normalize_msigdb_name(identifier_text(request))
        if not name:
            return error(message_id, "missing_identifier", "MSigDB fetch requires an exact gene-set name, URL, or result")
        data, _ = urlopen_json(f"{MSIGDB}/{species}/geneset/{urllib.parse.quote(name)}.json")
        record = data.get(name) if isinstance(data, dict) and isinstance(data.get(name), dict) else (data if isinstance(data, dict) else {"name": name, "raw": data})
        detail = msigdb_set_item(name, species, record)
        response = base_response(request, "msigdb", operation, detail=detail, total=1, raw={"msigdb": data})
        return {"id": message_id, "type": "result", "response": response}
    return error(message_id, "unsupported_operation", f"MSigDB does not support operation {operation}")


# KEGG

def kegg_text(endpoint: str) -> str:
    body, _ = urlopen_text(f"{KEGG}/{endpoint.lstrip('/')}", timeout=30, accept="text/plain")
    return body


def normalize_kegg_id(value: str) -> str:
    value = (value or "").strip().rstrip("/")
    if value.startswith("http://") or value.startswith("https://"):
        value = urllib.parse.urlparse(value).path.rstrip("/").rsplit("/", 1)[-1]
    match = re.search(r"\b(?:map|hsa|mmu|rno|dre|dme|sce|eco)\d{5}\b", value, re.I)
    if match:
        return match.group(0).lower()
    match = re.search(r"\b(?:[a-z]{3,4}:\S+|cpd:C\d{5}|dr:D\d{5}|ec:[\d.-]+|ko:K\d{5})\b", value, re.I)
    return match.group(0) if match else value


def kegg_item_from_line(line: str) -> Optional[Dict[str, Any]]:
    if not line.strip():
        return None
    parts = line.split("\t", 1)
    entry = parts[0].replace("path:", "")
    name = parts[1].strip() if len(parts) > 1 else entry
    entry_url = entry.replace(":", "/")
    return {
        "id": entry,
        "accession": entry,
        "title": name,
        "url": f"https://www.kegg.jp/entry/{urllib.parse.quote(entry_url)}",
        "favicon": FAVICONS["kegg"],
        "snippet": name,
        "content": line,
        "metadata": {"source": "kegg", "kegg_id": entry, "name": name},
        "raw": {"line": line},
    }


def parse_kegg_entry(text: str, entry_id: str) -> Dict[str, Any]:
    fields: Dict[str, List[str]] = {}
    current = None
    for raw in text.splitlines():
        key = raw[:12].strip()
        value = raw[12:].strip() if len(raw) > 12 else ""
        if key:
            current = key
            fields.setdefault(key, []).append(value)
        elif current:
            fields.setdefault(current, []).append(value)
    title = fields.get("NAME", [entry_id])[0].rstrip(";")
    description = " ".join(fields.get("DESCRIPTION", []) or fields.get("CLASS", []) or [])
    url_id = entry_id.replace(":", "/")
    return {
        "id": entry_id,
        "accession": entry_id,
        "title": title,
        "url": f"https://www.kegg.jp/entry/{urllib.parse.quote(url_id)}",
        "favicon": FAVICONS["kegg"],
        "snippet": description[:800] or title,
        "content": text[:20000],
        "metadata": {"source": "kegg", "kegg_id": entry_id, "name": title, "fields": fields},
        "raw": {"entry": text, "fields": fields},
    }


def handle_kegg(message_id: str, request: Dict[str, Any]) -> Dict[str, Any]:
    operation = request.get("operation", "query")
    params = request_params(request)
    mode = str(params.get("mode") or "").strip().lower().replace("-", "_")
    database = str(params.get("database") or "pathway").strip()
    organism = str(params.get("organism") or params.get("species") or "hsa").strip()
    option = str(params.get("option") or "").strip()
    target = str(params.get("target") or "").strip()
    term = query_text(request)

    if operation in ("search", "query"):
        if mode == "info":
            text = kegg_text(f"info/{database}")
            detail = parse_kegg_entry(text, database)
            response = base_response(request, "kegg", operation, detail=detail, total=1, raw={"endpoint": f"info/{database}"})
            return {"id": message_id, "type": "result", "response": response}
        if mode == "link":
            if not target:
                target = "pathway"
            source = term or str(params.get("source") or organism)
            if not source:
                return error(message_id, "missing_query", "KEGG link mode requires query/source and target")
            text = kegg_text(f"link/{urllib.parse.quote(target)}/{urllib.parse.quote(source)}")
            lines = [line for line in text.splitlines() if line.strip()]
            items = [item for item in (kegg_item_from_line(line) for line in lines[:max_results(request, ceiling=100)]) if item]
            response = base_response(request, "kegg", operation, items=items, total=len(lines), raw={"endpoint": f"link/{target}/{source}"})
            return {"id": message_id, "type": "result", "response": response}
        if mode == "conv":
            if not target:
                target = "ncbi-geneid"
            source = term or str(params.get("source") or organism)
            text = kegg_text(f"conv/{urllib.parse.quote(target)}/{urllib.parse.quote(source)}")
            lines = [line for line in text.splitlines() if line.strip()]
            items = [item for item in (kegg_item_from_line(line) for line in lines[:max_results(request, ceiling=100)]) if item]
            response = base_response(request, "kegg", operation, items=items, total=len(lines), raw={"endpoint": f"conv/{target}/{source}"})
            return {"id": message_id, "type": "result", "response": response}
        if mode == "find" and term:
            endpoint = f"find/{urllib.parse.quote(database)}/{urllib.parse.quote(term)}" + (f"/{urllib.parse.quote(option)}" if option else "")
            text = kegg_text(endpoint)
            lines = [line for line in text.splitlines() if line.strip()]
            items = [item for item in (kegg_item_from_line(line) for line in lines[:max_results(request)]) if item]
            response = base_response(request, "kegg", operation, items=items, total=len(lines), raw={"endpoint": endpoint})
            return {"id": message_id, "type": "result", "response": response}
        if term and re.match(r"^(?:map|[a-z]{3,4})\d{5}$", term.strip(), re.I):
            request = {**request, "id": term}
            operation = "fetch"
        else:
            endpoint = f"list/pathway/{urllib.parse.quote(organism)}" if database == "pathway" else f"list/{urllib.parse.quote(database)}"
            text = kegg_text(endpoint)
            lines = [line for line in text.splitlines() if line.strip()]
            needle = (term or "").lower()
            filtered = [line for line in lines if not needle or needle in line.lower()]
            items = [item for item in (kegg_item_from_line(line) for line in filtered[:max_results(request)]) if item]
            response = base_response(request, "kegg", operation, items=items, total=len(filtered), notes=["KEGG REST API; academic-use terms may apply."], raw={"endpoint": endpoint})
            return {"id": message_id, "type": "result", "response": response}

    if operation == "fetch":
        entry_id = normalize_kegg_id(identifier_text(request) or term)
        if not entry_id:
            return error(message_id, "missing_identifier", "KEGG fetch requires a KEGG entry ID, URL, or result")
        endpoint = f"get/{urllib.parse.quote(entry_id)}" + (f"/{urllib.parse.quote(option)}" if option else "")
        text = kegg_text(endpoint)
        detail = parse_kegg_entry(text, entry_id) if not option else {
            "id": entry_id,
            "accession": entry_id,
            "title": f"{entry_id} ({option})",
            "url": f"https://www.kegg.jp/entry/{urllib.parse.quote(entry_id.replace(':', '/'))}",
            "favicon": FAVICONS["kegg"],
            "snippet": f"KEGG {option} payload for {entry_id}",
            "content": text[:20000],
            "metadata": {"source": "kegg", "kegg_id": entry_id, "option": option},
            "raw": {"payload": text},
        }
        response = base_response(request, "kegg", operation, detail=detail, total=1, notes=["KEGG REST API; academic-use terms may apply."], raw={"endpoint": endpoint})
        return {"id": message_id, "type": "result", "response": response}
    return error(message_id, "unsupported_operation", f"KEGG does not support operation {operation}")


def handle_execute(message: Dict[str, Any]) -> Dict[str, Any]:
    message_id = str(message.get("id", "execute"))
    request = message.get("request") if isinstance(message.get("request"), dict) else {}
    source = normalize_source(str(request.get("source", "")))
    if not source_is_allowed(source):
        return error(message_id, "unknown_source", f"source is not served by this plugin: {source}")
    if is_validation(request):
        return validation_result(message_id, request)
    try:
        if source == "reactome":
            return handle_reactome(message_id, request)
        if source == "gene_ontology":
            return handle_gene_ontology(message_id, request)
        if source == "msigdb":
            return handle_msigdb(message_id, request)
        if source == "kegg":
            return handle_kegg(message_id, request)
        return error(message_id, "unknown_source", f"unknown pathway source: {source}")
    except Exception as exc:
        return error(message_id, "provider_error", str(exc))


def main() -> int:
    for line in sys.stdin:
        if not line.strip():
            continue
        try:
            message = json.loads(line)
        except json.JSONDecodeError as exc:
            write(error("unknown", "bad_json", f"invalid JSON input: {exc}"))
            continue
        message_type = message.get("type")
        message_id = str(message.get("id", message_type or "unknown"))
        if message_type == "initialize":
            write({"id": message_id, "type": "initialized", "protocolVersion": PROTOCOL_VERSION, "resources": configured_sources()})
        elif message_type == "execute":
            write(handle_execute(message))
        elif message_type == "shutdown":
            write({"id": message_id, "type": "shutdown"})
            return 0
        else:
            write(error(message_id, "unknown_message_type", f"unsupported message type: {message_type}"))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
