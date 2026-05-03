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
    {"category": "dataset", "id": "ncbi_datasets", "capabilities": ["search", "query", "fetch"]},
    {"category": "dataset", "id": "gtex", "capabilities": ["search", "query", "fetch"]},
    {"category": "dataset", "id": "cbioportal", "capabilities": ["search", "query", "fetch"]},
]
NCBI_EUTILS = "https://eutils.ncbi.nlm.nih.gov/entrez/eutils"
NCBI_DATASETS = "https://api.ncbi.nlm.nih.gov/datasets/v2"
BIOSTUDIES = "https://www.ebi.ac.uk/biostudies/api/v1"
GTEX = "https://gtexportal.org/api/v2"
CBIOPORTAL = "https://www.cbioportal.org/api"
USER_AGENT = "Omiga public-dataset-sources retrieval plugin/0.1"

VALIDATION_ACCESSIONS = {
    "biosample": "SAMN00000001",
    "arrayexpress": "E-MTAB-0000",
    "ncbi_datasets": "GCF_000001405.40",
    "gtex": "ENSG00000012048.21",
    "cbioportal": "brca_tcga",
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


def normalize_gtex_identifier(value: str) -> str:
    value = (value or "").strip().rstrip("/")
    if not value:
        return ""
    match = re.search(r"(ENS[A-Z]*G\d+(?:\.\d+)?)", value, re.IGNORECASE)
    if match:
        return match.group(1).upper()
    if "gtexportal.org" in value and "/gene/" in value:
        return urllib.parse.unquote(value.rsplit("/gene/", 1)[-1].split("?", 1)[0].split("#", 1)[0])
    return value


def gtex_json(endpoint: str, params: Dict[str, Any]) -> Any:
    url = f"{GTEX}/{endpoint.lstrip('/')}?{urllib.parse.urlencode(params, doseq=True)}"
    return urlopen_json(url)


def gtex_gene_items(request: Dict[str, Any], gene_id: str, limit: Optional[int] = None) -> List[Dict[str, Any]]:
    params = request_params(request)
    data = gtex_json(
        "reference/gene",
        {
            "geneId": normalize_gtex_identifier(gene_id),
            "gencodeVersion": params.get("gencodeVersion") or params.get("gencode_version") or "v26",
            "genomeBuild": params.get("genomeBuild") or params.get("genome_build") or "GRCh38/hg38",
            "page": params.get("page") or 0,
            "itemsPerPage": limit or max_results(request),
        },
    )
    return [gtex_item(item) for item in data_items(data) if isinstance(item, dict)]


def gtex_item(record: Dict[str, Any]) -> Dict[str, Any]:
    gencode_id = first_string(record, "gencodeId", "gencode_id", "geneId", "gene_id")
    symbol = first_string(record, "geneSymbol", "gene_symbol") or gencode_id
    description = first_string(record, "description", "geneDescription")
    title = f"{symbol} ({gencode_id})" if symbol and symbol != gencode_id else gencode_id
    chromosome = first_string(record, "chromosome")
    return {
        "id": gencode_id,
        "accession": gencode_id,
        "title": title,
        "url": f"https://gtexportal.org/home/gene/{urllib.parse.quote(gencode_id)}",
        "snippet": " | ".join(part for part in (chromosome, description) if part)[:500],
        "content": json.dumps(record, ensure_ascii=False, indent=2)[:20000],
        "metadata": {"source": "gtex", "gene_symbol": symbol, "chromosome": chromosome},
        "raw": record,
    }


def handle_gtex(message_id: str, request: Dict[str, Any]) -> Dict[str, Any]:
    operation = request.get("operation", "search")
    if operation in ("search", "query"):
        term = query_text(request)
        if not term:
            return error(message_id, "missing_query", "GTEx search/query requires a gene symbol or GENCODE ID")
        items = gtex_gene_items(request, term)
        response = base_response(request, "gtex", operation, items=items, total=len(items))
        return {"id": message_id, "type": "result", "response": response}
    if operation == "fetch":
        identifier = normalize_gtex_identifier(identifier_text(request))
        if not identifier:
            return error(message_id, "missing_identifier", "GTEx fetch requires a gene symbol, GENCODE ID, or GTEx gene URL")
        items = gtex_gene_items(request, identifier, 1)
        if not items:
            return error(message_id, "not_found", f"GTEx gene not found: {identifier}")
        response = base_response(request, "gtex", operation, items=[], detail=items[0], total=1)
        return {"id": message_id, "type": "result", "response": response}
    return error(message_id, "unsupported_operation", f"GTEx does not support operation {operation}")


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
        if source == "ncbi_datasets":
            return handle_ncbi_datasets(message_id, request)
        if source == "gtex":
            return handle_gtex(message_id, request)
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
