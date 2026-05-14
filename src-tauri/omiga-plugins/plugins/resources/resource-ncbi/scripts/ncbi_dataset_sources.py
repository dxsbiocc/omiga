#!/usr/bin/env python3
"""Public dataset retrieval plugin for Omiga's local JSONL protocol."""

from __future__ import annotations

import json
import os
import re
import sys
import urllib.error
import urllib.parse
import urllib.request
from typing import Any, Dict, Iterable, List, Optional, Tuple


from pathlib import Path

RESOURCE_UTILS = Path(__file__).resolve().parents[2] / "utils"
if RESOURCE_UTILS.is_dir() and str(RESOURCE_UTILS) not in sys.path:
    sys.path.insert(0, str(RESOURCE_UTILS))

from retrieval_http import fetch_json, fetch_json_with_headers, fetch_text, fetch_text_with_headers, with_query_credentials


PROTOCOL_VERSION = 1

SOURCES = [
    {"category": 'dataset', "id": 'geo', "capabilities": ["search", "query", "fetch"]},
    {"category": 'dataset', "id": 'biosample', "capabilities": ["search", "query", "fetch"]},
    {"category": 'dataset', "id": 'ncbi_datasets', "capabilities": ["search", "query", "fetch"]},
]

PLUGIN_NAME = os.environ.get("OMIGA_RETRIEVAL_PLUGIN_NAME", 'resource-ncbi')


def configured_source_ids() -> Optional[set[str]]:
    raw = os.environ.get("OMIGA_RETRIEVAL_SOURCE_IDS", "")
    values = {value.strip().lower().replace("-", "_") for value in raw.split(",") if value.strip()}
    return values or None


def configured_sources() -> List[Dict[str, Any]]:
    allowed = configured_source_ids()
    if allowed is None:
        return SOURCES
    return [source for source in SOURCES if source.get("id") in allowed]


def source_is_allowed(source: str) -> bool:
    allowed = configured_source_ids()
    return allowed is None or source in allowed

NCBI_EUTILS = "https://eutils.ncbi.nlm.nih.gov/entrez/eutils"
NCBI_DATASETS = "https://api.ncbi.nlm.nih.gov/datasets/v2"
BIOSTUDIES = "https://www.ebi.ac.uk/biostudies/api/v1"
GTEX = "https://gtexportal.org/api/v2"
CBIOPORTAL = "https://www.cbioportal.org/api"
ENA_PORTAL = "https://www.ebi.ac.uk/ena/portal/api/search"
ENA_BROWSER_XML = "https://www.ebi.ac.uk/ena/browser/api/xml"
USER_AGENT = "Omiga public-dataset-sources retrieval plugin/0.1"

VALIDATION_ACCESSIONS = {
    "geo": "GSE000001",
    "biosample": "SAMN00000001",
    "ncbi_datasets": "GCF_000001405.40"
}



def write(message: Dict[str, Any]) -> None:
    print(json.dumps(message, separators=(",", ":"), ensure_ascii=False), flush=True)


def error(message_id: str, code: str, message: str) -> Dict[str, Any]:
    return {"id": message_id, "type": "error", "error": {"code": code, "message": message}}


def request_params(request: Dict[str, Any]) -> Dict[str, Any]:
    params = request.get("params")
    return params if isinstance(params, dict) else {}


def max_results(request: Dict[str, Any], default: int = 5, ceiling: int = 25) -> int:
    value = request.get("maxResults") or request.get("max_results") or request_params(request).get("limit") or request_params(request).get("retmax") or default
    try:
        parsed = int(value)
    except (TypeError, ValueError):
        parsed = default
    return max(1, min(parsed, ceiling))


def is_validation(request: Dict[str, Any]) -> bool:
    return bool(request_params(request).get("omigaValidation"))


def query_text(request: Dict[str, Any]) -> str:
    params = request_params(request)
    for key in ("query", "term", "q", "text"):
        value = request.get(key) if key in request else params.get(key)
        if isinstance(value, str) and value.strip():
            return value.strip()
    return ""


def identifier_text(request: Dict[str, Any]) -> str:
    params = request_params(request)
    for key in ("id", "accession", "uid"):
        value = request.get(key) if key in request else params.get(key)
        if isinstance(value, str) and value.strip():
            return value.strip()
    result = request.get("result")
    if isinstance(result, dict):
        for key in ("id", "accession", "uid"):
            value = result.get(key)
            if isinstance(value, str) and value.strip():
                return value.strip()
        metadata = result.get("metadata")
        if isinstance(metadata, dict):
            for key in ("id", "accession", "uid"):
                value = metadata.get(key)
                if isinstance(value, str) and value.strip():
                    return value.strip()
    url = request.get("url")
    if isinstance(url, str):
        for pattern in (
            r"(SAM[NEDAG]?\d+)",
            r"/biosample/(\d+)",
            r"(E-[A-Z]+-\d+)",
            r"[?&]acc=([^&#]+)",
            r"(G(?:SE|SM|PL|DS)\d+)",
            r"/ena/browser/view/([^/?#]+)",
            r"\b((?:PRJ|ERP|SRP|DRP|ERX|SRX|DRX|ERR|SRR|DRR|ERS|SRS|DRS|ERZ|SRZ|DRZ)[A-Z]*\d+)\b",
            r"(GC[AF]_\d+(?:\.\d+)?)",
            r"[?&](?:id|studyId|study_id)=([^&#]+)",
            r"/gene/([^/?#]+)",
        ):
            match = re.search(pattern, url, re.IGNORECASE)
            if match:
                return urllib.parse.unquote(match.group(1))
    return ""


def credentials(request: Dict[str, Any]) -> Dict[str, str]:
    value = request.get("credentials")
    if not isinstance(value, dict):
        return {}
    return {str(k): str(v) for k, v in value.items() if str(v).strip()}




def urlopen_json(url: str, timeout: int = 25, headers: Optional[Dict[str, str]] = None) -> Any:
    return fetch_json(url, timeout=timeout, headers=headers, user_agent=USER_AGENT)


def with_ncbi_credentials(url: str, request: Dict[str, Any]) -> str:
    return with_query_credentials(
        url,
        credentials(request),
        api_key_key="pubmed_api_key",
        email_key="pubmed_email",
        tool_key="pubmed_tool_name",
    )

def ncbi_header_credentials(request: Dict[str, Any]) -> Dict[str, str]:
    creds = credentials(request)
    if creds.get("pubmed_api_key"):
        return {"api-key": creds["pubmed_api_key"]}
    return {}


def plugin_item(source: str, operation: str, index: int = 1) -> Dict[str, Any]:
    accession = VALIDATION_ACCESSIONS.get(source, f"{source}-validation")
    return {
        "id": accession,
        "accession": accession,
        "title": f"Validation {source} {operation} result",
        "url": f"https://example.test/{source}/{accession}",
        "snippet": "Offline validation result from public-dataset-sources plugin.",
        "content": "This fixture response is returned only for Omiga validation smoke calls.",
        "metadata": {"source": source, "validation": True, "index": index},
        "raw": {"validation": True},
    }


def validation_result(message_id: str, request: Dict[str, Any]) -> Dict[str, Any]:
    operation = request.get("operation", "search")
    source = request.get("source", "biosample")
    response: Dict[str, Any] = {
        "ok": True,
        "operation": operation,
        "category": request.get("category", "dataset"),
        "source": source,
        "effectiveSource": source,
        "notes": ["offline validation response"],
        "raw": {"validation": True},
        "total": 1,
    }
    if operation == "fetch":
        response["items"] = []
        response["detail"] = plugin_item(source, operation)
    else:
        response["items"] = [plugin_item(source, operation)]
    return {"id": message_id, "type": "result", "response": response}



def first_string(record: Dict[str, Any], *keys: str) -> str:
    for key in keys:
        value = record.get(key)
        if isinstance(value, str) and value.strip():
            return value.strip()
        if value is not None and not isinstance(value, (dict, list)):
            text = str(value).strip()
            if text:
                return text
    return ""


def nested_string(record: Dict[str, Any], *path: str) -> str:
    value: Any = record
    for key in path:
        if not isinstance(value, dict):
            return ""
        value = value.get(key)
    if isinstance(value, str):
        return value.strip()
    return "" if value is None else str(value).strip()


def data_items(value: Any) -> List[Any]:
    if isinstance(value, list):
        return value
    if isinstance(value, dict):
        for key in ("data", "reports", "items", "results", "hits", "studies"):
            items = value.get(key)
            if isinstance(items, list):
                return items
    return []



def normalize_geo_accession(value: str) -> str:
    match = re.search(r"\bG(?:SE|SM|PL|DS)\d+\b", value or "", re.IGNORECASE)
    if match:
        return match.group(0).upper()
    match = re.search(r"\b\d+\b", value or "")
    return match.group(0) if match else value.strip()


def geo_esearch(request: Dict[str, Any], term: str) -> List[str]:
    encoded = urllib.parse.urlencode({
        "db": "gds",
        "term": term,
        "retmode": "json",
        "retmax": str(max_results(request)),
    })
    data = urlopen_json(with_ncbi_credentials(f"{NCBI_EUTILS}/esearch.fcgi?{encoded}", request))
    ids = data.get("esearchresult", {}).get("idlist", [])
    return [str(value) for value in ids]


def geo_esummary(request: Dict[str, Any], ids: Iterable[str]) -> List[Dict[str, Any]]:
    ids = [str(value) for value in ids if str(value).strip()]
    if not ids:
        return []
    encoded = urllib.parse.urlencode({"db": "gds", "id": ",".join(ids), "retmode": "json"})
    data = urlopen_json(with_ncbi_credentials(f"{NCBI_EUTILS}/esummary.fcgi?{encoded}", request))
    result = data.get("result", {}) if isinstance(data, dict) else {}
    items = []
    for uid in result.get("uids", ids):
        record = result.get(str(uid), {}) if isinstance(result, dict) else {}
        items.append(geo_item(record, str(uid)))
    return items


def geo_item(record: Dict[str, Any], fallback_id: str = "") -> Dict[str, Any]:
    accession = str(record.get("accession") or record.get("gds") or fallback_id)
    if accession.isdigit():
        accession = f"GDS{accession}"
    title = str(record.get("title") or record.get("summary") or accession)
    summary = str(record.get("summary") or record.get("taxon") or title)
    url = f"https://www.ncbi.nlm.nih.gov/geo/query/acc.cgi?acc={urllib.parse.quote(accession)}"
    return {
        "id": fallback_id or accession,
        "accession": accession,
        "title": title,
        "url": url,
        "snippet": summary[:500],
        "content": json.dumps(record, ensure_ascii=False, indent=2)[:20000],
        "metadata": {"source": "geo", "uid": fallback_id, "entry_type": record.get("entrytype"), "taxon": record.get("taxon")},
        "raw": record,
    }


def handle_geo(message_id: str, request: Dict[str, Any]) -> Dict[str, Any]:
    operation = request.get("operation", "search")
    if operation in ("search", "query"):
        term = query_text(request)
        if not term:
            return error(message_id, "missing_query", "GEO search/query requires query text")
        items = geo_esummary(request, geo_esearch(request, term))
        response = base_response(request, "geo", operation, items=items, total=len(items))
        return {"id": message_id, "type": "result", "response": response}
    if operation == "fetch":
        identifier = normalize_geo_accession(identifier_text(request))
        if not identifier:
            return error(message_id, "missing_identifier", "GEO fetch requires accession, URL, or prior result")
        ids = [identifier] if identifier.isdigit() else geo_esearch(request, identifier)[:1]
        items = geo_esummary(request, ids)
        if not items:
            return error(message_id, "not_found", f"GEO record not found: {identifier}")
        response = base_response(request, "geo", operation, items=[], detail=items[0], total=1)
        return {"id": message_id, "type": "result", "response": response}
    return error(message_id, "unsupported_operation", f"GEO does not support operation {operation}")

def normalize_biosample_id(value: str) -> str:
    value = value.strip()
    match = re.search(r"SAM[NEDAG]?\d+", value, re.IGNORECASE)
    if match:
        return match.group(0).upper()
    match = re.search(r"\b\d+\b", value)
    return match.group(0) if match else value


def biosample_esearch(request: Dict[str, Any], term: str) -> List[str]:
    encoded = urllib.parse.urlencode({
        "db": "biosample",
        "term": term,
        "retmode": "json",
        "retmax": str(max_results(request)),
    })
    data = urlopen_json(with_ncbi_credentials(f"{NCBI_EUTILS}/esearch.fcgi?{encoded}", request))
    ids = data.get("esearchresult", {}).get("idlist", [])
    return [str(value) for value in ids]


def biosample_esummary(request: Dict[str, Any], ids: Iterable[str]) -> List[Dict[str, Any]]:
    ids = [str(value) for value in ids if str(value).strip()]
    if not ids:
        return []
    encoded = urllib.parse.urlencode({"db": "biosample", "id": ",".join(ids), "retmode": "json"})
    data = urlopen_json(with_ncbi_credentials(f"{NCBI_EUTILS}/esummary.fcgi?{encoded}", request))
    result = data.get("result", {}) if isinstance(data, dict) else {}
    items = []
    for uid in result.get("uids", ids):
        record = result.get(str(uid), {}) if isinstance(result, dict) else {}
        title = record.get("title") or record.get("sampledata") or f"BioSample {uid}"
        accession = record.get("accession") or record.get("sampleid") or str(uid)
        organism = record.get("organism") or record.get("taxname")
        items.append({
            "id": str(uid),
            "accession": str(accession),
            "title": str(title),
            "url": f"https://www.ncbi.nlm.nih.gov/biosample/{uid}",
            "snippet": str(organism or title)[:500],
            "metadata": {"uid": str(uid), "accession": str(accession), "organism": organism},
            "raw": record,
        })
    return items


def biosample_fetch_report(request: Dict[str, Any], identifier: str) -> Optional[Dict[str, Any]]:
    if not re.match(r"SAM[NEDAG]?\d+", identifier, re.IGNORECASE):
        return None
    encoded = urllib.parse.quote(identifier, safe="")
    data = urlopen_json(with_ncbi_credentials(f"{NCBI_DATASETS}/biosample/{encoded}/reports", request))
    reports = data.get("reports") if isinstance(data, dict) else None
    if not reports:
        return None
    record = reports[0]
    sample = record.get("sample", record) if isinstance(record, dict) else record
    title = sample.get("title") or sample.get("accession") or identifier
    return {
        "id": identifier,
        "accession": sample.get("accession") or identifier,
        "title": str(title),
        "url": f"https://www.ncbi.nlm.nih.gov/biosample/{identifier}",
        "snippet": str(sample.get("organism", {}).get("organismName") or title)[:500] if isinstance(sample, dict) else str(title),
        "content": json.dumps(record, ensure_ascii=False, indent=2)[:20000],
        "metadata": {"accession": identifier, "api": "ncbi_datasets_biosample_reports"},
        "raw": record,
    }


def handle_biosample(message_id: str, request: Dict[str, Any]) -> Dict[str, Any]:
    operation = request.get("operation", "search")
    if operation in ("search", "query"):
        term = query_text(request)
        if not term:
            return error(message_id, "missing_query", "BioSample search/query requires query text")
        ids = biosample_esearch(request, term)
        items = biosample_esummary(request, ids)
        response = base_response(request, "biosample", operation, items=items, total=len(items))
        return {"id": message_id, "type": "result", "response": response}
    if operation == "fetch":
        identifier = normalize_biosample_id(identifier_text(request))
        if not identifier:
            return error(message_id, "missing_identifier", "BioSample fetch requires id, URL, or prior result")
        detail = biosample_fetch_report(request, identifier)
        if detail is None:
            ids = [identifier] if identifier.isdigit() else biosample_esearch(request, identifier)[:1]
            items = biosample_esummary(request, ids)
            detail = items[0] if items else None
        if detail is None:
            return error(message_id, "not_found", f"BioSample record not found: {identifier}")
        response = base_response(request, "biosample", operation, items=[], detail=detail, total=1)
        return {"id": message_id, "type": "result", "response": response}
    return error(message_id, "unsupported_operation", f"BioSample does not support operation {operation}")



def normalize_ncbi_assembly_accession(value: str) -> str:
    match = re.search(r"\bGC[AF]_\d+(?:\.\d+)?\b", value or "", re.IGNORECASE)
    return match.group(0).upper() if match else ""


def ncbi_datasets_mode(request: Dict[str, Any], term: str) -> str:
    params = request_params(request)
    raw = params.get("mode") or params.get("endpoint") or params.get("lookup") or params.get("by") or params.get("type")
    normalized = str(raw or "").strip().lower().replace("-", "_").replace(" ", "_")
    aliases = {
        "accessions": "accession",
        "assembly_accession": "accession",
        "assembly_accessions": "accession",
        "taxonomy": "taxon",
        "organism": "taxon",
        "taxid": "taxon",
        "tax_id": "taxon",
        "bio_project": "bioproject",
        "bioprojects": "bioproject",
        "project": "bioproject",
        "bio_sample": "biosample",
        "biosample_id": "biosample",
        "sample": "biosample",
        "wgs_accession": "wgs",
        "assembly_names": "assembly_name",
        "name": "assembly_name",
    }
    if normalized in {"accession", "taxon", "bioproject", "biosample", "wgs", "assembly_name"}:
        return normalized
    if normalized in aliases:
        return aliases[normalized]
    upper = term.upper()
    if normalize_ncbi_assembly_accession(term):
        return "accession"
    if upper.startswith(("PRJN", "PRJE", "PRJD")):
        return "bioproject"
    if upper.startswith(("SAMN", "SAMEA", "SAMD")):
        return "biosample"
    return "taxon"


def ncbi_datasets_endpoint(mode: str, lookup: str) -> str:
    encoded = urllib.parse.quote(lookup, safe="")
    if mode == "accession":
        return f"genome/accession/{encoded}/dataset_report"
    if mode == "bioproject":
        return f"genome/bioproject/{encoded}/dataset_report"
    if mode == "biosample":
        return f"genome/biosample/{encoded}/dataset_report"
    if mode == "wgs":
        return f"genome/wgs/{encoded}/dataset_report"
    if mode == "assembly_name":
        return f"genome/assembly_name/{encoded}/dataset_report"
    return f"genome/taxon/{encoded}/dataset_report"


def ncbi_datasets_json(endpoint: str, request: Dict[str, Any], params: Optional[Dict[str, Any]] = None) -> Any:
    query = dict(params or {})
    if "page_size" not in query:
        query["page_size"] = str(max_results(request))
    encoded = urllib.parse.urlencode(query, doseq=True)
    url = f"{NCBI_DATASETS}/{endpoint.lstrip('/')}"
    if encoded:
        url = f"{url}?{encoded}"
    return urlopen_json(url, headers=ncbi_header_credentials(request))


def ncbi_datasets_item(record: Dict[str, Any]) -> Dict[str, Any]:
    accession = first_string(record, "accession", "current_accession")
    organism = nested_string(record, "organism", "organism_name")
    assembly_name = nested_string(record, "assembly_info", "assembly_name")
    assembly_level = nested_string(record, "assembly_info", "assembly_level")
    description = nested_string(record, "assembly_info", "description")
    title = " ".join(part for part in (organism, assembly_name) if part).strip() or accession
    summary_parts = [part for part in (description, f"Level: {assembly_level}" if assembly_level else "") if part]
    return {
        "id": accession,
        "accession": accession,
        "title": title,
        "url": f"https://www.ncbi.nlm.nih.gov/datasets/genome/{urllib.parse.quote(accession)}/",
        "snippet": " | ".join(summary_parts)[:500],
        "content": json.dumps(record, ensure_ascii=False, indent=2)[:20000],
        "metadata": {
            "source": "ncbi_datasets",
            "organism": organism,
            "assembly_name": assembly_name,
            "assembly_level": assembly_level,
            "download_summary_url": f"{NCBI_DATASETS}/genome/accession/{urllib.parse.quote(accession)}/download_summary" if accession else None,
        },
        "raw": record,
    }


def handle_ncbi_datasets(message_id: str, request: Dict[str, Any]) -> Dict[str, Any]:
    operation = request.get("operation", "search")
    if operation in ("search", "query"):
        term = query_text(request)
        if not term:
            return error(message_id, "missing_query", "NCBI Datasets search/query requires query text")
        mode = ncbi_datasets_mode(request, term)
        lookup = normalize_ncbi_assembly_accession(term) if mode == "accession" else term
        if not lookup:
            return error(message_id, "missing_identifier", "NCBI Datasets accession lookup requires a GCA_/GCF_ accession")
        data = ncbi_datasets_json(ncbi_datasets_endpoint(mode, lookup), request)
        items = [ncbi_datasets_item(item) for item in data_items(data) if isinstance(item, dict)]
        total = data.get("total_count") if isinstance(data, dict) else len(items)
        response = base_response(request, "ncbi_datasets", operation, items=items, total=total if isinstance(total, int) else len(items))
        return {"id": message_id, "type": "result", "response": response}
    if operation == "fetch":
        accession = normalize_ncbi_assembly_accession(identifier_text(request))
        if not accession:
            return error(message_id, "missing_identifier", "NCBI Datasets fetch requires a GCA_/GCF_ accession")
        data = ncbi_datasets_json(ncbi_datasets_endpoint("accession", accession), request, {"page_size": "1"})
        items = [ncbi_datasets_item(item) for item in data_items(data) if isinstance(item, dict)]
        if not items:
            return error(message_id, "not_found", f"NCBI Datasets genome report not found: {accession}")
        response = base_response(request, "ncbi_datasets", operation, items=[], detail=items[0], total=1)
        return {"id": message_id, "type": "result", "response": response}
    if operation == "download_summary":
        accession = normalize_ncbi_assembly_accession(identifier_text(request) or query_text(request))
        if not accession:
            return error(message_id, "missing_identifier", "NCBI Datasets download_summary requires a GCA_/GCF_ accession")
        data = ncbi_datasets_json(f"genome/accession/{urllib.parse.quote(accession)}/download_summary", request, {})
        detail = {
            "id": accession,
            "accession": accession,
            "title": f"NCBI Datasets download summary for {accession}",
            "url": f"https://www.ncbi.nlm.nih.gov/datasets/genome/{urllib.parse.quote(accession)}/",
            "snippet": f"Record count: {data.get('record_count')}" if isinstance(data, dict) else "",
            "content": json.dumps(data, ensure_ascii=False, indent=2)[:20000],
            "metadata": {"source": "ncbi_datasets", "operation": "download_summary"},
            "raw": data,
        }
        response = base_response(request, "ncbi_datasets", operation, items=[], detail=detail, total=1)
        return {"id": message_id, "type": "result", "response": response}
    return error(message_id, "unsupported_operation", f"NCBI Datasets does not support operation {operation}")



def base_response(
    request: Dict[str, Any],
    source: str,
    operation: str,
    *,
    items: Optional[List[Dict[str, Any]]] = None,
    detail: Optional[Dict[str, Any]] = None,
    total: Optional[int] = None,
) -> Dict[str, Any]:
    response: Dict[str, Any] = {
        "ok": True,
        "operation": operation,
        "category": request.get("category", "dataset"),
        "source": source,
        "effectiveSource": source,
        "items": items or [],
        "total": total,
        "notes": [f"{PLUGIN_NAME} plugin"],
        "raw": {"plugin": PLUGIN_NAME},
    }
    if detail is not None:
        response["detail"] = detail
    return response




def handle_execute(message: Dict[str, Any]) -> Dict[str, Any]:
    message_id = str(message.get("id", "execute"))
    request = message.get("request") if isinstance(message.get("request"), dict) else {}
    source = str(request.get("source", "")).strip().lower().replace("-", "_")
    if not source_is_allowed(source):
        return error(message_id, "unknown_source", f"source is not served by this plugin: {source}")
    if is_validation(request):
        return validation_result(message_id, request)
    try:
        if source == "geo":
            return handle_geo(message_id, request)
        if source == "biosample":
            return handle_biosample(message_id, request)
        if source == "ncbi_datasets":
            return handle_ncbi_datasets(message_id, request)
        return error(message_id, "unknown_source", f"unknown dataset source: {source}")
    except Exception as exc:  # Keep provider failures structured for host quarantine/backoff.
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
