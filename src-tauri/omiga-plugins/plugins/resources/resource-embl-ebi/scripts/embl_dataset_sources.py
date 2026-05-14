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
    {"category": 'dataset', "id": 'ena', "capabilities": ["search", "query", "fetch"]},
    {"category": 'dataset', "id": 'ena_run', "capabilities": ["search", "query", "fetch"]},
    {"category": 'dataset', "id": 'ena_experiment', "capabilities": ["search", "query", "fetch"]},
    {"category": 'dataset', "id": 'ena_sample', "capabilities": ["search", "query", "fetch"]},
    {"category": 'dataset', "id": 'ena_analysis', "capabilities": ["search", "query", "fetch"]},
    {"category": 'dataset', "id": 'ena_assembly', "capabilities": ["search", "query", "fetch"]},
    {"category": 'dataset', "id": 'ena_sequence', "capabilities": ["search", "query", "fetch"]},
    {"category": 'dataset', "id": 'arrayexpress', "capabilities": ["search", "query", "fetch"]},
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
NCBI_DATASETS = "https://api.ncbi.nlm.nih.gov/datasets/v2"
BIOSTUDIES = "https://www.ebi.ac.uk/biostudies/api/v1"
GTEX = "https://gtexportal.org/api/v2"
CBIOPORTAL = "https://www.cbioportal.org/api"
ENA_PORTAL = "https://www.ebi.ac.uk/ena/portal/api/search"
ENA_BROWSER_XML = "https://www.ebi.ac.uk/ena/browser/api/xml"
USER_AGENT = "Omiga public-dataset-sources retrieval plugin/0.1"

VALIDATION_ACCESSIONS = {
    "ena": "PRJEB000001",
    "ena_run": "ERR000001",
    "ena_experiment": "ERX000001",
    "ena_sample": "ERS000001",
    "ena_analysis": "ERZ000001",
    "ena_assembly": "GCA_000001405.29",
    "ena_sequence": "AB000001",
    "arrayexpress": "E-MTAB-0000"
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



ENA_RESULT_BY_SOURCE = {
    "ena": "read_study",
    "ena_run": "read_run",
    "ena_experiment": "read_experiment",
    "ena_sample": "sample",
    "ena_analysis": "analysis",
    "ena_assembly": "assembly",
    "ena_sequence": "sequence",
}

ENA_FIELDS_BY_SOURCE = {
    "ena": "study_accession,secondary_study_accession,study_title,description,study_alias,center_name,tax_id,scientific_name,first_public,last_updated",
    "ena_run": "run_accession,experiment_accession,sample_accession,study_accession,secondary_study_accession,scientific_name,instrument_platform,instrument_model,library_strategy,library_source,first_public,last_updated,fastq_ftp,submitted_ftp,sra_ftp",
    "ena_experiment": "experiment_accession,study_accession,sample_accession,experiment_title,experiment_alias,scientific_name,instrument_platform,instrument_model,library_strategy,library_source,first_public,last_updated",
    "ena_sample": "sample_accession,secondary_sample_accession,sample_alias,scientific_name,tax_id,description,country,collection_date,host,host_tax_id,first_public,last_updated",
    "ena_analysis": "analysis_accession,study_accession,sample_accession,analysis_title,analysis_description,analysis_alias,analysis_type,assembly_type,description,scientific_name,first_public,last_updated,submitted_ftp,generated_ftp",
    "ena_assembly": "assembly_accession,scientific_name,tax_id,assembly_name,assembly_title,assembly_level,description,last_updated",
    "ena_sequence": "accession,description,scientific_name,tax_id,specimen_voucher,bio_material,first_public,last_updated",
}

ENA_SIMPLE_FIELDS = {
    "ena": ("study_title", "description"),
    "ena_run": ("description", "scientific_name", "study_title"),
    "ena_experiment": ("experiment_title", "description", "scientific_name"),
    "ena_sample": ("description", "scientific_name", "sample_alias"),
    "ena_analysis": ("analysis_title", "analysis_description", "description", "analysis_type", "scientific_name"),
    "ena_assembly": ("assembly_name", "assembly_title", "description", "scientific_name"),
    "ena_sequence": ("description", "scientific_name"),
}

ENA_ACCESSION_FIELDS = {
    "ena": ("study_accession", "secondary_study_accession", "accession"),
    "ena_run": ("run_accession", "accession"),
    "ena_experiment": ("experiment_accession", "accession"),
    "ena_sample": ("sample_accession", "secondary_sample_accession", "accession"),
    "ena_analysis": ("analysis_accession", "accession"),
    "ena_assembly": ("assembly_accession", "accession"),
    "ena_sequence": ("accession",),
}


def normalize_ena_accession(value: str) -> str:
    value = (value or "").strip().rstrip("/")
    if not value:
        return ""
    if "ebi.ac.uk/ena/browser/view/" in value:
        return urllib.parse.unquote(value.rsplit("/view/", 1)[-1].split("?", 1)[0].split("#", 1)[0])
    match = re.search(r"\b((?:PRJ|ERP|SRP|DRP|ERX|SRX|DRX|ERR|SRR|DRR|ERS|SRS|DRS|ERZ|SRZ|DRZ)[A-Z]*\d+|GC[AF]_\d+(?:\.\d+)?|[A-Z]{1,4}\d{5,}(?:\.\d+)?)\b", value, re.IGNORECASE)
    return match.group(1).upper() if match else value


def ena_escape(value: str) -> str:
    return value.replace("\\", "\\\\").replace('"', '\\"')


def ena_query(source: str, term: str, accession: bool = False) -> str:
    escaped = ena_escape(term)
    if accession:
        fields = ENA_ACCESSION_FIELDS.get(source, ("accession",))
        return " OR ".join(f'{field}="{escaped}"' for field in fields)
    lower = term.lower()
    if "=" in term or " and " in lower or " or " in lower or "tax_" in lower or "country" in lower:
        return term
    fields = ENA_SIMPLE_FIELDS.get(source, ("description",))
    return " OR ".join(f'{field}="*{escaped}*"' for field in fields)


def ena_search_json(source: str, query: str, request: Dict[str, Any], limit: Optional[int] = None) -> Any:
    params = {
        "result": ENA_RESULT_BY_SOURCE[source],
        "query": query,
        "fields": ENA_FIELDS_BY_SOURCE[source],
        "format": "json",
        "limit": str(limit or max_results(request)),
    }
    return urlopen_json(f"{ENA_PORTAL}?{urllib.parse.urlencode(params)}")


def split_ena_files(value: Any) -> List[str]:
    if not isinstance(value, str) or not value.strip():
        return []
    files = []
    for item in value.split(";"):
        item = item.strip()
        if not item:
            continue
        if item.startswith(("ftp://", "http://", "https://")):
            files.append(item)
        elif item.startswith("ftp."):
            files.append(f"ftp://{item}")
        else:
            files.append(item)
    return files


def ena_item(source: str, record: Dict[str, Any], fallback_id: str = "") -> Dict[str, Any]:
    accession = first_string(record, *ENA_ACCESSION_FIELDS.get(source, ("accession",))) or fallback_id
    title = first_string(
        record,
        "study_title",
        "experiment_title",
        "sample_alias",
        "analysis_title",
        "assembly_name",
        "assembly_title",
        "description",
        "accession",
    ) or accession
    summary = first_string(record, "description", "analysis_description", "library_strategy", "scientific_name", "assembly_level")
    files: List[str] = []
    for key in ("fastq_ftp", "submitted_ftp", "sra_ftp", "generated_ftp"):
        files.extend(split_ena_files(record.get(key)))
    return {
        "id": accession,
        "accession": accession,
        "title": title,
        "url": f"https://www.ebi.ac.uk/ena/browser/view/{urllib.parse.quote(accession)}",
        "snippet": summary[:500],
        "content": json.dumps(record, ensure_ascii=False, indent=2)[:20000],
        "metadata": {
            "source": source,
            "organism": first_string(record, "scientific_name"),
            "platform": first_string(record, "instrument_platform", "instrument_model"),
            "files": files,
        },
        "raw": record,
    }


def ena_xml_fallback(source: str, accession: str) -> Optional[Dict[str, Any]]:
    req = urllib.request.Request(f"{ENA_BROWSER_XML}/{urllib.parse.quote(accession)}", headers={"User-Agent": USER_AGENT, "Accept": "application/xml,text/xml,*/*"})
    try:
        with urllib.request.urlopen(req, timeout=25) as response:
            xml = response.read().decode("utf-8", errors="replace")
    except Exception:
        return None
    title_match = re.search(r"<(?:STUDY_TITLE|TITLE|DESCRIPTION)>(.*?)</(?:STUDY_TITLE|TITLE|DESCRIPTION)>", xml, re.DOTALL | re.IGNORECASE)
    summary_match = re.search(r"<(?:STUDY_ABSTRACT|DESCRIPTION)>(.*?)</(?:STUDY_ABSTRACT|DESCRIPTION)>", xml, re.DOTALL | re.IGNORECASE)
    title = re.sub(r"<[^>]+>", " ", title_match.group(1)).strip() if title_match else accession
    summary = re.sub(r"<[^>]+>", " ", summary_match.group(1)).strip() if summary_match else ""
    return ena_item(source, {"accession": accession, "description": summary, "title": title, "xml": xml[:20000]}, accession)


def handle_ena(message_id: str, request: Dict[str, Any], source: str) -> Dict[str, Any]:
    operation = request.get("operation", "search")
    if operation in ("search", "query"):
        term = query_text(request)
        if not term:
            return error(message_id, "missing_query", f"{source} search/query requires query text")
        data = ena_search_json(source, ena_query(source, term), request)
        items = [ena_item(source, item) for item in data_items(data) if isinstance(item, dict)]
        response = base_response(request, source, operation, items=items, total=len(items))
        return {"id": message_id, "type": "result", "response": response}
    if operation == "fetch":
        accession = normalize_ena_accession(identifier_text(request))
        if not accession:
            return error(message_id, "missing_identifier", f"{source} fetch requires an ENA accession or URL")
        data = ena_search_json(source, ena_query(source, accession, accession=True), request, 1)
        items = [ena_item(source, item, accession) for item in data_items(data) if isinstance(item, dict)]
        if not items:
            fallback = ena_xml_fallback(source, accession)
            items = [fallback] if fallback else []
        if not items:
            return error(message_id, "not_found", f"ENA record not found: {accession}")
        response = base_response(request, source, operation, items=[], detail=items[0], total=1)
        return {"id": message_id, "type": "result", "response": response}
    return error(message_id, "unsupported_operation", f"{source} does not support operation {operation}")



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
        if source in ENA_RESULT_BY_SOURCE:
            return handle_ena(message_id, request, source)
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
