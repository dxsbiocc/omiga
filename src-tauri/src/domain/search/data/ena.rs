//! ENA Portal and Browser API adapter.

use super::common::*;
use super::PublicDataClient;
use serde_json::{json, Map as JsonMap, Value as Json};

impl PublicDataClient {
    pub(super) async fn search_ena(
        &self,
        source: PublicDataSource,
        args: DataSearchArgs,
    ) -> Result<DataSearchResponse, String> {
        let limit = args.normalized_max_results();
        let result = source
            .ena_result()
            .ok_or_else(|| "GEO is not an ENA source".to_string())?;
        let query = ena_portal_query(source, args.query.trim());
        let fields = ena_fields(source);
        let response = self
            .http
            .get(&self.base_urls.ena_portal_search)
            .query(&[
                ("result", result.to_string()),
                ("query", query),
                ("fields", fields),
                ("format", "json".to_string()),
                ("limit", limit.to_string()),
            ])
            .send()
            .await
            .map_err(|e| format!("ENA Portal search request failed: {e}"))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| format!("read ENA Portal search response: {e}"))?;
        if !status.is_success() {
            return Err(format!(
                "ENA Portal search returned HTTP {}: {}",
                status.as_u16(),
                truncate_for_error(&body)
            ));
        }
        let json: Json =
            serde_json::from_str(&body).map_err(|e| format!("parse ENA Portal JSON: {e}"))?;
        let results = parse_ena_portal_json(source, &json);
        Ok(DataSearchResponse {
            query: args.query.trim().to_string(),
            source: source.as_str().to_string(),
            total: Some(results.len() as u64),
            results,
            notes: vec![
                format!("ENA Portal API {result} search"),
                "Simple free-text queries are translated to source-specific wildcard fields; advanced ENA query syntax is passed through.".to_string(),
            ],
        })
    }

    pub(super) async fn fetch_ena(
        &self,
        source: PublicDataSource,
        identifier: &str,
    ) -> Result<DataRecord, String> {
        let accession = normalize_accession(identifier)
            .ok_or_else(|| "ENA fetch requires an accession or ENA Browser URL".to_string())?;
        let source = if matches!(source, PublicDataSource::EnaStudy) {
            infer_ena_source_from_accession(&accession).unwrap_or(source)
        } else {
            source
        };
        let result = source
            .ena_result()
            .ok_or_else(|| "GEO is not an ENA source".to_string())?;
        let query = ena_accession_query(source, &accession);
        let fields = ena_fields(source);
        let response = self
            .http
            .get(&self.base_urls.ena_portal_search)
            .query(&[
                ("result", result.to_string()),
                ("query", query),
                ("fields", fields),
                ("format", "json".to_string()),
                ("limit", "1".to_string()),
            ])
            .send()
            .await
            .map_err(|e| format!("ENA Portal fetch request failed: {e}"))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| format!("read ENA Portal fetch response: {e}"))?;
        if status.is_success() {
            let json: Json =
                serde_json::from_str(&body).map_err(|e| format!("parse ENA Portal JSON: {e}"))?;
            if let Some(record) = parse_ena_portal_json(source, &json).into_iter().next() {
                return Ok(record);
            }
        }

        let url = format!(
            "{}/{}",
            self.base_urls.ena_browser_xml,
            encode_path_segment(&accession)
        );
        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| format!("ENA Browser XML fetch request failed: {e}"))?;
        let status = response.status();
        let xml = response
            .text()
            .await
            .map_err(|e| format!("read ENA Browser XML response: {e}"))?;
        if !status.is_success() {
            return Err(format!(
                "ENA fetch returned HTTP {}: {}",
                status.as_u16(),
                truncate_for_error(&xml)
            ));
        }
        parse_ena_xml_record(source, &xml, &accession)
            .ok_or_else(|| format!("ENA did not return a parseable record for `{accession}`"))
    }
}

fn parse_ena_portal_json(source: PublicDataSource, value: &Json) -> Vec<DataRecord> {
    let Some(items) = value.as_array() else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| parse_ena_portal_item(source, item))
        .collect()
}

fn parse_ena_portal_item(source: PublicDataSource, item: &Json) -> Option<DataRecord> {
    let map = item.as_object()?;
    let accession = string_field_any(map, ena_accession_fields(source))?;
    let display_accession = string_field_any(map, ena_display_accession_fields(source))
        .unwrap_or_else(|| accession.clone());
    let title = string_field_any(map, ena_title_fields(source))
        .or_else(|| string_field_any(map, &["description"]))
        .unwrap_or_else(|| accession.clone());
    let summary = string_field_any(map, ena_summary_fields(source)).unwrap_or_default();
    let organism = string_field_any(map, &["scientific_name", "organism", "taxon"]);
    let published_date = string_field_any(map, &["first_public", "first_publication"]);
    let updated_date = string_field_any(map, &["last_updated", "last_update"]);
    let sample_count = json_u64_from_keys(map, &["sample_count", "samples"]);
    let platform = string_field_any(
        map,
        &["instrument_platform", "instrument_model", "platform"],
    );
    let files = string_vec_field_any(
        map,
        &[
            "submitted_ftp",
            "fastq_ftp",
            "fasta_ftp",
            "cram_ftp",
            "bam_ftp",
            "sra_ftp",
            "generated_ftp",
        ],
    );
    let mut extra = JsonMap::new();
    for key in [
        "secondary_study_accession",
        "study_accession",
        "experiment_accession",
        "run_accession",
        "sample_accession",
        "analysis_accession",
        "assembly_accession",
        "analysis_title",
        "analysis_alias",
        "analysis_description",
        "assembly_title",
        "center_name",
        "tax_id",
        "study_alias",
        "sample_alias",
        "experiment_alias",
        "library_strategy",
        "library_source",
        "analysis_type",
        "assembly_type",
        "country",
        "collection_date",
        "host",
        "host_tax_id",
        "host_body_site",
        "specimen_voucher",
        "bio_material",
        "fastq_md5",
        "sra_md5",
        "submitted_md5",
        "generated_md5",
    ] {
        if let Some(value) = map.get(key) {
            extra.insert(key.to_string(), value.clone());
        }
    }
    Some(DataRecord {
        id: accession.clone(),
        accession: display_accession,
        source,
        title: clean_html_text(&title),
        summary: clean_html_text(&summary),
        url: ena_record_url(&accession),
        record_type: source.ena_result().map(str::to_string),
        organism,
        published_date,
        updated_date,
        sample_count,
        platform,
        files,
        extra,
    })
}

fn parse_ena_xml_record(
    source: PublicDataSource,
    xml: &str,
    fallback_accession: &str,
) -> Option<DataRecord> {
    let accession = extract_xml_attr(xml, "STUDY", "accession")
        .or_else(|| extract_xml_attr(xml, "SAMPLE", "accession"))
        .or_else(|| extract_xml_attr(xml, "RUN", "accession"))
        .or_else(|| extract_xml_attr(xml, "EXPERIMENT", "accession"))
        .or_else(|| extract_xml_attr(xml, "ANALYSIS", "accession"))
        .or_else(|| extract_xml_attr(xml, "ASSEMBLY", "accession"))
        .or_else(|| extract_xml_attr(xml, "SEQUENCE", "accession"))
        .unwrap_or_else(|| fallback_accession.to_string());
    let title = extract_first_xml_tag(
        xml,
        &["STUDY_TITLE", "TITLE", "SAMPLE_TITLE", "DESCRIPTION"],
    )
    .unwrap_or_else(|| accession.clone());
    let summary = extract_first_xml_tag(xml, &["STUDY_ABSTRACT", "DESCRIPTION", "ABSTRACT"])
        .unwrap_or_default();
    let center = extract_xml_attr(xml, "STUDY", "center_name")
        .or_else(|| extract_xml_attr(xml, "SAMPLE", "center_name"))
        .or_else(|| extract_xml_attr(xml, "RUN", "center_name"))
        .or_else(|| extract_xml_attr(xml, "EXPERIMENT", "center_name"))
        .or_else(|| extract_xml_attr(xml, "ANALYSIS", "center_name"))
        .or_else(|| extract_first_xml_tag(xml, &["CENTER_NAME"]));
    let alias = extract_xml_attr(xml, "STUDY", "alias")
        .or_else(|| extract_xml_attr(xml, "SAMPLE", "alias"))
        .or_else(|| extract_xml_attr(xml, "RUN", "alias"))
        .or_else(|| extract_xml_attr(xml, "EXPERIMENT", "alias"))
        .or_else(|| extract_xml_attr(xml, "ANALYSIS", "alias"));
    let mut extra = JsonMap::new();
    if let Some(center) = center {
        extra.insert("center_name".to_string(), json!(center));
    }
    if let Some(alias) = alias {
        extra.insert("alias".to_string(), json!(alias));
    }
    Some(DataRecord {
        id: accession.clone(),
        accession: accession.clone(),
        source,
        title: clean_xml_fragment(&title),
        summary: clean_xml_fragment(&summary),
        url: ena_record_url(&accession),
        record_type: Some(format!(
            "{} xml_record",
            source.ena_result().unwrap_or("ena")
        )),
        organism: extract_first_xml_tag(xml, &["SCIENTIFIC_NAME", "TAXON"]),
        published_date: None,
        updated_date: None,
        sample_count: None,
        platform: None,
        files: extract_ena_file_links(xml),
        extra,
    })
}

fn ena_fields(source: PublicDataSource) -> String {
    match source {
        PublicDataSource::Geo | PublicDataSource::CbioPortal | PublicDataSource::Gtex => Vec::new(),
        PublicDataSource::EnaStudy => vec![
            "study_accession",
            "secondary_study_accession",
            "study_title",
            "description",
            "study_alias",
            "center_name",
            "tax_id",
            "scientific_name",
            "first_public",
            "last_updated",
        ],
        PublicDataSource::EnaRun => vec![
            "run_accession",
            "experiment_accession",
            "sample_accession",
            "study_accession",
            "secondary_study_accession",
            "scientific_name",
            "instrument_platform",
            "instrument_model",
            "library_strategy",
            "library_source",
            "first_public",
            "last_updated",
            "fastq_ftp",
            "fastq_md5",
            "submitted_ftp",
            "submitted_md5",
            "sra_ftp",
            "sra_md5",
        ],
        PublicDataSource::EnaExperiment => vec![
            "experiment_accession",
            "study_accession",
            "sample_accession",
            "experiment_title",
            "experiment_alias",
            "scientific_name",
            "instrument_platform",
            "instrument_model",
            "library_strategy",
            "library_source",
            "first_public",
            "last_updated",
        ],
        PublicDataSource::EnaSample => vec![
            "sample_accession",
            "secondary_sample_accession",
            "sample_alias",
            "scientific_name",
            "tax_id",
            "description",
            "country",
            "collection_date",
            "host",
            "host_tax_id",
            "first_public",
            "last_updated",
        ],
        PublicDataSource::EnaAnalysis => vec![
            "analysis_accession",
            "study_accession",
            "sample_accession",
            "analysis_title",
            "analysis_description",
            "analysis_alias",
            "analysis_type",
            "assembly_type",
            "description",
            "scientific_name",
            "first_public",
            "last_updated",
            "submitted_ftp",
            "submitted_md5",
            "generated_ftp",
            "generated_md5",
        ],
        PublicDataSource::EnaAssembly => vec![
            "assembly_accession",
            "scientific_name",
            "tax_id",
            "assembly_name",
            "assembly_title",
            "assembly_level",
            "description",
            "last_updated",
        ],
        PublicDataSource::EnaSequence => vec![
            "accession",
            "description",
            "scientific_name",
            "tax_id",
            "specimen_voucher",
            "bio_material",
            "first_public",
            "last_updated",
        ],
    }
    .join(",")
}

fn ena_portal_query(source: PublicDataSource, query: &str) -> String {
    let query = query.trim();
    if looks_like_ena_advanced_query(query) {
        return query.to_string();
    }
    let escaped = escape_ena_query_value(query);
    ena_simple_search_fields(source)
        .iter()
        .map(|field| format!("{field}=\"*{escaped}*\""))
        .collect::<Vec<_>>()
        .join(" OR ")
}

fn looks_like_ena_advanced_query(query: &str) -> bool {
    let lower = query.to_ascii_lowercase();
    query.contains('=')
        || lower.contains(" and ")
        || lower.contains(" or ")
        || lower.contains("tax_")
        || lower.contains("country")
        || lower.contains("scientific_name")
}

fn ena_simple_search_fields(source: PublicDataSource) -> &'static [&'static str] {
    match source {
        PublicDataSource::Geo | PublicDataSource::CbioPortal | PublicDataSource::Gtex => {
            &["description"]
        }
        PublicDataSource::EnaStudy => &["study_title", "description"],
        PublicDataSource::EnaRun => &["description", "scientific_name", "study_title"],
        PublicDataSource::EnaExperiment => &["experiment_title", "description", "scientific_name"],
        PublicDataSource::EnaSample => &["description", "scientific_name", "sample_alias"],
        PublicDataSource::EnaAnalysis => &[
            "analysis_title",
            "analysis_description",
            "description",
            "analysis_type",
            "scientific_name",
        ],
        PublicDataSource::EnaAssembly => &[
            "assembly_name",
            "assembly_title",
            "description",
            "scientific_name",
        ],
        PublicDataSource::EnaSequence => &["description", "scientific_name"],
    }
}

fn ena_accession_fields(source: PublicDataSource) -> &'static [&'static str] {
    match source {
        PublicDataSource::Geo | PublicDataSource::CbioPortal | PublicDataSource::Gtex => {
            &["accession"]
        }
        PublicDataSource::EnaStudy => {
            &["study_accession", "secondary_study_accession", "accession"]
        }
        PublicDataSource::EnaRun => &["run_accession", "accession"],
        PublicDataSource::EnaExperiment => &["experiment_accession", "accession"],
        PublicDataSource::EnaSample => &[
            "sample_accession",
            "secondary_sample_accession",
            "accession",
        ],
        PublicDataSource::EnaAnalysis => &["analysis_accession", "accession"],
        PublicDataSource::EnaAssembly => &["assembly_accession", "accession"],
        PublicDataSource::EnaSequence => &["accession"],
    }
}

fn ena_display_accession_fields(source: PublicDataSource) -> &'static [&'static str] {
    match source {
        PublicDataSource::EnaStudy => {
            &["secondary_study_accession", "study_accession", "accession"]
        }
        PublicDataSource::EnaSample => &[
            "secondary_sample_accession",
            "sample_accession",
            "accession",
        ],
        _ => ena_accession_fields(source),
    }
}

fn ena_title_fields(source: PublicDataSource) -> &'static [&'static str] {
    match source {
        PublicDataSource::Geo | PublicDataSource::CbioPortal | PublicDataSource::Gtex => &["title"],
        PublicDataSource::EnaStudy => &["study_title", "title", "description"],
        PublicDataSource::EnaRun => &["run_alias", "description", "run_accession"],
        PublicDataSource::EnaExperiment => &["experiment_title", "experiment_alias", "description"],
        PublicDataSource::EnaSample => &["sample_alias", "description", "scientific_name"],
        PublicDataSource::EnaAnalysis => &[
            "analysis_title",
            "analysis_alias",
            "analysis_description",
            "description",
            "analysis_accession",
        ],
        PublicDataSource::EnaAssembly => &[
            "assembly_name",
            "assembly_title",
            "description",
            "assembly_accession",
        ],
        PublicDataSource::EnaSequence => &["description", "accession"],
    }
}

fn ena_summary_fields(source: PublicDataSource) -> &'static [&'static str] {
    match source {
        PublicDataSource::Geo | PublicDataSource::CbioPortal | PublicDataSource::Gtex => {
            &["summary"]
        }
        PublicDataSource::EnaStudy => &["study_description", "description"],
        PublicDataSource::EnaRun => &[
            "description",
            "library_strategy",
            "library_source",
            "instrument_model",
        ],
        PublicDataSource::EnaExperiment => &[
            "experiment_title",
            "experiment_alias",
            "library_strategy",
            "library_source",
        ],
        PublicDataSource::EnaSample => &["description", "sample_alias", "country"],
        PublicDataSource::EnaAnalysis => &[
            "analysis_description",
            "analysis_title",
            "description",
            "analysis_type",
            "assembly_type",
        ],
        PublicDataSource::EnaAssembly => &[
            "description",
            "assembly_title",
            "assembly_name",
            "assembly_level",
        ],
        PublicDataSource::EnaSequence => &["description", "scientific_name"],
    }
}

fn ena_accession_query(source: PublicDataSource, accession: &str) -> String {
    let escaped = escape_ena_query_value(accession);
    ena_accession_query_fields(source, infer_ena_source_from_accession(accession))
        .iter()
        .map(|field| format!("{field}=\"{escaped}\""))
        .collect::<Vec<_>>()
        .join(" OR ")
}

fn ena_accession_query_fields(
    source: PublicDataSource,
    accession_source: Option<PublicDataSource>,
) -> Vec<&'static str> {
    match (source, accession_source) {
        (PublicDataSource::Geo | PublicDataSource::CbioPortal | PublicDataSource::Gtex, _) => {
            vec!["accession"]
        }
        (PublicDataSource::EnaStudy, _) => vec!["study_accession", "secondary_study_accession"],
        (PublicDataSource::EnaRun, Some(PublicDataSource::EnaStudy)) => {
            vec!["study_accession", "secondary_study_accession"]
        }
        (PublicDataSource::EnaRun, Some(PublicDataSource::EnaExperiment)) => {
            vec!["experiment_accession"]
        }
        (PublicDataSource::EnaRun, Some(PublicDataSource::EnaSample)) => {
            vec!["sample_accession", "secondary_sample_accession"]
        }
        (PublicDataSource::EnaRun, _) => vec!["run_accession"],
        (PublicDataSource::EnaExperiment, Some(PublicDataSource::EnaStudy)) => {
            vec!["study_accession", "secondary_study_accession"]
        }
        (PublicDataSource::EnaExperiment, Some(PublicDataSource::EnaSample)) => {
            vec!["sample_accession", "secondary_sample_accession"]
        }
        (PublicDataSource::EnaExperiment, _) => vec!["experiment_accession"],
        (PublicDataSource::EnaSample, _) => vec!["sample_accession", "secondary_sample_accession"],
        (PublicDataSource::EnaAnalysis, Some(PublicDataSource::EnaStudy)) => {
            vec!["study_accession", "secondary_study_accession"]
        }
        (PublicDataSource::EnaAnalysis, Some(PublicDataSource::EnaSample)) => {
            vec!["sample_accession", "secondary_sample_accession"]
        }
        (PublicDataSource::EnaAnalysis, _) => vec!["analysis_accession"],
        (PublicDataSource::EnaAssembly, _) => vec!["assembly_accession"],
        (PublicDataSource::EnaSequence, _) => vec!["accession"],
    }
}

fn escape_ena_query_value(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

pub fn looks_like_ena_accession(value: &str) -> bool {
    let Some(accession) = normalize_accession(value) else {
        return false;
    };
    infer_ena_source_from_accession(&accession).is_some()
}

pub fn inferred_ena_source_key(value: &str) -> Option<&'static str> {
    infer_ena_source_from_accession(value).map(PublicDataSource::as_str)
}

fn infer_ena_source_from_accession(value: &str) -> Option<PublicDataSource> {
    let accession = normalize_accession(value)?;
    let upper = accession.to_ascii_uppercase();
    if upper.starts_with("PRJ")
        || upper.starts_with("ERP")
        || upper.starts_with("SRP")
        || upper.starts_with("DRP")
    {
        return Some(PublicDataSource::EnaStudy);
    }
    if upper.starts_with("ERX") || upper.starts_with("SRX") || upper.starts_with("DRX") {
        return Some(PublicDataSource::EnaExperiment);
    }
    if upper.starts_with("ERR") || upper.starts_with("SRR") || upper.starts_with("DRR") {
        return Some(PublicDataSource::EnaRun);
    }
    if upper.starts_with("ERS")
        || upper.starts_with("SRS")
        || upper.starts_with("DRS")
        || upper.starts_with("SAM")
    {
        return Some(PublicDataSource::EnaSample);
    }
    if upper.starts_with("ERZ") || upper.starts_with("SRZ") || upper.starts_with("DRZ") {
        return Some(PublicDataSource::EnaAnalysis);
    }
    if upper.starts_with("GCA_") || upper.starts_with("GCF_") {
        return Some(PublicDataSource::EnaAssembly);
    }
    None
}

fn ena_record_url(accession: &str) -> String {
    format!("https://www.ebi.ac.uk/ena/browser/view/{accession}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_ena_portal_json() {
        let value = json!([{
            "study_accession": "PRJEB123",
            "secondary_study_accession": "ERP123",
            "study_title": "Metagenome study",
            "description": "Rumen samples",
            "center_name": "EBI",
            "scientific_name": "cow metagenome",
            "first_public": "2024-01-01",
            "last_updated": "2024-02-01"
        }]);
        let records = parse_ena_portal_json(PublicDataSource::EnaStudy, &value);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, "PRJEB123");
        assert_eq!(records[0].accession, "ERP123");
        assert_eq!(records[0].organism.as_deref(), Some("cow metagenome"));
        assert!(records[0].url.ends_with("/PRJEB123"));
    }

    #[test]
    fn parses_ena_run_portal_json_with_file_links() {
        let value = json!([{
            "run_accession": "ERR123",
            "experiment_accession": "ERX123",
            "sample_accession": "ERS123",
            "study_accession": "PRJEB123",
            "scientific_name": "Homo sapiens",
            "instrument_platform": "ILLUMINA",
            "instrument_model": "Illumina NovaSeq 6000",
            "library_strategy": "RNA-Seq",
            "fastq_ftp": "ftp.sra.ebi.ac.uk/vol1/fastq/ERR123/ERR123_1.fastq.gz;ftp.sra.ebi.ac.uk/vol1/fastq/ERR123/ERR123_2.fastq.gz"
        }]);
        let records = parse_ena_portal_json(PublicDataSource::EnaRun, &value);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, "ERR123");
        assert_eq!(records[0].source, PublicDataSource::EnaRun);
        assert_eq!(records[0].platform.as_deref(), Some("ILLUMINA"));
        assert_eq!(records[0].files.len(), 2);
        assert_eq!(
            records[0].extra["library_strategy"].as_str(),
            Some("RNA-Seq")
        );
    }

    #[test]
    fn parses_ena_xml_record() {
        let xml = r#"
        <STUDY_SET>
          <STUDY accession="PRJEB999" alias="alias-1" center_name="EBI">
            <DESCRIPTOR>
              <STUDY_TITLE>XML Study</STUDY_TITLE>
              <STUDY_ABSTRACT>XML abstract &amp; details.</STUDY_ABSTRACT>
            </DESCRIPTOR>
            <STUDY_LINKS>
              <STUDY_LINK>
                <XREF_LINK><DB>ENA-FASTQ-FILES</DB><URL>ftp://example/file.fastq.gz</URL></XREF_LINK>
              </STUDY_LINK>
            </STUDY_LINKS>
          </STUDY>
        </STUDY_SET>
        "#;
        let record = parse_ena_xml_record(PublicDataSource::EnaStudy, xml, "fallback").unwrap();
        assert_eq!(record.accession, "PRJEB999");
        assert_eq!(record.title, "XML Study");
        assert_eq!(record.summary, "XML abstract & details.");
        assert_eq!(record.files, vec!["ftp://example/file.fastq.gz"]);
    }

    #[test]
    fn builds_ena_queries_and_detects_record_types() {
        assert_eq!(
            PublicDataSource::parse("ena_run"),
            Some(PublicDataSource::EnaRun)
        );
        assert_eq!(
            PublicDataSource::parse("read_experiment"),
            Some(PublicDataSource::EnaExperiment)
        );
        assert_eq!(PublicDataSource::EnaRun.ena_result(), Some("read_run"));
        assert_eq!(
            ena_portal_query(PublicDataSource::EnaStudy, "rumen"),
            "study_title=\"*rumen*\" OR description=\"*rumen*\""
        );
        assert_eq!(
            ena_portal_query(PublicDataSource::EnaRun, "rumen"),
            "description=\"*rumen*\" OR scientific_name=\"*rumen*\" OR study_title=\"*rumen*\""
        );
        assert_eq!(
            ena_portal_query(
                PublicDataSource::EnaRun,
                "country=\"United Kingdom\" AND host_tax_id=9913"
            ),
            "country=\"United Kingdom\" AND host_tax_id=9913"
        );
        assert_eq!(
            ena_accession_query(PublicDataSource::EnaRun, "ERR123"),
            "run_accession=\"ERR123\""
        );
        assert_eq!(
            ena_accession_query(PublicDataSource::EnaRun, "ERX123"),
            "experiment_accession=\"ERX123\""
        );
        assert_eq!(
            ena_accession_query(PublicDataSource::EnaRun, "PRJEB123"),
            "study_accession=\"PRJEB123\" OR secondary_study_accession=\"PRJEB123\""
        );
        assert_eq!(
            ena_accession_query(PublicDataSource::EnaAnalysis, "SAMEA123"),
            "sample_accession=\"SAMEA123\" OR secondary_sample_accession=\"SAMEA123\""
        );
        assert_eq!(
            infer_ena_source_from_accession("ERX123"),
            Some(PublicDataSource::EnaExperiment)
        );
        assert_eq!(
            infer_ena_source_from_accession("ERZ123"),
            Some(PublicDataSource::EnaAnalysis)
        );
        let study_fields = ena_fields(PublicDataSource::EnaStudy);
        assert!(study_fields.contains("description"));
        assert!(!study_fields.contains("study_description"));
        let assembly_fields = ena_fields(PublicDataSource::EnaAssembly);
        assert!(assembly_fields.contains("assembly_title"));
        assert!(!assembly_fields.contains("first_public"));
        assert!(ena_fields(PublicDataSource::EnaAnalysis).contains("generated_ftp"));
    }
}
