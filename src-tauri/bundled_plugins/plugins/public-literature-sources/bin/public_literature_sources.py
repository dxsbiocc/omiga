#!/usr/bin/env python3
"""Public literature retrieval plugin for Omiga's local JSONL protocol."""

from __future__ import annotations

import json
import re
import sys
import urllib.error
import urllib.parse
import urllib.request
from typing import Any, Dict, Iterable, List, Optional

PROTOCOL_VERSION = 1
SOURCES = [
    {"category": "literature", "id": "pubmed", "capabilities": ["search", "query", "fetch"]},
    {"category": "literature", "id": "semantic_scholar", "capabilities": ["search", "query", "fetch"]},
]
NCBI_EUTILS = "https://eutils.ncbi.nlm.nih.gov/entrez/eutils"
SEMANTIC_SCHOLAR = "https://api.semanticscholar.org/graph/v1"
USER_AGENT = "Omiga public-literature-sources retrieval plugin/0.1"
S2_FIELDS = "paperId,externalIds,url,title,abstract,venue,year,authors,citationCount,influentialCitationCount,publicationDate,openAccessPdf,fieldsOfStudy"
VALIDATION_IDS = {"pubmed": "12345678", "semantic_scholar": "validation-paper-id"}


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
    request_headers = {"User-Agent": USER_AGENT, "Accept": "application/json"}
    if headers:
        request_headers.update(headers)
    req = urllib.request.Request(url, headers=request_headers)
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
        "notes": ["public-literature-sources plugin"],
        "raw": {"plugin": "public-literature-sources"},
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
        if source == "pubmed":
            return handle_pubmed(message_id, request)
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
