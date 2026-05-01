//! NCBI BioSample adapter.
//!
//! Search uses official NCBI Entrez E-utilities (`db=biosample`) to discover
//! BioSample UIDs/accessions. Detail fetch uses the official NCBI Datasets v2
//! BioSample report endpoint by BioSample accession.

use super::common::*;
use super::PublicDataClient;
use serde_json::{json, Map as JsonMap, Value as Json};

impl PublicDataClient {
    pub(in crate::domain::search::data) async fn search_biosample(
        &self,
        args: DataSearchArgs,
    ) -> Result<DataSearchResponse, String> {
        let ret_max = args.normalized_max_results();
        let mut params = self.biosample_entrez_params("json");
        params.push(("term".to_string(), biosample_query(&args)));
        params.push(("retmax".to_string(), ret_max.to_string()));

        let search_json = self.get_biosample_entrez_json("esearch", &params).await?;
        let (count, ids, query_translation) = parse_biosample_esearch(&search_json)?;
        if ids.is_empty() {
            return Ok(DataSearchResponse {
                query: args.query.trim().to_string(),
                source: PublicDataSource::BioSample.as_str().to_string(),
                total: Some(count),
                results: Vec::new(),
                notes: vec!["NCBI BioSample ESearch returned no matching UIDs".to_string()],
            });
        }

        let mut summary_params = self.biosample_entrez_params("json");
        summary_params.push(("id".to_string(), ids.join(",")));
        let summary_json = self
            .get_biosample_entrez_json("esummary", &summary_params)
            .await?;
        let results = parse_biosample_esummary(&summary_json, &ids);
        let mut notes = vec![
            "NCBI Entrez E-utilities db=biosample for search; fetch uses NCBI Datasets v2 BioSample reports."
                .to_string(),
        ];
        if let Some(q) = query_translation {
            notes.push(format!("NCBI query translation: {q}"));
        }
        Ok(DataSearchResponse {
            query: args.query.trim().to_string(),
            source: PublicDataSource::BioSample.as_str().to_string(),
            total: Some(count),
            results,
            notes,
        })
    }

    pub(in crate::domain::search::data) async fn fetch_biosample(
        &self,
        identifier: &str,
    ) -> Result<DataRecord, String> {
        let accession = normalize_biosample_accession(identifier).ok_or_else(|| {
            format!(
                "BioSample fetch requires a BioSample accession (SAMN/SAMEA/SAMD) or URL, got `{identifier}`"
            )
        })?;
        let endpoint = format!(
            "biosample/accession/{}/biosample_report",
            encode_path_segment(&accession)
        );
        let json = self.get_ncbi_datasets_json(&endpoint, &[]).await?;
        parse_biosample_report_page(&json)
            .into_iter()
            .next()
            .ok_or_else(|| format!("NCBI Datasets returned no BioSample report for `{accession}`"))
    }

    fn biosample_entrez_params(&self, retmode: &str) -> Vec<(String, String)> {
        let mut params = vec![
            ("db".to_string(), "biosample".to_string()),
            ("retmode".to_string(), retmode.to_string()),
            ("tool".to_string(), self.settings.tool.clone()),
            ("email".to_string(), self.settings.email.clone()),
        ];
        if let Some(api_key) = &self.settings.api_key {
            params.push(("api_key".to_string(), api_key.clone()));
        }
        params
    }

    async fn get_biosample_entrez_json(
        &self,
        utility: &str,
        params: &[(String, String)],
    ) -> Result<Json, String> {
        #[cfg(debug_assertions)]
        if self.base_urls.entrez == "mock://entrez" {
            return mock_biosample_entrez_json(utility, params).ok_or_else(|| {
                format!("debug BioSample Entrez mock has no fixture for {utility} {params:?}")
            });
        }

        let url = format!("{}/{}.fcgi", self.base_urls.entrez, utility);
        let response = self
            .http
            .get(&url)
            .query(params)
            .send()
            .await
            .map_err(|e| format!("NCBI BioSample Entrez {utility} request failed: {e}"))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| format!("NCBI BioSample Entrez {utility} response read failed: {e}"))?;
        if !status.is_success() {
            return Err(format!(
                "NCBI BioSample Entrez {utility} returned HTTP {status}: {}",
                truncate_for_error(&body)
            ));
        }
        let json: Json = serde_json::from_str(&body).map_err(|e| {
            format!(
                "NCBI BioSample Entrez {utility} returned non-JSON response: {e}; body: {}",
                truncate_for_error(&body)
            )
        })?;
        if let Some(error) = json.get("error").and_then(json_string) {
            return Err(format!("NCBI BioSample Entrez {utility} error: {error}"));
        }
        Ok(json)
    }
}

pub fn looks_like_biosample_accession(value: &str) -> bool {
    normalize_biosample_accession(value).is_some()
}

pub(in crate::domain::search::data) fn parse_biosample_report_page(
    value: &Json,
) -> Vec<DataRecord> {
    value
        .get("reports")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .filter_map(parse_biosample_report)
        .collect()
}

fn parse_biosample_esearch(value: &Json) -> Result<(u64, Vec<String>, Option<String>), String> {
    let result = value
        .get("esearchresult")
        .ok_or_else(|| "NCBI BioSample ESearch response missing esearchresult".to_string())?;
    let count = result
        .get("count")
        .and_then(json_u64_from_string_or_number)
        .unwrap_or(0);
    let ids = result
        .get("idlist")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .filter_map(json_string)
        .collect::<Vec<_>>();
    let translation = result.get("querytranslation").and_then(json_string);
    Ok((count, ids, translation))
}

fn parse_biosample_esummary(value: &Json, ordered_ids: &[String]) -> Vec<DataRecord> {
    let Some(result) = value.get("result").and_then(Json::as_object) else {
        return Vec::new();
    };
    ordered_ids
        .iter()
        .filter_map(|uid| {
            result
                .get(uid)
                .and_then(|doc| parse_biosample_summary_doc(uid, doc))
        })
        .collect()
}

fn parse_biosample_summary_doc(uid: &str, doc: &Json) -> Option<DataRecord> {
    let map = doc.as_object()?;
    let accession = string_field_any(map, &["accession", "sourcesample"])
        .and_then(|value| normalize_biosample_accession(&value))?;
    let title =
        string_field_any(map, &["title"]).unwrap_or_else(|| format!("NCBI BioSample {accession}"));
    let organism = string_field_any(map, &["organism"]);
    let package = string_field_any(map, &["package"]);
    let identifiers = string_field_any(map, &["identifiers"]);
    let infraspecies = string_field_any(map, &["infraspecies"]);
    let comment = string_field_any(map, &["sampledata"])
        .and_then(|xml| extract_first_xml_tag(&xml, &["Paragraph", "Comment"]));

    let mut summary = Vec::new();
    push_labeled(&mut summary, "Organism", organism.as_deref());
    push_labeled(&mut summary, "Package", package.as_deref());
    push_labeled(&mut summary, "Identifiers", identifiers.as_deref());
    push_labeled(&mut summary, "Infraspecies", infraspecies.as_deref());
    push_plain(&mut summary, comment.as_deref());

    let mut extra = JsonMap::new();
    insert_extra(&mut extra, "uid", Some(uid.to_string()));
    insert_extra(&mut extra, "taxonomy", string_field_any(map, &["taxonomy"]));
    insert_extra(
        &mut extra,
        "organization",
        string_field_any(map, &["organization"]),
    );
    insert_extra(&mut extra, "identifiers", identifiers);
    insert_extra(&mut extra, "infraspecies", infraspecies);
    insert_extra(&mut extra, "package", package);
    if let Some(sampledata) = map.get("sampledata").and_then(json_string) {
        extra.insert("sampledata_xml".to_string(), Json::String(sampledata));
    }

    Some(DataRecord {
        id: accession.clone(),
        accession: accession.clone(),
        source: PublicDataSource::BioSample,
        title,
        summary: summary.join(" | "),
        url: biosample_record_url(&accession),
        record_type: Some("biosample".to_string()),
        organism,
        published_date: string_field_any(map, &["publicationdate", "date"]),
        updated_date: string_field_any(map, &["modificationdate"]),
        sample_count: None,
        platform: None,
        files: Vec::new(),
        extra,
    })
}

fn parse_biosample_report(report: &Json) -> Option<DataRecord> {
    let accession = json_path_string(report, &["accession"])
        .and_then(|value| normalize_biosample_accession(&value))?;
    let title = json_path_string(report, &["description", "title"])
        .unwrap_or_else(|| format!("NCBI BioSample {accession}"));
    let organism = json_path_string(report, &["description", "organism", "organism_name"]);
    let comment = json_path_string(report, &["description", "comment"]);
    let package = json_path_string(report, &["package"]);
    let owner = json_path_string(report, &["owner", "name"]);
    let status = json_path_string(report, &["status", "status"]);
    let attributes = biosample_attributes(report);

    let mut summary = Vec::new();
    push_labeled(&mut summary, "Organism", organism.as_deref());
    push_labeled(&mut summary, "Package", package.as_deref());
    push_labeled(&mut summary, "Owner", owner.as_deref());
    push_labeled(&mut summary, "Status", status.as_deref());
    for key in [
        "tissue",
        "sex",
        "collection_date",
        "geo_loc_name",
        "isolate",
        "host",
    ] {
        push_labeled(
            &mut summary,
            key,
            attributes.get(key).and_then(Json::as_str),
        );
    }
    push_plain(&mut summary, comment.as_deref());

    let mut extra = JsonMap::new();
    insert_extra(
        &mut extra,
        "tax_id",
        json_path_string(report, &["description", "organism", "tax_id"]),
    );
    insert_extra(&mut extra, "owner", owner);
    insert_extra(&mut extra, "package", package);
    insert_extra(&mut extra, "status", status);
    insert_extra(
        &mut extra,
        "status_when",
        json_path_string(report, &["status", "when"]),
    );
    insert_extra(&mut extra, "models", json_array_join(report.get("models")));
    insert_extra(&mut extra, "sample_ids", sample_ids_join(report));
    insert_extra(&mut extra, "bioprojects", bioprojects_join(report));
    if !attributes.is_empty() {
        extra.insert("attributes".to_string(), Json::Object(attributes));
    }

    Some(DataRecord {
        id: accession.clone(),
        accession: accession.clone(),
        source: PublicDataSource::BioSample,
        title,
        summary: summary.join(" | "),
        url: biosample_record_url(&accession),
        record_type: Some("biosample".to_string()),
        organism,
        published_date: json_path_string(report, &["publication_date"]),
        updated_date: json_path_string(report, &["last_updated"]),
        sample_count: None,
        platform: None,
        files: Vec::new(),
        extra,
    })
}

fn biosample_query(args: &DataSearchArgs) -> String {
    let mut query = args.query.trim().to_string();
    if let Some(organism) = param_string(args.params.as_ref(), &["organism"]) {
        query = format!("{query} AND {organism}[Organism]");
    }
    if let Some(taxon_id) = param_string(args.params.as_ref(), &["taxon_id", "taxid", "tax_id"]) {
        query = format!("{query} AND txid{taxon_id}[Organism:exp]");
    }
    query
}

fn normalize_biosample_accession(value: &str) -> Option<String> {
    let value = value.trim().trim_end_matches('/');
    if value.is_empty() {
        return None;
    }
    if let Ok(parsed) = reqwest::Url::parse(value) {
        let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
        if host.contains("ncbi.nlm.nih.gov") || host.contains("api.ncbi.nlm.nih.gov") {
            if let Some(accession) = parsed
                .path_segments()
                .into_iter()
                .flatten()
                .find_map(biosample_accession_from_text)
            {
                return Some(accession);
            }
            for (_, val) in parsed.query_pairs() {
                if let Some(accession) = biosample_accession_from_text(&val) {
                    return Some(accession);
                }
            }
        }
    }
    biosample_accession_from_text(value)
}

fn biosample_accession_from_text(value: &str) -> Option<String> {
    lazy_static::lazy_static! {
        static ref RE_BIOSAMPLE: regex::Regex = regex::Regex::new(r#"(?i)\bSAM(?:N|D|EA)\d+\b"#).unwrap();
    }
    RE_BIOSAMPLE
        .find(value)
        .map(|m| m.as_str().to_ascii_uppercase())
}

fn biosample_attributes(report: &Json) -> JsonMap<String, Json> {
    let mut out = JsonMap::new();
    if let Some(items) = report.get("attributes").and_then(Json::as_array) {
        for item in items {
            let Some(name) = json_path_string(item, &["name"]) else {
                continue;
            };
            let Some(value) = json_path_string(item, &["value"]) else {
                continue;
            };
            out.insert(name, Json::String(value));
        }
    }
    for key in [
        "age",
        "biomaterial_provider",
        "breed",
        "collected_by",
        "collection_date",
        "cultivar",
        "dev_stage",
        "ecotype",
        "geo_loc_name",
        "host",
        "host_disease",
        "identified_by",
        "isolate",
        "lat_lon",
        "sex",
        "tissue",
    ] {
        if let Some(value) = json_path_string(report, &[key]) {
            out.entry(key.to_string()).or_insert(Json::String(value));
        }
    }
    out
}

fn sample_ids_join(report: &Json) -> Option<String> {
    let items = report.get("sample_ids")?.as_array()?;
    let values = items
        .iter()
        .filter_map(|item| {
            let value = json_path_string(item, &["value"])?;
            let label =
                json_path_string(item, &["label"]).or_else(|| json_path_string(item, &["db"]));
            Some(match label {
                Some(label) => format!("{label}: {value}"),
                None => value,
            })
        })
        .collect::<Vec<_>>();
    (!values.is_empty()).then(|| values.join("; "))
}

fn bioprojects_join(report: &Json) -> Option<String> {
    let items = report.get("bioprojects")?.as_array()?;
    let values = items
        .iter()
        .filter_map(|item| json_path_string(item, &["accession"]).or_else(|| json_string(item)))
        .collect::<Vec<_>>();
    (!values.is_empty()).then(|| values.join("; "))
}

fn json_array_join(value: Option<&Json>) -> Option<String> {
    let values = value?
        .as_array()?
        .iter()
        .filter_map(json_string)
        .collect::<Vec<_>>();
    (!values.is_empty()).then(|| values.join("; "))
}

fn json_path_string(value: &Json, path: &[&str]) -> Option<String> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    json_string(current)
}

fn param_string(params: Option<&Json>, keys: &[&str]) -> Option<String> {
    let object = params?.as_object()?;
    keys.iter()
        .find_map(|key| object.get(*key).and_then(json_string))
}

fn push_labeled(out: &mut Vec<String>, label: &str, value: Option<&str>) {
    if let Some(value) = value.map(str::trim).filter(|s| !s.is_empty()) {
        out.push(format!("{label}: {value}"));
    }
}

fn push_plain(out: &mut Vec<String>, value: Option<&str>) {
    if let Some(value) = value.map(str::trim).filter(|s| !s.is_empty()) {
        out.push(value.to_string());
    }
}

fn insert_extra(map: &mut JsonMap<String, Json>, key: &str, value: Option<String>) {
    if let Some(value) = value.filter(|v| !v.trim().is_empty()) {
        map.insert(key.to_string(), Json::String(value));
    }
}

fn biosample_record_url(accession: &str) -> String {
    format!("https://www.ncbi.nlm.nih.gov/biosample/{accession}")
}

#[cfg(debug_assertions)]
fn mock_biosample_entrez_json(utility: &str, params: &[(String, String)]) -> Option<Json> {
    match utility {
        "esearch" => Some(json!({
            "esearchresult": {
                "count": "1",
                "idlist": ["15960293"],
                "querytranslation": "Gallus gallus[All Fields]"
            }
        })),
        "esummary" if params.iter().any(|(_, value)| value.contains("15960293")) => Some(json!({
            "result": {
                "uids": ["15960293"],
                "15960293": {
                    "uid": "15960293",
                    "title": "Animal sample from Gallus gallus (bGalGal3)",
                    "accession": "SAMN15960293",
                    "publicationdate": "2020/09/01",
                    "modificationdate": "2024/08/20",
                    "organization": "G10K",
                    "taxonomy": "9031",
                    "organism": "Gallus gallus",
                    "sampledata": "<BioSample><Description><Comment><Paragraph>Chicken reference genome sample.</Paragraph></Comment></Description></BioSample>",
                    "identifiers": "BioSample: SAMN15960293; SRA: SRS22402101",
                    "package": "Model organism or animal; version 1.0"
                }
            }
        })),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_biosample_report() {
        let json = json!({
            "reports": [{
                "accession": "SAMN15960293",
                "last_updated": "2024-08-20T04:05:23.840",
                "publication_date": "2020-09-01T00:00:00.000",
                "description": {
                    "title": "Animal sample from Gallus gallus (bGalGal3)",
                    "organism": {"tax_id": 9031, "organism_name": "Gallus gallus"},
                    "comment": "Chicken reference genome sample."
                },
                "owner": {"name": "G10K"},
                "models": ["Model organism or animal"],
                "package": "Model.organism.animal.1.0",
                "attributes": [
                    {"name": "tissue", "value": "Blood"},
                    {"name": "sex", "value": "female"}
                ],
                "status": {"status": "live", "when": "2020-09-01T13:51:06.392"}
            }]
        });
        let records = parse_biosample_report_page(&json);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].source, PublicDataSource::BioSample);
        assert_eq!(records[0].accession, "SAMN15960293");
        assert!(records[0].summary.contains("tissue: Blood"));
        assert_eq!(
            records[0].extra["attributes"]["sex"].as_str(),
            Some("female")
        );
    }

    #[test]
    fn parses_biosample_esearch_and_esummary() {
        let search = json!({"esearchresult": {"count": "1", "idlist": ["15960293"]}});
        let (count, ids, _) = parse_biosample_esearch(&search).unwrap();
        assert_eq!(count, 1);
        assert_eq!(ids, vec!["15960293"]);

        let summary =
            mock_biosample_entrez_json("esummary", &[("id".to_string(), "15960293".to_string())])
                .unwrap();
        let records = parse_biosample_esummary(&summary, &ids);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].accession, "SAMN15960293");
        assert!(records[0]
            .summary
            .contains("Chicken reference genome sample"));
    }

    #[test]
    fn recognizes_biosample_accessions_and_urls() {
        assert!(looks_like_biosample_accession("SAMN15960293"));
        assert!(looks_like_biosample_accession(
            "https://www.ncbi.nlm.nih.gov/biosample/SAMEA12345"
        ));
        assert_eq!(
            normalize_biosample_accession(
                "https://api.ncbi.nlm.nih.gov/datasets/v2/biosample/accession/SAMD123/biosample_report"
            )
            .as_deref(),
            Some("SAMD123")
        );
    }
}
