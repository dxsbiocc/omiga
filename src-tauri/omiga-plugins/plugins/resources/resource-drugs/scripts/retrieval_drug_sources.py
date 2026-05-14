#!/usr/bin/env python3
"""Public drug retrieval plugin for Omiga's local JSONL protocol."""

from __future__ import annotations

import csv
import io
import json
import os
import re
import sys
import urllib.error
import urllib.parse
import urllib.request
import xml.etree.ElementTree as ET
from typing import Any, Dict, Iterable, List, Optional, Tuple

from pathlib import Path

RESOURCE_UTILS = Path(__file__).resolve().parents[2] / "utils"
if RESOURCE_UTILS.is_dir() and str(RESOURCE_UTILS) not in sys.path:
    sys.path.insert(0, str(RESOURCE_UTILS))

from retrieval_http import fetch_json, fetch_json_with_headers, fetch_text, fetch_text_with_headers, with_query_credentials

PROTOCOL_VERSION = 1
SOURCES = [
    {"category": "drug", "id": "chembl", "capabilities": ["search", "query", "fetch"]},
    {"category": "drug", "id": "pubchem", "capabilities": ["search", "query", "fetch"]},
    {"category": "drug", "id": "broad_repurposing_hub", "capabilities": ["search", "query", "fetch"]},
    {"category": "drug", "id": "openfda", "capabilities": ["search", "query", "fetch"]},
    {"category": "drug", "id": "clinicaltrials", "capabilities": ["search", "query", "fetch"]},
    {"category": "drug", "id": "dailymed", "capabilities": ["search", "query", "fetch"]},
]
PLUGIN_NAME = os.environ.get("OMIGA_RETRIEVAL_PLUGIN_NAME", "public-drug-source")

CHEMBL = "https://www.ebi.ac.uk/chembl/api/data"
NCBI_EUTILS = "https://eutils.ncbi.nlm.nih.gov/entrez/eutils"
PUBCHEM_PUG = "https://pubchem.ncbi.nlm.nih.gov/rest/pug"
OPENFDA = "https://api.fda.gov"
CLINICALTRIALS = "https://clinicaltrials.gov/api/v2"
DAILYMED = "https://dailymed.nlm.nih.gov/dailymed/services/v2"
BROAD_DRUG_TABLE_URLS = [
    "https://repo-hub.broadinstitute.org/public/data/repo-drug-annotation-20200324.txt",
    "https://s3.amazonaws.com/data.clue.io/repurposing/downloads/repurposing_drugs_20200324.txt",
]
USER_AGENT = "Omiga public-drug-sources retrieval plugin/0.1"
VALIDATION_IDS = {
    "chembl": "CHEMBL25",
    "pubchem": "2244",
    "broad_repurposing_hub": "aspirin",
    "openfda": "openfda-validation-label",
    "clinicaltrials": "NCT00000102",
    "dailymed": "055d8420-c189-4a9e-acdc-21627935c8eb",
}
BROAD_CACHE: Optional[Tuple[str, List[Dict[str, str]]]] = None


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


def write(message: Dict[str, Any]) -> None:
    print(json.dumps(message, separators=(",", ":"), ensure_ascii=False), flush=True)


def error(message_id: str, code: str, message: str) -> Dict[str, Any]:
    return {"id": message_id, "type": "error", "error": {"code": code, "message": message}}


def request_params(request: Dict[str, Any]) -> Dict[str, Any]:
    params = request.get("params")
    return params if isinstance(params, dict) else {}


def max_results(request: Dict[str, Any], default: int = 5, ceiling: int = 25) -> int:
    params = request_params(request)
    value = request.get("maxResults") or request.get("max_results") or params.get("limit") or params.get("retmax") or params.get("pageSize") or default
    try:
        parsed = int(value)
    except (TypeError, ValueError):
        parsed = default
    return max(1, min(parsed, ceiling))


def is_validation(request: Dict[str, Any]) -> bool:
    return bool(request_params(request).get("omigaValidation"))


def clean(value: Any) -> str:
    return str(value).strip() if value is not None else ""


def query_text(request: Dict[str, Any]) -> str:
    params = request_params(request)
    for key in ("query", "term", "q", "text", "drug", "drug_name", "name"):
        value = request.get(key) if key in request else params.get(key)
        if isinstance(value, str) and value.strip():
            return value.strip()
    return ""


def identifier_text(request: Dict[str, Any]) -> str:
    params = request_params(request)
    for key in (
        "id",
        "accession",
        "uid",
        "cid",
        "chembl_id",
        "setid",
        "set_id",
        "nct_id",
        "nctId",
        "application_number",
        "name",
    ):
        value = request.get(key) if key in request else params.get(key)
        if isinstance(value, str) and value.strip():
            return value.strip()
    result = request.get("result")
    if isinstance(result, dict):
        for key in ("id", "accession", "uid", "cid", "setid", "set_id", "nct_id", "nctId", "url", "link", "name"):
            value = result.get(key)
            if isinstance(value, str) and value.strip():
                return value.strip()
        metadata = result.get("metadata")
        if isinstance(metadata, dict):
            for key in ("cid", "chembl_id", "setid", "set_id", "nct_id", "nctId", "application_number", "drug_name"):
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


def urlopen_text(url: str, timeout: int = 25, headers: Optional[Dict[str, str]] = None) -> str:
    return fetch_text(url, timeout=timeout, headers=headers, user_agent=USER_AGENT)


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


def validation_item(source: str, operation: str) -> Dict[str, Any]:
    accession = VALIDATION_IDS.get(source, f"{source}-validation")
    return {
        "id": accession,
        "accession": accession,
        "title": f"Validation {source} {operation} result",
        "url": f"https://example.test/{source}/{urllib.parse.quote(accession)}",
        "snippet": "Offline validation result from public-drug-sources plugin.",
        "content": "This fixture response is returned only for Omiga validation smoke calls.",
        "metadata": {"source": source, "validation": True},
        "raw": {"validation": True},
    }


def validation_result(message_id: str, request: Dict[str, Any]) -> Dict[str, Any]:
    operation = request.get("operation", "search")
    source = normalize_source_alias(str(request.get("source", "chembl")))
    response: Dict[str, Any] = {
        "ok": True,
        "operation": operation,
        "category": request.get("category", "drug"),
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


def normalize_source_alias(source: str) -> str:
    source = (source or "").strip().lower().replace("-", "_")
    aliases = {
        "clinicaltrials_gov": "clinicaltrials",
        "clinical_trials": "clinicaltrials",
        "clinicaltrials.gov": "clinicaltrials",
        "broad": "broad_repurposing_hub",
        "broad_repurposing": "broad_repurposing_hub",
        "repurposing_hub": "broad_repurposing_hub",
        "daily_med": "dailymed",
        "open_fda": "openfda",
    }
    return aliases.get(source, source)


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
        "category": request.get("category", "drug"),
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


def first_list_value(value: Any) -> str:
    if isinstance(value, list):
        return clean(value[0]) if value else ""
    return clean(value)


def truncate_json(record: Any) -> str:
    return json.dumps(record, ensure_ascii=False, indent=2)[:20000]


def normalize_chembl_id(value: str) -> str:
    match = re.search(r"\bCHEMBL\d+\b", value or "", re.IGNORECASE)
    return match.group(0).upper() if match else value.strip()


def chembl_item(record: Dict[str, Any]) -> Dict[str, Any]:
    chembl_id = clean(record.get("molecule_chembl_id"))
    name = clean(record.get("pref_name")) or chembl_id or "ChEMBL molecule"
    props = record.get("molecule_properties") if isinstance(record.get("molecule_properties"), dict) else {}
    structures = record.get("molecule_structures") if isinstance(record.get("molecule_structures"), dict) else {}
    synonyms = record.get("molecule_synonyms") if isinstance(record.get("molecule_synonyms"), list) else []
    synonym_values = [clean(s.get("molecule_synonym")) for s in synonyms if isinstance(s, dict) and clean(s.get("molecule_synonym"))]
    phase = clean(record.get("max_phase"))
    approval = clean(record.get("first_approval"))
    molecule_type = clean(record.get("molecule_type"))
    formula = clean(props.get("full_molformula"))
    weight = clean(props.get("full_mwt"))
    snippet = " · ".join(part for part in [molecule_type, f"phase {phase}" if phase else "", f"first approval {approval}" if approval else "", formula] if part)
    return {
        "id": chembl_id or name,
        "accession": chembl_id,
        "title": name,
        "url": f"https://www.ebi.ac.uk/chembl/compound_report_card/{chembl_id}/" if chembl_id else "https://www.ebi.ac.uk/chembl/",
        "snippet": snippet[:500],
        "content": truncate_json(record),
        "metadata": {
            "source": "chembl",
            "chembl_id": chembl_id,
            "molecule_type": molecule_type,
            "max_phase": phase,
            "first_approval": approval,
            "formula": formula,
            "molecular_weight": weight,
            "canonical_smiles": clean(structures.get("canonical_smiles")),
            "standard_inchi_key": clean(structures.get("standard_inchi_key")),
            "synonyms": synonym_values[:20],
        },
        "raw": record,
    }


def chembl_search(request: Dict[str, Any], term: str) -> List[Dict[str, Any]]:
    params = urllib.parse.urlencode({"q": term, "limit": str(max_results(request))})
    data = urlopen_json(f"{CHEMBL}/molecule/search.json?{params}")
    records = data.get("molecules", []) if isinstance(data, dict) else []
    return [chembl_item(record) for record in records if isinstance(record, dict)]


def chembl_fetch(identifier: str) -> Dict[str, Any]:
    chembl_id = normalize_chembl_id(identifier)
    if re.fullmatch(r"CHEMBL\d+", chembl_id, re.IGNORECASE):
        data = urlopen_json(f"{CHEMBL}/molecule/{urllib.parse.quote(chembl_id.upper())}.json")
        if isinstance(data, dict):
            return chembl_item(data)
    items = chembl_search({"params": {"limit": 1}}, identifier)
    if not items:
        raise RuntimeError(f"ChEMBL molecule not found: {identifier}")
    return items[0]


def handle_chembl(message_id: str, request: Dict[str, Any]) -> Dict[str, Any]:
    operation = request.get("operation", "search")
    if operation in ("search", "query"):
        term = query_text(request)
        if not term:
            return error(message_id, "missing_query", "ChEMBL search/query requires molecule, target, or drug text")
        items = chembl_search(request, term)
        return {"id": message_id, "type": "result", "response": base_response(request, "chembl", operation, items=items, total=len(items))}
    if operation == "fetch":
        identifier = identifier_text(request)
        if not identifier:
            return error(message_id, "missing_identifier", "ChEMBL fetch requires CHEMBL id, molecule name, URL, or prior result")
        detail = chembl_fetch(identifier)
        return {"id": message_id, "type": "result", "response": base_response(request, "chembl", operation, detail=detail, total=1)}
    return error(message_id, "unsupported_operation", f"ChEMBL does not support operation {operation}")


def normalize_pubchem_cid(value: str) -> str:
    value = (value or "").strip()
    match = re.search(r"pubchem\.ncbi\.nlm\.nih\.gov/compound/(?:CID)?/?(\d+)", value, re.IGNORECASE)
    if match:
        return match.group(1)
    match = re.search(r"\bCID[:\s]*(\d+)\b", value, re.IGNORECASE)
    if match:
        return match.group(1)
    match = re.fullmatch(r"\d+", value)
    return match.group(0) if match else value


def pubchem_esearch(request: Dict[str, Any], term: str) -> List[str]:
    encoded = urllib.parse.urlencode({"db": "pccompound", "term": term, "retmode": "json", "retmax": str(max_results(request))})
    data = urlopen_json(with_ncbi_credentials(f"{NCBI_EUTILS}/esearch.fcgi?{encoded}", request))
    ids = data.get("esearchresult", {}).get("idlist", []) if isinstance(data, dict) else []
    return [str(value) for value in ids]


def pubchem_properties(cids: Iterable[str]) -> List[Dict[str, Any]]:
    cids = [str(cid).strip() for cid in cids if str(cid).strip()]
    if not cids:
        return []
    fields = "Title,MolecularFormula,MolecularWeight,CanonicalSMILES,IsomericSMILES,InChIKey,IUPACName"
    data = urlopen_json(f"{PUBCHEM_PUG}/compound/cid/{','.join(cids)}/property/{fields}/JSON")
    props = data.get("PropertyTable", {}).get("Properties", []) if isinstance(data, dict) else []
    return [record for record in props if isinstance(record, dict)]


def pubchem_item(record: Dict[str, Any]) -> Dict[str, Any]:
    cid = clean(record.get("CID"))
    title = clean(record.get("Title")) or f"PubChem CID {cid}"
    formula = clean(record.get("MolecularFormula"))
    weight = clean(record.get("MolecularWeight"))
    smiles = clean(record.get("CanonicalSMILES")) or clean(record.get("SMILES")) or clean(record.get("ConnectivitySMILES"))
    iupac = clean(record.get("IUPACName"))
    snippet = " · ".join(part for part in [formula, f"MW {weight}" if weight else "", iupac] if part)
    return {
        "id": cid or title,
        "accession": cid,
        "title": title,
        "url": f"https://pubchem.ncbi.nlm.nih.gov/compound/{cid}" if cid else "https://pubchem.ncbi.nlm.nih.gov/",
        "snippet": snippet[:500],
        "content": truncate_json(record),
        "metadata": {
            "source": "pubchem",
            "cid": cid,
            "formula": formula,
            "molecular_weight": weight,
            "canonical_smiles": smiles,
            "isomeric_smiles": clean(record.get("IsomericSMILES")),
            "inchi_key": clean(record.get("InChIKey")),
            "iupac_name": iupac,
        },
        "raw": record,
    }


def handle_pubchem(message_id: str, request: Dict[str, Any]) -> Dict[str, Any]:
    operation = request.get("operation", "search")
    if operation in ("search", "query"):
        term = query_text(request)
        if not term:
            return error(message_id, "missing_query", "PubChem search/query requires compound text")
        items = [pubchem_item(record) for record in pubchem_properties(pubchem_esearch(request, term))]
        return {"id": message_id, "type": "result", "response": base_response(request, "pubchem", operation, items=items, total=len(items))}
    if operation == "fetch":
        identifier = normalize_pubchem_cid(identifier_text(request))
        if not identifier:
            return error(message_id, "missing_identifier", "PubChem fetch requires CID, URL, name, or prior result")
        cids = [identifier] if re.fullmatch(r"\d+", identifier) else pubchem_esearch({"params": {"limit": 1}}, identifier)
        records = pubchem_properties(cids[:1])
        if not records:
            return error(message_id, "not_found", f"PubChem compound not found: {identifier}")
        detail = pubchem_item(records[0])
        return {"id": message_id, "type": "result", "response": base_response(request, "pubchem", operation, detail=detail, total=1)}
    return error(message_id, "unsupported_operation", f"PubChem does not support operation {operation}")


def load_broad_table() -> Tuple[str, List[Dict[str, str]]]:
    global BROAD_CACHE
    if BROAD_CACHE is not None:
        return BROAD_CACHE
    last_error = ""
    for url in BROAD_DRUG_TABLE_URLS:
        try:
            text = urlopen_text(url, timeout=30, headers={"Accept": "text/plain,*/*"})
            break
        except Exception as exc:  # Try mirror URL before surfacing the provider error.
            last_error = str(exc)
    else:
        raise RuntimeError(f"Broad Repurposing Hub table unavailable: {last_error}")
    rows_text = "\n".join(line for line in text.splitlines() if line and not line.startswith("!"))
    rows = [dict(row) for row in csv.DictReader(io.StringIO(rows_text), delimiter="\t")]
    BROAD_CACHE = (url, rows)
    return BROAD_CACHE


def broad_row_id(row: Dict[str, str]) -> str:
    return clean(row.get("pert_iname"))


def broad_item(row: Dict[str, str], source_url: str) -> Dict[str, Any]:
    name = broad_row_id(row) or "Broad Repurposing Hub drug"
    phase = clean(row.get("clinical_phase"))
    moa = clean(row.get("moa"))
    target = clean(row.get("target"))
    disease_area = clean(row.get("disease_area"))
    indication = clean(row.get("indication"))
    snippet = " · ".join(part for part in [phase, moa, f"targets {target}" if target else "", disease_area, indication] if part)
    return {
        "id": name,
        "accession": name,
        "title": name,
        "url": "https://repo-hub.broadinstitute.org/repurposing",
        "snippet": snippet[:500],
        "content": truncate_json(row),
        "metadata": {
            "source": "broad_repurposing_hub",
            "drug_name": name,
            "clinical_phase": phase,
            "moa": moa,
            "target": [item.strip() for item in target.split("|") if item.strip()],
            "disease_area": disease_area,
            "indication": indication,
            "table_url": source_url,
        },
        "raw": row,
    }


def broad_matches(row: Dict[str, str], term: str) -> bool:
    haystack = " ".join(clean(value).lower() for value in row.values())
    return all(token in haystack for token in term.lower().split())


def handle_broad(message_id: str, request: Dict[str, Any]) -> Dict[str, Any]:
    operation = request.get("operation", "search")
    source_url, rows = load_broad_table()
    if operation in ("search", "query"):
        term = query_text(request)
        if not term:
            return error(message_id, "missing_query", "Broad Repurposing Hub search/query requires drug, target, mechanism, or indication text")
        items = [broad_item(row, source_url) for row in rows if broad_matches(row, term)][: max_results(request)]
        return {"id": message_id, "type": "result", "response": base_response(request, "broad_repurposing_hub", operation, items=items, total=len(items), raw={"plugin": PLUGIN_NAME, "table_url": source_url})}
    if operation == "fetch":
        identifier = identifier_text(request)
        if not identifier:
            return error(message_id, "missing_identifier", "Broad Repurposing Hub fetch requires perturbagen name or prior result")
        normalized = identifier.strip().lower()
        matched = next((row for row in rows if broad_row_id(row).lower() == normalized), None)
        if matched is None:
            matched = next((row for row in rows if broad_matches(row, identifier)), None)
        if matched is None:
            return error(message_id, "not_found", f"Broad Repurposing Hub drug not found: {identifier}")
        detail = broad_item(matched, source_url)
        return {"id": message_id, "type": "result", "response": base_response(request, "broad_repurposing_hub", operation, detail=detail, total=1, raw={"plugin": PLUGIN_NAME, "table_url": source_url})}
    return error(message_id, "unsupported_operation", f"Broad Repurposing Hub does not support operation {operation}")


def openfda_endpoint(request: Dict[str, Any]) -> str:
    endpoint = clean(request_params(request).get("endpoint") or request_params(request).get("api") or "label").lower().replace("-", "_")
    if endpoint in {"event", "adverse", "adverse_event", "drug_event"}:
        return "event"
    if endpoint in {"drugsfda", "approval", "approvals", "drug_approval"}:
        return "drugsfda"
    return "label"


def openfda_search_url(endpoint: str, term: str, limit: int) -> str:
    if endpoint == "event":
        search = f'patient.drug.medicinalproduct:"{term}"'
    elif endpoint == "drugsfda":
        search = term
    else:
        search = term
    params = urllib.parse.urlencode({"search": search, "limit": str(limit)})
    return f"{OPENFDA}/drug/{endpoint}.json?{params}"


def openfda_fetch_url(endpoint: str, identifier: str) -> str:
    value = identifier.strip()
    if endpoint == "event":
        search = f"safetyreportid:{value}"
    elif endpoint == "drugsfda":
        search = f"application_number:{value}"
    else:
        if re.fullmatch(r"[A-Za-z0-9_-]+", value):
            search = f"id:{value}"
        else:
            search = value
    params = urllib.parse.urlencode({"search": search, "limit": "1"})
    return f"{OPENFDA}/drug/{endpoint}.json?{params}"


def openfda_label_title(record: Dict[str, Any]) -> str:
    openfda = record.get("openfda") if isinstance(record.get("openfda"), dict) else {}
    return first_list_value(openfda.get("brand_name")) or first_list_value(openfda.get("generic_name")) or first_list_value(record.get("spl_product_data_elements")) or clean(record.get("id")) or "openFDA drug label"


def openfda_item(record: Dict[str, Any], endpoint: str) -> Dict[str, Any]:
    openfda = record.get("openfda") if isinstance(record.get("openfda"), dict) else {}
    if endpoint == "event":
        report_id = clean(record.get("safetyreportid"))
        drugs = record.get("patient", {}).get("drug", []) if isinstance(record.get("patient"), dict) else []
        drug_names = [clean(drug.get("medicinalproduct")) for drug in drugs if isinstance(drug, dict) and clean(drug.get("medicinalproduct"))]
        title = ", ".join(drug_names[:3]) or f"openFDA adverse event {report_id}"
        snippet = " · ".join(part for part in [clean(record.get("serious")), clean(record.get("receivedate")), clean(record.get("primarysourcecountry"))] if part)
        accession = report_id
    elif endpoint == "drugsfda":
        accession = clean(record.get("application_number"))
        title = clean(record.get("sponsor_name")) or accession or "openFDA Drugs@FDA record"
        products = record.get("products") if isinstance(record.get("products"), list) else []
        product_names = [clean(p.get("brand_name")) for p in products if isinstance(p, dict) and clean(p.get("brand_name"))]
        if product_names:
            title = f"{', '.join(product_names[:3])} ({accession})"
        snippet = " · ".join(part for part in [clean(record.get("sponsor_name")), clean(record.get("submissions", [{}])[0].get("submission_status_date") if isinstance(record.get("submissions"), list) and record.get("submissions") else "")] if part)
    else:
        accession = clean(record.get("id")) or first_list_value(record.get("set_id")) or first_list_value(openfda.get("spl_set_id"))
        title = openfda_label_title(record)
        snippet = first_list_value(record.get("indications_and_usage")) or first_list_value(record.get("purpose")) or first_list_value(record.get("warnings"))
    return {
        "id": accession or title,
        "accession": accession,
        "title": title,
        "url": f"https://api.fda.gov/drug/{endpoint}.json?search={urllib.parse.quote(accession)}" if accession else f"https://open.fda.gov/apis/drug/{endpoint}/",
        "snippet": snippet[:500],
        "content": truncate_json(record),
        "metadata": {"source": "openfda", "endpoint": endpoint, "openfda": openfda, "accession": accession},
        "raw": record,
    }


def handle_openfda(message_id: str, request: Dict[str, Any]) -> Dict[str, Any]:
    operation = request.get("operation", "search")
    endpoint = openfda_endpoint(request)
    if operation in ("search", "query"):
        term = query_text(request)
        if not term:
            return error(message_id, "missing_query", "openFDA search/query requires drug or label text")
        data = urlopen_json(openfda_search_url(endpoint, term, max_results(request)))
        records = data.get("results", []) if isinstance(data, dict) else []
        items = [openfda_item(record, endpoint) for record in records if isinstance(record, dict)]
        total = data.get("meta", {}).get("results", {}).get("total") if isinstance(data, dict) else len(items)
        return {"id": message_id, "type": "result", "response": base_response(request, "openfda", operation, items=items, total=total, raw={"plugin": PLUGIN_NAME, "endpoint": endpoint})}
    if operation == "fetch":
        identifier = identifier_text(request)
        if not identifier:
            return error(message_id, "missing_identifier", "openFDA fetch requires record id/application number/safety report id or prior result")
        data = urlopen_json(openfda_fetch_url(endpoint, identifier))
        records = data.get("results", []) if isinstance(data, dict) else []
        if not records:
            return error(message_id, "not_found", f"openFDA record not found: {identifier}")
        detail = openfda_item(records[0], endpoint)
        return {"id": message_id, "type": "result", "response": base_response(request, "openfda", operation, detail=detail, total=1, raw={"plugin": PLUGIN_NAME, "endpoint": endpoint})}
    return error(message_id, "unsupported_operation", f"openFDA does not support operation {operation}")


def normalize_nct_id(value: str) -> str:
    match = re.search(r"\bNCT\d{8}\b", value or "", re.IGNORECASE)
    return match.group(0).upper() if match else value.strip()


def clinicaltrial_item(study: Dict[str, Any]) -> Dict[str, Any]:
    protocol = study.get("protocolSection") if isinstance(study.get("protocolSection"), dict) else {}
    ident = protocol.get("identificationModule") if isinstance(protocol.get("identificationModule"), dict) else {}
    status = protocol.get("statusModule") if isinstance(protocol.get("statusModule"), dict) else {}
    conditions = protocol.get("conditionsModule") if isinstance(protocol.get("conditionsModule"), dict) else {}
    arms = protocol.get("armsInterventionsModule") if isinstance(protocol.get("armsInterventionsModule"), dict) else {}
    nct_id = clean(ident.get("nctId"))
    title = clean(ident.get("briefTitle")) or clean(ident.get("officialTitle")) or nct_id or "ClinicalTrials.gov study"
    interventions = arms.get("interventions") if isinstance(arms.get("interventions"), list) else []
    intervention_names = [clean(item.get("name")) for item in interventions if isinstance(item, dict) and clean(item.get("name"))]
    condition_values = [clean(value) for value in conditions.get("conditions", [])] if isinstance(conditions.get("conditions"), list) else []
    snippet = " · ".join(part for part in [clean(status.get("overallStatus")), ", ".join(condition_values[:3]), ", ".join(intervention_names[:3])] if part)
    return {
        "id": nct_id or title,
        "accession": nct_id,
        "title": title,
        "url": f"https://clinicaltrials.gov/study/{nct_id}" if nct_id else "https://clinicaltrials.gov/",
        "snippet": snippet[:500],
        "content": truncate_json(study),
        "metadata": {
            "source": "clinicaltrials",
            "nct_id": nct_id,
            "overall_status": clean(status.get("overallStatus")),
            "conditions": condition_values,
            "interventions": intervention_names,
            "phase": arms.get("phases") if isinstance(arms.get("phases"), list) else [],
        },
        "raw": study,
    }


def clinicaltrials_search(request: Dict[str, Any], term: str) -> Tuple[List[Dict[str, Any]], Optional[int]]:
    params = urllib.parse.urlencode({"query.term": term, "pageSize": str(max_results(request)), "format": "json"})
    data = urlopen_json(f"{CLINICALTRIALS}/studies?{params}")
    studies = data.get("studies", []) if isinstance(data, dict) else []
    total = data.get("totalCount") if isinstance(data, dict) else len(studies)
    return [clinicaltrial_item(study) for study in studies if isinstance(study, dict)], total


def clinicaltrials_fetch(identifier: str) -> Dict[str, Any]:
    nct_id = normalize_nct_id(identifier)
    if not re.fullmatch(r"NCT\d{8}", nct_id, re.IGNORECASE):
        items, _ = clinicaltrials_search({"params": {"limit": 1}}, identifier)
        if not items:
            raise RuntimeError(f"ClinicalTrials.gov study not found: {identifier}")
        return items[0]
    data = urlopen_json(f"{CLINICALTRIALS}/studies/{nct_id.upper()}?format=json")
    if isinstance(data, dict) and isinstance(data.get("protocolSection"), dict):
        return clinicaltrial_item(data)
    if isinstance(data, dict) and isinstance(data.get("study"), dict):
        return clinicaltrial_item(data["study"])
    raise RuntimeError(f"ClinicalTrials.gov study not found: {identifier}")


def handle_clinicaltrials(message_id: str, request: Dict[str, Any]) -> Dict[str, Any]:
    operation = request.get("operation", "search")
    if operation in ("search", "query"):
        term = query_text(request)
        if not term:
            return error(message_id, "missing_query", "ClinicalTrials.gov search/query requires drug, condition, or study text")
        items, total = clinicaltrials_search(request, term)
        return {"id": message_id, "type": "result", "response": base_response(request, "clinicaltrials", operation, items=items, total=total)}
    if operation == "fetch":
        identifier = identifier_text(request)
        if not identifier:
            return error(message_id, "missing_identifier", "ClinicalTrials.gov fetch requires NCT id, URL, or prior result")
        try:
            detail = clinicaltrials_fetch(identifier)
        except Exception as exc:
            return error(message_id, "not_found", str(exc))
        return {"id": message_id, "type": "result", "response": base_response(request, "clinicaltrials", operation, detail=detail, total=1)}
    return error(message_id, "unsupported_operation", f"ClinicalTrials.gov does not support operation {operation}")


def normalize_dailymed_setid(value: str) -> str:
    value = (value or "").strip()
    match = re.search(r"\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b", value, re.IGNORECASE)
    return match.group(0).lower() if match else value


def dailymed_item(record: Dict[str, Any]) -> Dict[str, Any]:
    setid = clean(record.get("setid"))
    title = clean(record.get("title")) or setid or "DailyMed SPL"
    published = clean(record.get("published_date"))
    snippet = " · ".join(part for part in [published, f"SPL version {clean(record.get('spl_version'))}" if record.get("spl_version") else ""] if part)
    return {
        "id": setid or title,
        "accession": setid,
        "title": title,
        "url": f"https://dailymed.nlm.nih.gov/dailymed/drugInfo.cfm?setid={setid}" if setid else "https://dailymed.nlm.nih.gov/",
        "snippet": snippet[:500],
        "content": truncate_json(record),
        "metadata": {"source": "dailymed", "setid": setid, "published_date": published, "spl_version": record.get("spl_version")},
        "raw": record,
    }


def dailymed_search(request: Dict[str, Any], term: str) -> Tuple[List[Dict[str, Any]], Optional[int]]:
    params = urllib.parse.urlencode({"drug_name": term, "pagesize": str(max_results(request))})
    data = urlopen_json(f"{DAILYMED}/spls.json?{params}")
    records = data.get("data", []) if isinstance(data, dict) else []
    total = data.get("metadata", {}).get("total_elements") if isinstance(data, dict) else len(records)
    return [dailymed_item(record) for record in records if isinstance(record, dict)], total


def xml_text(root: ET.Element, names: Iterable[str], limit: int = 4000) -> str:
    parts: List[str] = []
    wanted = set(names)
    for elem in root.iter():
        tag = elem.tag.rsplit("}", 1)[-1]
        if tag in wanted and elem.text and elem.text.strip():
            parts.append(elem.text.strip())
        if sum(len(part) for part in parts) > limit:
            break
    return "\n".join(parts)[:limit]


def dailymed_fetch(identifier: str) -> Dict[str, Any]:
    setid = normalize_dailymed_setid(identifier)
    if not re.fullmatch(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}", setid, re.IGNORECASE):
        items, _ = dailymed_search({"params": {"limit": 1}}, identifier)
        if not items:
            raise RuntimeError(f"DailyMed SPL not found: {identifier}")
        return items[0]
    xml = urlopen_text(f"{DAILYMED}/spls/{setid}.xml", headers={"Accept": "application/xml,text/xml,*/*"})
    root = ET.fromstring(xml)
    title = xml_text(root, ["title"], 500) or f"DailyMed SPL {setid}"
    body = xml_text(root, ["paragraph", "item", "text"], 20000)
    record = {"setid": setid, "title": title, "content": body}
    item = dailymed_item(record)
    item["content"] = body or xml[:20000]
    item["raw"] = record
    return item


def handle_dailymed(message_id: str, request: Dict[str, Any]) -> Dict[str, Any]:
    operation = request.get("operation", "search")
    if operation in ("search", "query"):
        term = query_text(request)
        if not term:
            return error(message_id, "missing_query", "DailyMed search/query requires drug name text")
        items, total = dailymed_search(request, term)
        return {"id": message_id, "type": "result", "response": base_response(request, "dailymed", operation, items=items, total=total)}
    if operation == "fetch":
        identifier = identifier_text(request)
        if not identifier:
            return error(message_id, "missing_identifier", "DailyMed fetch requires setid, drug name, URL, or prior result")
        try:
            detail = dailymed_fetch(identifier)
        except Exception as exc:
            return error(message_id, "not_found", str(exc))
        return {"id": message_id, "type": "result", "response": base_response(request, "dailymed", operation, detail=detail, total=1)}
    return error(message_id, "unsupported_operation", f"DailyMed does not support operation {operation}")


def handle_execute(message: Dict[str, Any]) -> Dict[str, Any]:
    message_id = str(message.get("id", "execute"))
    request = message.get("request") if isinstance(message.get("request"), dict) else {}
    source = normalize_source_alias(str(request.get("source", "")).strip())
    if not source_is_allowed(source):
        return error(message_id, "unknown_source", f"source is not served by this plugin: {source}")
    if is_validation(request):
        return validation_result(message_id, request)
    try:
        if source == "chembl":
            return handle_chembl(message_id, request)
        if source == "pubchem":
            return handle_pubchem(message_id, request)
        if source == "broad_repurposing_hub":
            return handle_broad(message_id, request)
        if source == "openfda":
            return handle_openfda(message_id, request)
        if source == "clinicaltrials":
            return handle_clinicaltrials(message_id, request)
        if source == "dailymed":
            return handle_dailymed(message_id, request)
        return error(message_id, "unknown_source", f"unknown drug source: {source}")
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
