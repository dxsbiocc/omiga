#!/usr/bin/env python3
"""Public knowledge retrieval plugin for Omiga's local JSONL protocol."""

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
    {"category": 'knowledge', "id": 'uniprot', "capabilities": ["search", "query", "fetch"]},
]

PLUGIN_NAME = os.environ.get("OMIGA_RETRIEVAL_PLUGIN_NAME", 'retrieval-knowledge-uniprot')


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
ENSEMBL = "https://rest.ensembl.org"
UNIPROT = "https://rest.uniprot.org"
USER_AGENT = "Omiga public-knowledge-sources retrieval plugin/0.1"
NCBI_FAVICON = "https://www.ncbi.nlm.nih.gov/favicon.ico"
ENSEMBL_FAVICON = "https://www.ensembl.org/favicon.ico"
UNIPROT_FAVICON = "https://www.uniprot.org/favicon.ico"

VALIDATION_IDS = {
    "uniprot": "P04637"
}



def write(message: Dict[str, Any]) -> None:
    print(json.dumps(message, separators=(",", ":"), ensure_ascii=False), flush=True)


def error(message_id: str, code: str, message: str) -> Dict[str, Any]:
    return {"id": message_id, "type": "error", "error": {"code": code, "message": message}}


def request_params(request: Dict[str, Any]) -> Dict[str, Any]:
    params = request.get("params")
    return params if isinstance(params, dict) else {}


def credentials(request: Dict[str, Any]) -> Dict[str, str]:
    value = request.get("credentials")
    if not isinstance(value, dict):
        return {}
    return {str(k): str(v) for k, v in value.items() if str(v).strip()}


def max_results(request: Dict[str, Any], default: int = 5, ceiling: int = 25) -> int:
    params = request_params(request)
    value = request.get("maxResults") or request.get("max_results") or params.get("limit") or params.get("retmax") or params.get("size") or default
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
    return str_param(request, ("query", "term", "q", "text")) or ""


def identifier_text(request: Dict[str, Any]) -> str:
    for value in (str_param(request, ("id", "accession", "gene_id", "stable_id", "uid")),):
        if value:
            return value
    result = request.get("result")
    if isinstance(result, dict):
        for key in ("id", "accession", "gene_id", "stable_id", "uid", "url", "link"):
            value = result.get(key)
            if value is not None and str(value).strip():
                return str(value).strip()
        metadata = result.get("metadata")
        if isinstance(metadata, dict):
            for key in ("id", "accession", "gene_id", "stable_id", "ensembl_id", "uniprot", "uniprot_id"):
                value = metadata.get(key)
                if value is not None and str(value).strip():
                    return str(value).strip()
    url = request.get("url")
    if isinstance(url, str) and url.strip():
        return url.strip()
    return ""




def urlopen_text(url: str, timeout: int = 25, headers: Optional[Dict[str, str]] = None) -> Tuple[str, Dict[str, str]]:
    return fetch_text_with_headers(url, timeout=timeout, headers=headers, user_agent=USER_AGENT)


def urlopen_json(url: str, timeout: int = 25, headers: Optional[Dict[str, str]] = None) -> Tuple[Any, Dict[str, str]]:
    return fetch_json_with_headers(url, timeout=timeout, headers=headers, user_agent=USER_AGENT)


def with_ncbi_credentials(url: str, request: Dict[str, Any]) -> str:
    return with_query_credentials(
        url,
        credentials(request),
        api_key_key="pubmed_api_key",
        email_key="pubmed_email",
        tool_key="pubmed_tool_name",
        default_email="omiga@example.invalid",
        default_tool="omiga",
    )

def validation_item(source: str, operation: str) -> Dict[str, Any]:
    accession = VALIDATION_IDS.get(source, f"{source}-validation")
    return {
        "id": accession,
        "accession": accession,
        "title": f"Validation {source} {operation} result",
        "url": f"https://example.test/{source}/{urllib.parse.quote(accession)}",
        "snippet": "Offline validation result from public-knowledge-sources plugin.",
        "content": "This fixture response is returned only for Omiga validation smoke calls.",
        "metadata": {"source": source, "validation": True},
        "raw": {"validation": True},
    }


def validation_result(message_id: str, request: Dict[str, Any]) -> Dict[str, Any]:
    operation = request.get("operation", "query")
    source = request.get("source", "ncbi_gene")
    response: Dict[str, Any] = {
        "ok": True,
        "operation": operation,
        "category": request.get("category", "knowledge"),
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


def first_string(value: Any, *keys: str) -> str:
    if not isinstance(value, dict):
        return ""
    for key in keys:
        item = value.get(key)
        if item is not None and str(item).strip():
            return str(item).strip()
    return ""


def list_from_delimited(value: Any) -> List[str]:
    if isinstance(value, list):
        return [str(v).strip() for v in value if str(v).strip()]
    if value is None:
        return []
    return [part.strip() for part in re.split(r"[|,;]", str(value)) if part.strip()]



def build_uniprot_query(request: Dict[str, Any], term: str) -> str:
    query = term.strip()
    taxon_id = str_param(request, ("taxon_id", "taxid", "tax_id", "taxonomy_id"))
    organism = str_param(request, ("organism", "species"))
    reviewed = request_params(request).get("reviewed")
    if taxon_id and "organism_id:" not in query.lower() and "taxonomy_id:" not in query.lower():
        query = f"({query}) AND (organism_id:{taxon_id})"
    elif organism and "organism_name:" not in query.lower() and "organism_id:" not in query.lower():
        query = f"({query}) AND (organism_name:\"{organism.replace(chr(34), '')}\")"
    if reviewed is not None and "reviewed:" not in query.lower():
        if isinstance(reviewed, str):
            value = reviewed.strip().lower() in {"1", "true", "yes", "reviewed", "swiss_prot", "swissprot"}
        else:
            value = bool(reviewed)
        query = f"({query}) AND (reviewed:{str(value).lower()})"
    return query


def normalize_uniprot_accession(value: str) -> str:
    value = (value or "").strip().rstrip("/")
    if not value:
        return ""
    if value.startswith("http://") or value.startswith("https://"):
        parsed = urllib.parse.urlparse(value)
        tail = parsed.path.rstrip("/").rsplit("/", 1)[-1]
        if tail.endswith(".json"):
            tail = tail[:-5]
        return urllib.parse.unquote(tail)
    match = re.search(r"\b[A-Z0-9]{6,10}\b", value, re.IGNORECASE)
    return match.group(0).upper() if match else value


def nested_value(value: Any, path: Iterable[str]) -> Any:
    current = value
    for key in path:
        if not isinstance(current, dict):
            return None
        current = current.get(key)
    return current


def uniprot_full_name(value: Any) -> str:
    for path in (("recommendedName", "fullName", "value"), ("submissionNames",), ("alternativeNames",)):
        if path == ("submissionNames",) or path == ("alternativeNames",):
            items = value.get(path[0]) if isinstance(value, dict) else None
            if isinstance(items, list):
                for item in items:
                    name = nested_value(item, ("fullName", "value"))
                    if name:
                        return str(name)
        else:
            name = nested_value(value, path)
            if name:
                return str(name)
    return ""


def uniprot_gene_names(record: Dict[str, Any]) -> List[str]:
    out: List[str] = []
    genes = record.get("genes")
    if isinstance(genes, list):
        for gene in genes:
            if not isinstance(gene, dict):
                continue
            for key in ("geneName", "orderedLocusNames", "orfNames", "synonyms"):
                value = gene.get(key)
                values = value if isinstance(value, list) else [value]
                for item in values:
                    name = nested_value(item, ("value",)) if isinstance(item, dict) else None
                    if name and str(name) not in out:
                        out.append(str(name))
    return out


def uniprot_comment_text(record: Dict[str, Any], comment_type: str) -> List[str]:
    out: List[str] = []
    comments = record.get("comments")
    if isinstance(comments, list):
        for comment in comments:
            if not isinstance(comment, dict) or str(comment.get("commentType", "")).upper() != comment_type:
                continue
            texts = comment.get("texts")
            if isinstance(texts, list):
                for item in texts:
                    value = nested_value(item, ("value",)) if isinstance(item, dict) else None
                    if value and str(value) not in out:
                        out.append(str(value))
            note = comment.get("note")
            if note and str(note) not in out:
                out.append(str(note))
    return out


def uniprot_item(record: Dict[str, Any]) -> Dict[str, Any]:
    accession = first_string(record, "primaryAccession")
    entry_name = first_string(record, "uniProtkbId") or accession
    protein_name = uniprot_full_name(record.get("proteinDescription") or {}) or entry_name
    organism = record.get("organism") if isinstance(record.get("organism"), dict) else {}
    organism_name = first_string(organism, "scientificName", "commonName")
    taxon_id = organism.get("taxonId")
    sequence = record.get("sequence") if isinstance(record.get("sequence"), dict) else {}
    gene_names = uniprot_gene_names(record)
    function = "\n\n".join(uniprot_comment_text(record, "FUNCTION"))
    title = f"{protein_name} ({entry_name})" if protein_name != entry_name else entry_name
    snippet = " · ".join(str(part) for part in [organism_name, ", ".join(gene_names[:3]), function] if part)
    return {
        "id": accession,
        "accession": accession,
        "title": title,
        "url": f"https://www.uniprot.org/uniprotkb/{urllib.parse.quote(accession)}",
        "favicon": UNIPROT_FAVICON,
        "snippet": snippet[:500],
        "content": json.dumps(record, ensure_ascii=False, indent=2)[:20000],
        "metadata": {
            "source": "uniprot",
            "accession": accession,
            "entry_name": entry_name,
            "entry_type": record.get("entryType"),
            "protein_name": protein_name,
            "gene_names": gene_names,
            "organism": organism_name,
            "taxon_id": taxon_id,
            "length": sequence.get("length"),
            "mass": sequence.get("molWeight"),
            "function": function,
            "source_specific": record,
        },
        "raw": record,
    }


def handle_uniprot(message_id: str, request: Dict[str, Any]) -> Dict[str, Any]:
    operation = request.get("operation", "query")
    if operation in ("search", "query"):
        term = query_text(request)
        if not term:
            return error(message_id, "missing_query", "UniProt search/query requires query text")
        params = urllib.parse.urlencode({"query": build_uniprot_query(request, term), "format": "json", "size": str(max_results(request))})
        data, headers = urlopen_json(f"{UNIPROT}/uniprotkb/search?{params}")
        records = data.get("results", []) if isinstance(data, dict) else []
        items = [uniprot_item(record) for record in records if isinstance(record, dict)]
        total_header = headers.get("x-total-results")
        total = int(total_header) if total_header and total_header.isdigit() else len(items)
        response = base_response(request, "uniprot", operation, items=items, total=total)
        return {"id": message_id, "type": "result", "response": response}
    if operation == "fetch":
        accession = normalize_uniprot_accession(identifier_text(request))
        if not accession:
            return error(message_id, "missing_identifier", "UniProt fetch requires accession, URL, or prior result")
        data, _ = urlopen_json(f"{UNIPROT}/uniprotkb/{urllib.parse.quote(accession)}.json")
        if not isinstance(data, dict):
            return error(message_id, "not_found", f"UniProt record not found: {accession}")
        detail = uniprot_item(data)
        response = base_response(request, "uniprot", operation, items=[], detail=detail, total=1)
        return {"id": message_id, "type": "result", "response": response}
    return error(message_id, "unsupported_operation", f"UniProt does not support operation {operation}")




def handle_execute(message: Dict[str, Any]) -> Dict[str, Any]:
    message_id = str(message.get("id", "execute"))
    request = message.get("request") if isinstance(message.get("request"), dict) else {}
    source = str(request.get("source", "")).strip().lower().replace("-", "_")
    if not source_is_allowed(source):
        return error(message_id, "unknown_source", f"source is not served by this plugin: {source}")
    if is_validation(request):
        return validation_result(message_id, request)
    try:
        if source in {"uniprot", "uni_prot", "uniprotkb", "uniprot_kb", "protein", "proteins"}:
            return handle_uniprot(message_id, request)
        return error(message_id, "unknown_source", f"unknown knowledge source: {source}")
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
