#!/usr/bin/env python3
"""Public knowledge retrieval plugin for Omiga's local JSONL protocol."""

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
    {"category": "knowledge", "id": "ncbi_gene", "capabilities": ["search", "query", "fetch"]},
    {"category": "knowledge", "id": "ensembl", "capabilities": ["search", "query", "fetch"]},
    {"category": "knowledge", "id": "uniprot", "capabilities": ["search", "query", "fetch"]},
]
NCBI_EUTILS = "https://eutils.ncbi.nlm.nih.gov/entrez/eutils"
ENSEMBL = "https://rest.ensembl.org"
UNIPROT = "https://rest.uniprot.org"
USER_AGENT = "Omiga public-knowledge-sources retrieval plugin/0.1"
NCBI_FAVICON = "https://www.ncbi.nlm.nih.gov/favicon.ico"
ENSEMBL_FAVICON = "https://www.ensembl.org/favicon.ico"
UNIPROT_FAVICON = "https://www.uniprot.org/favicon.ico"
VALIDATION_IDS = {
    "ncbi_gene": "7157",
    "ensembl": "ENSG00000141510",
    "uniprot": "P04637",
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
    request_headers = {"User-Agent": USER_AGENT, "Accept": "application/json"}
    if headers:
        request_headers.update(headers)
    req = urllib.request.Request(url, headers=request_headers)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as response:
            body = response.read().decode("utf-8", errors="replace")
            return body, {key.lower(): value for key, value in response.headers.items()}
    except urllib.error.HTTPError as exc:
        body = exc.read().decode("utf-8", errors="replace")[:500]
        raise RuntimeError(f"HTTP {exc.code} from {url}: {body}") from exc
    except urllib.error.URLError as exc:
        raise RuntimeError(f"request failed for {url}: {exc.reason}") from exc


def urlopen_json(url: str, timeout: int = 25, headers: Optional[Dict[str, str]] = None) -> Tuple[Any, Dict[str, str]]:
    body, response_headers = urlopen_text(url, timeout=timeout, headers=headers)
    return json.loads(body), response_headers


def with_ncbi_credentials(url: str, request: Dict[str, Any]) -> str:
    creds = credentials(request)
    parts = urllib.parse.urlsplit(url)
    query = dict(urllib.parse.parse_qsl(parts.query, keep_blank_values=True))
    if creds.get("pubmed_api_key"):
        query["api_key"] = creds["pubmed_api_key"]
    if creds.get("pubmed_email"):
        query["email"] = creds["pubmed_email"]
    else:
        query.setdefault("email", "omiga@example.invalid")
    if creds.get("pubmed_tool_name"):
        query["tool"] = creds["pubmed_tool_name"]
    else:
        query.setdefault("tool", "omiga")
    encoded = urllib.parse.urlencode(query)
    return urllib.parse.urlunsplit((parts.scheme, parts.netloc, parts.path, encoded, parts.fragment))


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
        "notes": notes or ["public-knowledge-sources plugin"],
        "raw": raw or {"plugin": "public-knowledge-sources"},
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


def build_gene_query(request: Dict[str, Any], term: str) -> str:
    params = request_params(request)
    taxon_id = str_param(request, ("taxon_id", "taxid", "tax_id"))
    organism = str_param(request, ("organism", "species"))
    effective = term.strip()
    if taxon_id and "txid" not in effective.lower() and "[organism" not in effective.lower():
        effective = f"{effective} AND txid{taxon_id}[Organism:exp]"
    elif organism and "[organism" not in effective.lower():
        effective = f"{effective} AND {organism}[Organism]"
    sort = params.get("sort")
    return effective if not sort else effective


def ncbi_gene_esearch(request: Dict[str, Any], term: str) -> Tuple[List[str], Dict[str, Any]]:
    params = request_params(request)
    retstart = str(params.get("ret_start") or params.get("retstart") or params.get("offset") or 0)
    sort = str(params.get("sort") or "relevance")
    effective = build_gene_query(request, term)
    encoded = urllib.parse.urlencode({
        "db": "gene",
        "term": effective,
        "retmode": "json",
        "retmax": str(max_results(request)),
        "retstart": retstart,
        "sort": sort,
    })
    data, _ = urlopen_json(with_ncbi_credentials(f"{NCBI_EUTILS}/esearch.fcgi?{encoded}", request))
    root = data.get("esearchresult", {}) if isinstance(data, dict) else {}
    if root.get("error"):
        raise RuntimeError(f"NCBI Gene ESearch error: {root.get('error')}")
    ids = [str(value) for value in root.get("idlist", [])]
    return ids, {
        "query": term,
        "effective_query": effective,
        "count": int(root.get("count", 0) or 0),
        "ret_start": int(root.get("retstart", retstart) or 0),
        "ret_max": max_results(request),
        "query_translation": root.get("querytranslation"),
        "ids": ids,
    }


def ncbi_gene_esummary(request: Dict[str, Any], ids: Iterable[str]) -> List[Dict[str, Any]]:
    ids = [str(value) for value in ids if str(value).strip()]
    if not ids:
        return []
    encoded = urllib.parse.urlencode({"db": "gene", "id": ",".join(ids), "retmode": "json"})
    data, _ = urlopen_json(with_ncbi_credentials(f"{NCBI_EUTILS}/esummary.fcgi?{encoded}", request))
    result = data.get("result", {}) if isinstance(data, dict) else {}
    ordered = result.get("uids", ids) if isinstance(result, dict) else ids
    return [ncbi_gene_item(str(gene_id), result.get(str(gene_id), {})) for gene_id in ordered if isinstance(result, dict)]


def normalize_gene_id(value: str) -> str:
    value = (value or "").strip()
    match = re.search(r"ncbi\.nlm\.nih\.gov/gene/(\d+)", value, re.IGNORECASE)
    if match:
        return match.group(1)
    match = re.search(r"\bGene(?:ID)?[:\s]*(\d+)\b", value, re.IGNORECASE)
    if match:
        return match.group(1)
    match = re.search(r"\b\d+\b", value)
    return match.group(0) if match else value


def ncbi_gene_item(gene_id: str, record: Dict[str, Any]) -> Dict[str, Any]:
    gene_id = str(record.get("uid") or gene_id)
    symbol = first_string(record, "name", "nomenclaturesymbol") or gene_id
    description = first_string(record, "description", "nomenclaturename")
    organism = record.get("organism") if isinstance(record.get("organism"), dict) else {}
    organism_name = first_string(organism, "scientificname", "commonname")
    tax_id = first_string(organism, "taxid")
    chromosome = first_string(record, "chromosome")
    map_location = first_string(record, "maplocation")
    aliases = list_from_delimited(record.get("otheraliases"))
    summary = first_string(record, "summary")
    title = f"{symbol} — {description}" if description else symbol
    snippet = " · ".join(part for part in [organism_name, f"chr{chromosome}" if chromosome else "", map_location, summary] if part)
    return {
        "id": gene_id,
        "accession": gene_id,
        "title": title,
        "url": f"https://www.ncbi.nlm.nih.gov/gene/{gene_id}",
        "favicon": NCBI_FAVICON,
        "snippet": snippet[:500],
        "content": json.dumps(record, ensure_ascii=False, indent=2)[:20000],
        "metadata": {
            "source": "ncbi_gene",
            "gene_id": gene_id,
            "symbol": symbol,
            "description": description,
            "organism": organism_name,
            "tax_id": tax_id,
            "chromosome": chromosome,
            "map_location": map_location,
            "aliases": aliases,
            "summary": summary,
            "source_specific": record,
        },
        "raw": record,
    }


def handle_ncbi_gene(message_id: str, request: Dict[str, Any]) -> Dict[str, Any]:
    operation = request.get("operation", "query")
    if operation in ("search", "query"):
        term = query_text(request)
        if not term:
            return error(message_id, "missing_query", "NCBI Gene search/query requires query text")
        ids, search_meta = ncbi_gene_esearch(request, term)
        items = ncbi_gene_esummary(request, ids)
        response = base_response(request, "ncbi_gene", operation, items=items, total=search_meta.get("count"), raw=search_meta)
        return {"id": message_id, "type": "result", "response": response}
    if operation == "fetch":
        gene_id = normalize_gene_id(identifier_text(request))
        if not gene_id:
            return error(message_id, "missing_identifier", "NCBI Gene fetch requires numeric Gene ID, URL, or prior result")
        items = ncbi_gene_esummary(request, [gene_id])
        if not items:
            return error(message_id, "not_found", f"NCBI Gene record not found: {gene_id}")
        response = base_response(request, "ncbi_gene", operation, items=[], detail=items[0], total=1)
        return {"id": message_id, "type": "result", "response": response}
    return error(message_id, "unsupported_operation", f"NCBI Gene does not support operation {operation}")


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
    if is_validation(request):
        return validation_result(message_id, request)
    try:
        if source in {"ncbi_gene", "gene", "gene_id"}:
            return handle_ncbi_gene(message_id, request)
        if source in {"ensembl", "ensembl_gene", "ensembl_transcript", "variant", "variation"}:
            return handle_ensembl(message_id, request)
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
