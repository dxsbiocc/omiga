#!/usr/bin/env python3
"""Public dataset retrieval plugin for Omiga's local JSONL protocol."""

from __future__ import annotations

import json
import re
import sys
import urllib.error
import urllib.parse
import urllib.request
from typing import Any, Dict, Iterable, List, Optional, Tuple

PROTOCOL_VERSION = 1
SOURCES = [
    {"category": "dataset", "id": "biosample", "capabilities": ["search", "query", "fetch"]},
    {"category": "dataset", "id": "arrayexpress", "capabilities": ["search", "query", "fetch"]},
]
NCBI_EUTILS = "https://eutils.ncbi.nlm.nih.gov/entrez/eutils"
NCBI_DATASETS = "https://api.ncbi.nlm.nih.gov/datasets/v2"
BIOSTUDIES = "https://www.ebi.ac.uk/biostudies/api/v1"
USER_AGENT = "Omiga public-dataset-sources retrieval plugin/0.1"


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
        for pattern in (r"(SAM[NEDAG]?\d+)", r"/biosample/(\d+)", r"(E-[A-Z]+-\d+)"):
            match = re.search(pattern, url, re.IGNORECASE)
            if match:
                return match.group(1)
    return ""


def credentials(request: Dict[str, Any]) -> Dict[str, str]:
    value = request.get("credentials")
    if not isinstance(value, dict):
        return {}
    return {str(k): str(v) for k, v in value.items() if str(v).strip()}


def urlopen_json(url: str, timeout: int = 25) -> Any:
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT, "Accept": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as response:
            raw = response.read().decode("utf-8", errors="replace")
    except urllib.error.HTTPError as exc:
        body = exc.read().decode("utf-8", errors="replace")[:500]
        raise RuntimeError(f"HTTP {exc.code} from {url}: {body}") from exc
    except urllib.error.URLError as exc:
        raise RuntimeError(f"request failed for {url}: {exc.reason}") from exc
    return json.loads(raw)


def with_ncbi_credentials(url: str, request: Dict[str, Any]) -> str:
    creds = credentials(request)
    parts = urllib.parse.urlsplit(url)
    query = dict(urllib.parse.parse_qsl(parts.query, keep_blank_values=True))
    if creds.get("pubmed_api_key"):
        query["api_key"] = creds["pubmed_api_key"]
    if creds.get("pubmed_email"):
        query["email"] = creds["pubmed_email"]
    if creds.get("pubmed_tool_name"):
        query["tool"] = creds["pubmed_tool_name"]
    encoded = urllib.parse.urlencode(query)
    return urllib.parse.urlunsplit((parts.scheme, parts.netloc, parts.path, encoded, parts.fragment))


def plugin_item(source: str, operation: str, index: int = 1) -> Dict[str, Any]:
    accession = "SAMN00000001" if source == "biosample" else "E-MTAB-0000"
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


def normalize_arrayexpress_accession(value: str) -> str:
    match = re.search(r"E-[A-Z]+-\d+", value or "", re.IGNORECASE)
    return match.group(0).upper() if match else value.strip()


def arrayexpress_item(record: Dict[str, Any], fallback_id: str = "") -> Dict[str, Any]:
    accession = str(record.get("accession") or record.get("id") or record.get("accno") or fallback_id)
    title = str(record.get("title") or record.get("name") or accession)
    description = record.get("description") or record.get("releaseDate") or title
    return {
        "id": accession,
        "accession": accession,
        "title": title,
        "url": f"https://www.ebi.ac.uk/biostudies/arrayexpress/studies/{urllib.parse.quote(accession)}" if accession else "https://www.ebi.ac.uk/biostudies/arrayexpress",
        "snippet": str(description)[:500],
        "content": json.dumps(record, ensure_ascii=False, indent=2)[:20000],
        "metadata": {"accession": accession, "source": "arrayexpress"},
        "raw": record,
    }


def arrayexpress_search(request: Dict[str, Any], term: str) -> List[Dict[str, Any]]:
    encoded = urllib.parse.urlencode({"query": term, "limit": str(max_results(request)), "collection": "ArrayExpress"})
    data = urlopen_json(f"{BIOSTUDIES}/search?{encoded}")
    hits = []
    if isinstance(data, dict):
        for key in ("hits", "results", "studies"):
            value = data.get(key)
            if isinstance(value, list):
                hits = value
                break
        if not hits and isinstance(data.get("data"), list):
            hits = data["data"]
    return [arrayexpress_item(hit if isinstance(hit, dict) else {"accession": str(hit)}) for hit in hits[: max_results(request)]]


def arrayexpress_fetch(accession: str) -> Dict[str, Any]:
    accession = normalize_arrayexpress_accession(accession)
    if not accession:
        raise RuntimeError("missing ArrayExpress accession")
    data = urlopen_json(f"{BIOSTUDIES}/studies/{urllib.parse.quote(accession)}")
    record = data if isinstance(data, dict) else {"accession": accession, "raw": data}
    return arrayexpress_item(record, accession)


def handle_arrayexpress(message_id: str, request: Dict[str, Any]) -> Dict[str, Any]:
    operation = request.get("operation", "search")
    if operation in ("search", "query"):
        term = query_text(request)
        if not term:
            return error(message_id, "missing_query", "ArrayExpress search/query requires query text")
        items = arrayexpress_search(request, term)
        response = base_response(request, "arrayexpress", operation, items=items, total=len(items))
        return {"id": message_id, "type": "result", "response": response}
    if operation == "fetch":
        accession = normalize_arrayexpress_accession(identifier_text(request))
        if not accession:
            return error(message_id, "missing_identifier", "ArrayExpress fetch requires accession, URL, or prior result")
        detail = arrayexpress_fetch(accession)
        response = base_response(request, "arrayexpress", operation, items=[], detail=detail, total=1)
        return {"id": message_id, "type": "result", "response": response}
    return error(message_id, "unsupported_operation", f"ArrayExpress does not support operation {operation}")


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
        "notes": ["public-dataset-sources plugin"],
        "raw": {"plugin": "public-dataset-sources"},
    }
    if detail is not None:
        response["detail"] = detail
    return response


def handle_execute(message: Dict[str, Any]) -> Dict[str, Any]:
    message_id = str(message.get("id", "execute"))
    request = message.get("request") if isinstance(message.get("request"), dict) else {}
    source = str(request.get("source", "")).strip().lower().replace("-", "_")
    if is_validation(request):
        return validation_result(message_id, request)
    try:
        if source == "biosample":
            return handle_biosample(message_id, request)
        if source == "arrayexpress":
            return handle_arrayexpress(message_id, request)
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
            write({"id": message_id, "type": "initialized", "protocolVersion": PROTOCOL_VERSION, "sources": SOURCES})
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
