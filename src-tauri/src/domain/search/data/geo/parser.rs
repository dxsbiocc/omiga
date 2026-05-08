//! GEO Entrez response parsing and accession helpers.

use super::super::common::*;
use serde_json::{json, Map as JsonMap, Value as Json};

pub(super) fn parse_geo_esearch(
    value: &Json,
) -> Result<(u64, Vec<String>, Option<String>), String> {
    let root = value
        .get("esearchresult")
        .and_then(Json::as_object)
        .ok_or_else(|| "NCBI GEO ESearch response missing esearchresult".to_string())?;
    if let Some(error) = root.get("error").and_then(Json::as_str) {
        return Err(format!("NCBI GEO ESearch error: {error}"));
    }
    let count = root
        .get("count")
        .and_then(json_u64_from_string_or_number)
        .unwrap_or(0);
    let ids = root
        .get("idlist")
        .and_then(Json::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Json::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let query_translation = root
        .get("querytranslation")
        .and_then(Json::as_str)
        .map(str::to_string);
    Ok((count, ids, query_translation))
}

pub(super) fn parse_geo_esummary(value: &Json, ordered_ids: &[String]) -> Vec<DataRecord> {
    let Some(result) = value.get("result").and_then(Json::as_object) else {
        return Vec::new();
    };
    ordered_ids
        .iter()
        .filter_map(|uid| result.get(uid).and_then(|doc| parse_geo_doc(uid, doc)))
        .collect()
}

fn parse_geo_doc(uid: &str, doc: &Json) -> Option<DataRecord> {
    let map = doc.as_object()?;
    let accession = string_field_any(
        map,
        &["accession", "Accession", "gse", "GSE", "gds", "GDS", "acc"],
    )
    .unwrap_or_else(|| uid.to_string());
    let title = string_field_any(map, &["title", "Title", "gdsTitle", "GDS_Title"])
        .or_else(|| string_field_any(map, &["summary", "Summary"]))
        .unwrap_or_else(|| accession.clone());
    let summary = string_field_any(map, &["summary", "Summary", "description", "Description"])
        .unwrap_or_default();
    let record_type = string_field_any(
        map,
        &["gdsType", "gdstype", "entryType", "entrytype", "type"],
    );
    let organism = string_field_any(map, &["taxon", "taxa", "organism", "Organism"]);
    let sample_count = json_u64_from_keys(map, &["n_samples", "nSamples", "samples", "Samples"]);
    let platform = string_field_any(map, &["GPL", "gpl", "platform", "Platform"]);
    let published_date = string_field_any(map, &["PDAT", "pdat", "pubdate", "pub_date"]);
    let updated_date = string_field_any(map, &["updated", "updated_date", "last_update"]);
    let files = string_vec_field_any(map, &["suppFile", "suppFiles", "ftp", "files"]);
    let url = geo_record_url(&accession, uid);

    let mut extra = JsonMap::new();
    extra.insert("uid".to_string(), json!(uid));
    for key in [
        "gse",
        "GSE",
        "gpl",
        "GPL",
        "gdsType",
        "entryType",
        "FTPLink",
        "ftplink",
    ] {
        if let Some(value) = map.get(key) {
            extra.insert(key.to_string(), value.clone());
        }
    }

    Some(DataRecord {
        id: if accession.is_empty() {
            uid.to_string()
        } else {
            accession.clone()
        },
        accession,
        source: PublicDataSource::Geo,
        title: clean_html_text(&title),
        summary: clean_html_text(&summary),
        url,
        record_type,
        organism,
        published_date,
        updated_date,
        sample_count,
        platform,
        files,
        extra,
    })
}

pub(super) fn looks_like_geo_accession(value: &str) -> bool {
    let Some(accession) = normalize_accession(value) else {
        return false;
    };
    let upper = accession.to_ascii_uppercase();
    ["GSE", "GSM", "GPL", "GDS"].iter().any(|prefix| {
        upper
            .strip_prefix(prefix)
            .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()))
    })
}

fn geo_record_url(accession: &str, uid: &str) -> String {
    if looks_like_geo_accession(accession) {
        format!(
            "https://www.ncbi.nlm.nih.gov/geo/query/acc.cgi?acc={}",
            accession
        )
    } else {
        format!("https://www.ncbi.nlm.nih.gov/gds/{uid}")
    }
}
