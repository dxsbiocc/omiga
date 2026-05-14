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
    {"category": 'dataset', "id": 'cbioportal', "capabilities": ["search", "query", "fetch"]},
]

PLUGIN_NAME = os.environ.get("OMIGA_RETRIEVAL_PLUGIN_NAME", 'retrieval-dataset-cbioportal')


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
    "cbioportal": "brca_tcga"
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



def normalize_cbioportal_study_id(value: str) -> str:
    value = (value or "").strip().rstrip("/")
    if not value:
        return ""
    if "cbioportal" in value:
        parsed = urllib.parse.urlparse(value)
        query = urllib.parse.parse_qs(parsed.query)
        for key in ("id", "studyId", "study_id"):
            if query.get(key):
                return query[key][0].strip()
        path_id = parsed.path.rstrip("/").rsplit("/", 1)[-1]
        if path_id and path_id != "summary":
            return urllib.parse.unquote(path_id)
    return value


def cbioportal_item(record: Dict[str, Any]) -> Dict[str, Any]:
    study_id = first_string(record, "studyId", "study_id", "id")
    title = first_string(record, "name", "studyName", "title") or study_id
    description = first_string(record, "description", "shortDescription", "summary")
    cancer_type = first_string(record, "cancerTypeId", "cancer_type_id") or nested_string(record, "cancerType", "name")
    return {
        "id": study_id,
        "accession": study_id,
        "title": title,
        "url": f"https://www.cbioportal.org/study/summary?id={urllib.parse.quote(study_id)}",
        "snippet": description[:500],
        "content": json.dumps(record, ensure_ascii=False, indent=2)[:20000],
        "metadata": {
            "source": "cbioportal",
            "cancer_type": cancer_type,
            "sample_count": record.get("allSampleCount") or record.get("sampleCount"),
            "pmid": record.get("pmid") or record.get("PMID"),
        },
        "raw": record,
    }


def cbioportal_json(path: str, params: Dict[str, Any]) -> Any:
    url = f"{CBIOPORTAL}/{path.lstrip('/')}"
    if params:
        url = f"{url}?{urllib.parse.urlencode(params)}"
    return urlopen_json(url)


def handle_cbioportal(message_id: str, request: Dict[str, Any]) -> Dict[str, Any]:
    operation = request.get("operation", "search")
    if operation in ("search", "query"):
        term = query_text(request)
        if not term:
            return error(message_id, "missing_query", "cBioPortal search/query requires query text")
        data = cbioportal_json(
            "studies",
            {
                "projection": "SUMMARY",
                "keyword": term,
                "pageNumber": 0,
                "pageSize": max_results(request),
            },
        )
        items = [cbioportal_item(item) for item in data_items(data) if isinstance(item, dict)]
        response = base_response(request, "cbioportal", operation, items=items, total=len(items))
        return {"id": message_id, "type": "result", "response": response}
    if operation == "fetch":
        study_id = normalize_cbioportal_study_id(identifier_text(request))
        if not study_id:
            return error(message_id, "missing_identifier", "cBioPortal fetch requires a study id or URL")
        data = cbioportal_json(f"studies/{urllib.parse.quote(study_id)}", {"projection": "DETAILED"})
        if not isinstance(data, dict):
            return error(message_id, "not_found", f"cBioPortal study not found: {study_id}")
        detail = cbioportal_item(data)
        response = base_response(request, "cbioportal", operation, items=[], detail=detail, total=1)
        return {"id": message_id, "type": "result", "response": response}
    return error(message_id, "unsupported_operation", f"cBioPortal does not support operation {operation}")



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
        if source == "cbioportal":
            return handle_cbioportal(message_id, request)
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
