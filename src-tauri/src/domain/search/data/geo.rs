//! NCBI GEO DataSets adapter backed by Entrez E-utilities.

use super::common::*;
use super::PublicDataClient;
use serde_json::{json, Map as JsonMap, Value as Json};

impl PublicDataClient {
    pub(super) async fn search_geo(
        &self,
        args: DataSearchArgs,
    ) -> Result<DataSearchResponse, String> {
        let ret_max = args.normalized_max_results();
        let mut params = self.geo_entrez_params("json");
        params.push(("term".to_string(), args.query.trim().to_string()));
        params.push(("retmax".to_string(), ret_max.to_string()));

        let search_json = self.get_entrez_json("esearch", &params).await?;
        let (count, ids, query_translation) = parse_geo_esearch(&search_json)?;
        if ids.is_empty() {
            return Ok(DataSearchResponse {
                query: args.query.trim().to_string(),
                source: "geo".to_string(),
                total: Some(count),
                results: Vec::new(),
                notes: vec![
                    "NCBI GEO DataSets ESearch returned no matching UIDs".to_string(),
                    query_translation
                        .map(|q| format!("NCBI query translation: {q}"))
                        .unwrap_or_default(),
                ]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect(),
            });
        }

        let mut summary_params = self.geo_entrez_params("json");
        summary_params.push(("id".to_string(), ids.join(",")));
        let summary_json = self.get_entrez_json("esummary", &summary_params).await?;
        let results = parse_geo_esummary(&summary_json, &ids);
        let mut notes = vec!["NCBI Entrez E-utilities db=gds".to_string()];
        if let Some(q) = query_translation {
            notes.push(format!("NCBI query translation: {q}"));
        }
        Ok(DataSearchResponse {
            query: args.query.trim().to_string(),
            source: "geo".to_string(),
            total: Some(count),
            results,
            notes,
        })
    }

    pub(super) async fn fetch_geo(&self, identifier: &str) -> Result<DataRecord, String> {
        let uid = if identifier.chars().all(|c| c.is_ascii_digit()) {
            identifier.to_string()
        } else {
            let mut params = self.geo_entrez_params("json");
            params.push(("term".to_string(), format!("{}[ACCN]", identifier.trim())));
            params.push(("retmax".to_string(), "1".to_string()));
            let json = self.get_entrez_json("esearch", &params).await?;
            let (_, ids, _) = parse_geo_esearch(&json)?;
            ids.into_iter()
                .next()
                .ok_or_else(|| format!("GEO did not find accession `{identifier}`"))?
        };
        let mut params = self.geo_entrez_params("json");
        params.push(("id".to_string(), uid.clone()));
        let json = self.get_entrez_json("esummary", &params).await?;
        parse_geo_esummary(&json, std::slice::from_ref(&uid))
            .into_iter()
            .next()
            .ok_or_else(|| format!("GEO did not return a parseable summary for `{uid}`"))
    }

    fn geo_entrez_params(&self, retmode: &str) -> Vec<(String, String)> {
        let mut params = vec![
            ("db".to_string(), "gds".to_string()),
            ("retmode".to_string(), retmode.to_string()),
            ("tool".to_string(), self.settings.tool.clone()),
            ("email".to_string(), self.settings.email.clone()),
        ];
        if let Some(api_key) = &self.settings.api_key {
            params.push(("api_key".to_string(), api_key.clone()));
        }
        params
    }

    async fn get_entrez_json(
        &self,
        utility: &str,
        params: &[(String, String)],
    ) -> Result<Json, String> {
        let url = format!("{}/{}.fcgi", self.base_urls.entrez, utility);
        let response = self
            .http
            .get(&url)
            .query(params)
            .send()
            .await
            .map_err(|e| format!("NCBI Entrez {utility} request failed: {e}"))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| format!("NCBI Entrez {utility} response read failed: {e}"))?;
        if !status.is_success() {
            return Err(format!(
                "NCBI Entrez {utility} returned HTTP {status}: {}",
                truncate_for_error(&body)
            ));
        }
        let json: Json = serde_json::from_str(&body).map_err(|e| {
            format!(
                "NCBI Entrez {utility} returned non-JSON response: {e}; body: {}",
                truncate_for_error(&body)
            )
        })?;
        if let Some(error) = json.get("error").and_then(Json::as_str) {
            return Err(format!("NCBI Entrez {utility} error: {error}"));
        }
        Ok(json)
    }
}

fn parse_geo_esearch(value: &Json) -> Result<(u64, Vec<String>, Option<String>), String> {
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

fn parse_geo_esummary(value: &Json, ordered_ids: &[String]) -> Vec<DataRecord> {
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

pub fn looks_like_geo_accession(value: &str) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_geo_esearch_and_esummary() {
        let search = json!({"esearchresult": {"count": "1", "idlist": ["200000001"], "querytranslation": "cancer"}});
        let (count, ids, translation) = parse_geo_esearch(&search).unwrap();
        assert_eq!(count, 1);
        assert_eq!(ids, vec!["200000001"]);
        assert_eq!(translation.as_deref(), Some("cancer"));

        let summary = json!({"result": {"uids": ["200000001"], "200000001": {"uid": "200000001", "accession": "GSE123", "title": "<b>RNA-seq study</b>", "summary": "A useful dataset", "gdsType": "Expression profiling by high throughput sequencing", "taxon": "Homo sapiens", "n_samples": "42", "GPL": "GPL20301", "PDAT": "2024/01/02"}}});
        let records = parse_geo_esummary(&summary, &ids);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].accession, "GSE123");
        assert_eq!(records[0].title, "RNA-seq study");
        assert_eq!(records[0].sample_count, Some(42));
        assert!(records[0].url.contains("GSE123"));
    }
}
