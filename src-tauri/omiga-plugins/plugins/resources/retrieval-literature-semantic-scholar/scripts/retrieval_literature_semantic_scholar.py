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
    {"category": 'literature', "id": 'semantic_scholar', "capabilities": ["search", "query", "fetch"]},
]

PLUGIN_NAME = os.environ.get("OMIGA_RETRIEVAL_PLUGIN_NAME", 'retrieval-literature-semantic-scholar')


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
    "semantic_scholar": "validation-paper-id"
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



def normalize_s2_identifier(value: str) -> str:
    value = (value or "").strip().rstrip("/")
    if not value:
        return ""
    if value.startswith("https://") or value.startswith("http://"):
        parsed = urllib.parse.urlparse(value)
        if "semanticscholar.org" in parsed.netloc and "/paper/" in parsed.path:
            return urllib.parse.unquote(parsed.path.rstrip("/").rsplit("/", 1)[-1])
        if "doi.org" in parsed.netloc:
            return f"DOI:{parsed.path.strip('/')}"
        if "arxiv.org" in parsed.netloc:
            return f"ARXIV:{parsed.path.rstrip('/').rsplit('/', 1)[-1]}"
    if re.match(r"10\.\d{4,9}/", value, re.IGNORECASE):
        return f"DOI:{value}"
    if re.match(r"\d{4}\.\d{4,5}(?:v\d+)?", value, re.IGNORECASE):
        return f"ARXIV:{value}"
    return value


def s2_author_names(record: Dict[str, Any]) -> List[str]:
    authors = record.get("authors") if isinstance(record.get("authors"), list) else []
    return [str(a.get("name")) for a in authors if isinstance(a, dict) and a.get("name")]


def s2_external_id(record: Dict[str, Any], key: str) -> Optional[str]:
    external = record.get("externalIds")
    if isinstance(external, dict):
        value = external.get(key)
        if value:
            return str(value)
    return None


def semantic_scholar_item(record: Dict[str, Any]) -> Dict[str, Any]:
    paper_id = str(record.get("paperId") or "")
    title = str(record.get("title") or paper_id or "Semantic Scholar paper")
    authors = s2_author_names(record)
    year = record.get("year")
    venue = str(record.get("venue") or "")
    abstract = str(record.get("abstract") or "")
    url = str(record.get("url") or (f"https://www.semanticscholar.org/paper/{paper_id}" if paper_id else "https://www.semanticscholar.org"))
    doi = s2_external_id(record, "DOI")
    pubmed_id = s2_external_id(record, "PubMed")
    arxiv_id = s2_external_id(record, "ArXiv")
    snippet = " · ".join(str(part) for part in [venue, year, ", ".join(authors[:3])] if part)
    if abstract:
        snippet = f"{snippet} — {abstract}" if snippet else abstract
    return {
        "id": paper_id or doi or pubmed_id or title,
        "accession": paper_id or doi or pubmed_id,
        "title": title,
        "url": url,
        "snippet": snippet[:500],
        "content": json.dumps(record, ensure_ascii=False, indent=2)[:20000],
        "metadata": {"source": "semantic_scholar", "paper_id": paper_id, "doi": doi, "pubmed_id": pubmed_id, "arxiv_id": arxiv_id, "authors": authors, "year": year, "venue": venue},
        "raw": record,
    }


def s2_fields(request: Dict[str, Any]) -> str:
    fields = request_params(request).get("fields")
    return str(fields) if isinstance(fields, str) and fields.strip() else S2_FIELDS


def handle_semantic_scholar(message_id: str, request: Dict[str, Any]) -> Dict[str, Any]:
    operation = request.get("operation", "search")
    if operation in ("search", "query"):
        term = query_text(request)
        if not term:
            return error(message_id, "missing_query", "Semantic Scholar search/query requires query text")
        params = urllib.parse.urlencode({"query": term, "limit": str(max_results(request)), "fields": s2_fields(request)})
        data = urlopen_json(f"{SEMANTIC_SCHOLAR}/paper/search?{params}", headers=semantic_scholar_headers(request))
        records = data.get("data", []) if isinstance(data, dict) else []
        items = [semantic_scholar_item(item) for item in records if isinstance(item, dict)]
        total = data.get("total") if isinstance(data, dict) and isinstance(data.get("total"), int) else len(items)
        response = base_response(request, "semantic_scholar", operation, items=items, total=total)
        return {"id": message_id, "type": "result", "response": response}
    if operation == "fetch":
        identifier = normalize_s2_identifier(identifier_text(request))
        if not identifier:
            return error(message_id, "missing_identifier", "Semantic Scholar fetch requires paper id, DOI, arXiv id, URL, or prior result")
        encoded = urllib.parse.quote(identifier, safe=":/")
        params = urllib.parse.urlencode({"fields": s2_fields(request)})
        data = urlopen_json(f"{SEMANTIC_SCHOLAR}/paper/{encoded}?{params}", headers=semantic_scholar_headers(request))
        if not isinstance(data, dict):
            return error(message_id, "not_found", f"Semantic Scholar paper not found: {identifier}")
        detail = semantic_scholar_item(data)
        response = base_response(request, "semantic_scholar", operation, items=[], detail=detail, total=1)
        return {"id": message_id, "type": "result", "response": response}
    return error(message_id, "unsupported_operation", f"Semantic Scholar does not support operation {operation}")



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
        if source in {"semantic_scholar", "semanticscholar", "s2"}:
            return handle_semantic_scholar(message_id, request)
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
