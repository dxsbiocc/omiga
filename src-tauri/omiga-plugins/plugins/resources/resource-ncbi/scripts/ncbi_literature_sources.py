#!/usr/bin/env python3
"""Public literature retrieval plugin for Omiga's local JSONL protocol."""

from __future__ import annotations

import json
import os
import re
import sys
import urllib.error
import urllib.parse
import urllib.request
from typing import Any, Dict, Iterable, List, Optional


from pathlib import Path

RESOURCE_UTILS = Path(__file__).resolve().parents[2] / "utils"
if RESOURCE_UTILS.is_dir() and str(RESOURCE_UTILS) not in sys.path:
    sys.path.insert(0, str(RESOURCE_UTILS))

from retrieval_http import fetch_json, fetch_json_with_headers, fetch_text, fetch_text_with_headers, with_query_credentials


PROTOCOL_VERSION = 1

SOURCES = [
    {"category": 'literature', "id": 'pubmed', "capabilities": ["search", "query", "fetch"]},
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
SEMANTIC_SCHOLAR = "https://api.semanticscholar.org/graph/v1"
USER_AGENT = "Omiga public-literature-sources retrieval plugin/0.1"
S2_FIELDS = "paperId,externalIds,url,title,abstract,venue,year,authors,citationCount,influentialCitationCount,publicationDate,openAccessPdf,fieldsOfStudy"

VALIDATION_IDS = {
    "pubmed": "12345678"
}



def write(message: Dict[str, Any]) -> None:
    print(json.dumps(message, separators=(",", ":"), ensure_ascii=False), flush=True)


def error(message_id: str, code: str, message: str) -> Dict[str, Any]:
    return {"id": message_id, "type": "error", "error": {"code": code, "message": message}}


def request_params(request: Dict[str, Any]) -> Dict[str, Any]:
    params = request.get("params")
    return params if isinstance(params, dict) else {}


def max_results(request: Dict[str, Any], default: int = 5, ceiling: int = 25) -> int:
    params = request_params(request)
    value = request.get("maxResults") or request.get("max_results") or params.get("limit") or params.get("retmax") or default
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
    for key in ("id", "pmid", "paperId", "paper_id", "doi", "arxiv"):
        value = request.get(key) if key in request else params.get(key)
        if isinstance(value, str) and value.strip():
            return value.strip()
    result = request.get("result")
    if isinstance(result, dict):
        for key in ("id", "accession", "pmid", "paperId", "paper_id", "doi", "url"):
            value = result.get(key)
            if isinstance(value, str) and value.strip():
                return value.strip()
        metadata = result.get("metadata")
        if isinstance(metadata, dict):
            for key in ("pmid", "pubmed_id", "paperId", "paper_id", "doi", "arxiv_id"):
                value = metadata.get(key)
                if isinstance(value, str) and value.strip():
                    return value.strip()
    url = request.get("url")
    if isinstance(url, str) and url.strip():
        return url.strip()
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

def semantic_scholar_headers(request: Dict[str, Any]) -> Dict[str, str]:
    key = credentials(request).get("semantic_scholar_api_key")
    return {"x-api-key": key} if key else {}


def validation_item(source: str, operation: str) -> Dict[str, Any]:
    accession = VALIDATION_IDS.get(source, f"{source}-validation")
    return {
        "id": accession,
        "accession": accession,
        "title": f"Validation {source} {operation} result",
        "url": f"https://example.test/{source}/{urllib.parse.quote(accession)}",
        "snippet": "Offline validation result from public-literature-sources plugin.",
        "content": "This fixture response is returned only for Omiga validation smoke calls.",
        "metadata": {"source": source, "validation": True},
        "raw": {"validation": True},
    }


def validation_result(message_id: str, request: Dict[str, Any]) -> Dict[str, Any]:
    operation = request.get("operation", "search")
    source = request.get("source", "pubmed")
    response: Dict[str, Any] = {
        "ok": True,
        "operation": operation,
        "category": request.get("category", "literature"),
        "source": source,
        "effectiveSource": source,
        "notes": ["offline validation response"],
        "raw": {"validation": True},
        "total": 1,
    }
    if operation == "fetch":
        response["items"] = []
        response["detail"] = validation_item(source, operation)
    else:
        response["items"] = [validation_item(source, operation)]
    return {"id": message_id, "type": "result", "response": response}



def normalize_pmid(value: str) -> str:
    value = (value or "").strip()
    match = re.search(r"pubmed\.ncbi\.nlm\.nih\.gov/(\d+)", value, re.IGNORECASE)
    if match:
        return match.group(1)
    match = re.search(r"\bPMID[:\s]*(\d+)\b", value, re.IGNORECASE)
    if match:
        return match.group(1)
    match = re.search(r"\b\d{4,}\b", value)
    return match.group(0) if match else value


def pubmed_esearch(request: Dict[str, Any], term: str) -> List[str]:
    encoded = urllib.parse.urlencode({"db": "pubmed", "term": term, "retmode": "json", "retmax": str(max_results(request))})
    data = urlopen_json(with_ncbi_credentials(f"{NCBI_EUTILS}/esearch.fcgi?{encoded}", request))
    ids = data.get("esearchresult", {}).get("idlist", [])
    return [str(value) for value in ids]


def pubmed_esummary(request: Dict[str, Any], ids: Iterable[str]) -> List[Dict[str, Any]]:
    ids = [str(value) for value in ids if str(value).strip()]
    if not ids:
        return []
    encoded = urllib.parse.urlencode({"db": "pubmed", "id": ",".join(ids), "retmode": "json"})
    data = urlopen_json(with_ncbi_credentials(f"{NCBI_EUTILS}/esummary.fcgi?{encoded}", request))
    result = data.get("result", {}) if isinstance(data, dict) else {}
    items = []
    for pmid in result.get("uids", ids):
        record = result.get(str(pmid), {}) if isinstance(result, dict) else {}
        items.append(pubmed_item(str(pmid), record))
    return items


def pubmed_item(pmid: str, record: Dict[str, Any]) -> Dict[str, Any]:
    title = str(record.get("title") or f"PubMed {pmid}")
    authors = record.get("authors") if isinstance(record.get("authors"), list) else []
    author_names = [str(a.get("name")) for a in authors if isinstance(a, dict) and a.get("name")]
    journal = str(record.get("fulljournalname") or record.get("source") or "")
    pubdate = str(record.get("pubdate") or record.get("epubdate") or "")
    snippet = " · ".join(part for part in [journal, pubdate, ", ".join(author_names[:3])] if part)
    return {
        "id": pmid,
        "accession": pmid,
        "title": title,
        "url": f"https://pubmed.ncbi.nlm.nih.gov/{pmid}/",
        "snippet": snippet[:500],
        "content": json.dumps(record, ensure_ascii=False, indent=2)[:20000],
        "metadata": {"source": "pubmed", "pmid": pmid, "journal": journal, "pubdate": pubdate, "authors": author_names},
        "raw": record,
    }


def handle_pubmed(message_id: str, request: Dict[str, Any]) -> Dict[str, Any]:
    operation = request.get("operation", "search")
    if operation in ("search", "query"):
        term = query_text(request)
        if not term:
            return error(message_id, "missing_query", "PubMed search/query requires query text")
        items = pubmed_esummary(request, pubmed_esearch(request, term))
        response = base_response(request, "pubmed", operation, items=items, total=len(items))
        return {"id": message_id, "type": "result", "response": response}
    if operation == "fetch":
        pmid = normalize_pmid(identifier_text(request))
        if not pmid:
            return error(message_id, "missing_identifier", "PubMed fetch requires PMID, URL, or prior result")
        items = pubmed_esummary(request, [pmid])
        if not items:
            return error(message_id, "not_found", f"PubMed record not found: {pmid}")
        response = base_response(request, "pubmed", operation, items=[], detail=items[0], total=1)
        return {"id": message_id, "type": "result", "response": response}
    return error(message_id, "unsupported_operation", f"PubMed does not support operation {operation}")



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
        "category": request.get("category", "literature"),
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
        if source == "pubmed":
            return handle_pubmed(message_id, request)
        return error(message_id, "unknown_source", f"unknown literature source: {source}")
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
