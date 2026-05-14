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
    {"category": 'knowledge', "id": 'ensembl', "capabilities": ["search", "query", "fetch"]},
]

PLUGIN_NAME = os.environ.get("OMIGA_RETRIEVAL_PLUGIN_NAME", 'resource-embl-ebi')


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
    "ensembl": "ENSG00000141510"
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



def normalize_species(value: Optional[str]) -> str:
    raw = (value or "homo_sapiens").strip().lower().replace(" ", "_").replace("-", "_")
    aliases = {"human": "homo_sapiens", "h_sapiens": "homo_sapiens", "mouse": "mus_musculus", "m_musculus": "mus_musculus"}
    return aliases.get(raw, raw)


def normalize_object_type(value: Optional[str]) -> str:
    raw = (value or "gene").strip().lower().replace(" ", "_").replace("-", "_")
    return "gene" if raw in {"", "genes", "symbol"} else raw


def encode_segment(value: str) -> str:
    return urllib.parse.quote(value, safe="")


def ensembl_get(path: str, params: Optional[Dict[str, str]] = None) -> Any:
    query = f"?{urllib.parse.urlencode(params)}" if params else ""
    data, _ = urlopen_json(f"{ENSEMBL}{path}{query}", headers={"Accept": "application/json"})
    return data


def normalize_ensembl_identifier(value: str) -> str:
    value = (value or "").strip().rstrip("/")
    if not value:
        return ""
    if value.startswith("http://") or value.startswith("https://"):
        parsed = urllib.parse.urlparse(value)
        if "ensembl.org" in parsed.netloc:
            match = re.search(r"/(?:Gene|Transcript|Variation)/Summary\?.*?[?&]?(?:g|t|v)=([^&#]+)", value)
            if match:
                return urllib.parse.unquote(match.group(1))
            tail = parsed.path.rstrip("/").rsplit("/", 1)[-1]
            if tail:
                return urllib.parse.unquote(tail)
    stable = re.search(r"\bENS[A-Z]*\d+(?:\.\d+)?\b", value, re.IGNORECASE)
    if stable:
        return stable.group(0).split(".", 1)[0]
    variant = re.search(r"\brs\d+\b", value, re.IGNORECASE)
    if variant:
        return variant.group(0)
    return value


def ensembl_url(record: Dict[str, Any]) -> str:
    identifier = str(record.get("id") or record.get("name") or "")
    species = str(record.get("species") or "Homo_sapiens")
    if identifier.lower().startswith("rs"):
        return f"https://www.ensembl.org/{species}/Variation/Explore?v={urllib.parse.quote(identifier)}"
    return f"https://www.ensembl.org/{species}/Gene/Summary?g={urllib.parse.quote(identifier)}"


def ensembl_item(record: Dict[str, Any]) -> Dict[str, Any]:
    identifier = first_string(record, "id", "name")
    display_name = first_string(record, "display_name", "name") or identifier
    record_type = first_string(record, "object_type", "var_class") or ("Variant" if identifier.lower().startswith("rs") else "gene")
    description = first_string(record, "description", "source")
    species = first_string(record, "species")
    biotype = first_string(record, "biotype", "var_class")
    seq_region = first_string(record, "seq_region_name")
    start = record.get("start")
    end = record.get("end")
    location = ""
    if seq_region and start and end:
        location = f"{seq_region}:{start}-{end}"
    title = f"{display_name} — {description}" if description else display_name
    snippet = " · ".join(str(part) for part in [record_type, species, biotype, location] if part)
    return {
        "id": identifier,
        "accession": identifier,
        "title": title,
        "url": ensembl_url(record),
        "favicon": ENSEMBL_FAVICON,
        "snippet": snippet[:500],
        "content": json.dumps(record, ensure_ascii=False, indent=2)[:20000],
        "metadata": {
            "source": "ensembl",
            "id": identifier,
            "record_type": record_type,
            "display_name": display_name,
            "description": description,
            "species": species,
            "biotype": biotype,
            "seq_region_name": seq_region,
            "start": start,
            "end": end,
            "strand": record.get("strand"),
            "assembly_name": record.get("assembly_name"),
            "source_specific": record,
        },
        "raw": record,
    }


def ensembl_fetch_record(identifier: str, species: str) -> Dict[str, Any]:
    identifier = normalize_ensembl_identifier(identifier)
    if not identifier:
        raise RuntimeError("empty Ensembl identifier")
    if re.match(r"^rs\d+$", identifier, re.IGNORECASE):
        return ensembl_get(f"/variation/{encode_segment(species)}/{encode_segment(identifier)}")
    if re.match(r"^ENS[A-Z]*\d+", identifier, re.IGNORECASE):
        return ensembl_get(f"/lookup/id/{encode_segment(identifier)}", {"expand": "0"})
    return ensembl_get(f"/lookup/symbol/{encode_segment(species)}/{encode_segment(identifier)}", {"expand": "0"})


def handle_ensembl(message_id: str, request: Dict[str, Any]) -> Dict[str, Any]:
    operation = request.get("operation", "query")
    species = normalize_species(str_param(request, ("species", "organism")))
    object_type = normalize_object_type(str_param(request, ("object_type", "type")))
    if operation in ("search", "query"):
        term = query_text(request)
        if not term:
            return error(message_id, "missing_query", "Ensembl search/query requires query text")
        if re.match(r"^(ENS[A-Z]*\d+|rs\d+)", normalize_ensembl_identifier(term), re.IGNORECASE):
            items = [ensembl_item(ensembl_fetch_record(term, species))]
        else:
            xrefs = ensembl_get(f"/xrefs/symbol/{encode_segment(species)}/{encode_segment(term)}", {"object_type": object_type})
            ids: List[str] = []
            if isinstance(xrefs, list):
                for item in xrefs:
                    if not isinstance(item, dict):
                        continue
                    if item.get("id") and str(item.get("id")) not in ids:
                        ids.append(str(item.get("id")))
                    if len(ids) >= max_results(request):
                        break
            records = []
            if ids:
                for identifier in ids:
                    try:
                        records.append(ensembl_fetch_record(identifier, species))
                    except Exception:
                        continue
            else:
                records.append(ensembl_fetch_record(term, species))
            items = [ensembl_item(record) for record in records]
        response = base_response(request, "ensembl", operation, items=items, total=len(items), raw={"species": species, "object_type": object_type})
        return {"id": message_id, "type": "result", "response": response}
    if operation == "fetch":
        identifier = identifier_text(request)
        if not identifier:
            return error(message_id, "missing_identifier", "Ensembl fetch requires stable ID, rsID, symbol, URL, or prior result")
        detail = ensembl_item(ensembl_fetch_record(identifier, species))
        response = base_response(request, "ensembl", operation, items=[], detail=detail, total=1)
        return {"id": message_id, "type": "result", "response": response}
    return error(message_id, "unsupported_operation", f"Ensembl does not support operation {operation}")




def handle_execute(message: Dict[str, Any]) -> Dict[str, Any]:
    message_id = str(message.get("id", "execute"))
    request = message.get("request") if isinstance(message.get("request"), dict) else {}
    source = str(request.get("source", "")).strip().lower().replace("-", "_")
    if not source_is_allowed(source):
        return error(message_id, "unknown_source", f"source is not served by this plugin: {source}")
    if is_validation(request):
        return validation_result(message_id, request)
    try:
        if source in {"ensembl", "ensembl_gene", "ensembl_transcript", "variant", "variation"}:
            return handle_ensembl(message_id, request)
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
