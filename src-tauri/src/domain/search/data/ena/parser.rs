use super::{fields, query};
use crate::domain::search::data::common;
use crate::domain::search::data::common::{
    json_u64_from_keys, string_field_any, string_vec_field_any, DataRecord, PublicDataSource,
};
use serde_json::{json, Map as JsonMap, Value as Json};

pub(super) fn parse_ena_portal_json(source: PublicDataSource, value: &Json) -> Vec<DataRecord> {
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
    let accession = string_field_any(map, fields::ena_accession_fields(source))?;
    let display_accession = string_field_any(map, fields::ena_display_accession_fields(source))
        .unwrap_or_else(|| accession.clone());
    let title = string_field_any(map, fields::ena_title_fields(source))
        .or_else(|| string_field_any(map, &["description"]))
        .unwrap_or_else(|| accession.clone());
    let summary = string_field_any(map, fields::ena_summary_fields(source)).unwrap_or_default();
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
        title: common::clean_html_text(&title),
        summary: common::clean_html_text(&summary),
        url: query::ena_record_url(&accession),
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

pub(super) fn parse_ena_xml_record(
    source: PublicDataSource,
    xml: &str,
    fallback_accession: &str,
) -> Option<DataRecord> {
    let accession = common::extract_xml_attr(xml, "STUDY", "accession")
        .or_else(|| common::extract_xml_attr(xml, "SAMPLE", "accession"))
        .or_else(|| common::extract_xml_attr(xml, "RUN", "accession"))
        .or_else(|| common::extract_xml_attr(xml, "EXPERIMENT", "accession"))
        .or_else(|| common::extract_xml_attr(xml, "ANALYSIS", "accession"))
        .or_else(|| common::extract_xml_attr(xml, "ASSEMBLY", "accession"))
        .or_else(|| common::extract_xml_attr(xml, "SEQUENCE", "accession"))
        .unwrap_or_else(|| fallback_accession.to_string());
    let title = common::extract_first_xml_tag(
        xml,
        &["STUDY_TITLE", "TITLE", "SAMPLE_TITLE", "DESCRIPTION"],
    )
    .unwrap_or_else(|| accession.clone());
    let summary =
        common::extract_first_xml_tag(xml, &["STUDY_ABSTRACT", "DESCRIPTION", "ABSTRACT"])
            .unwrap_or_default();
    let center = common::extract_xml_attr(xml, "STUDY", "center_name")
        .or_else(|| common::extract_xml_attr(xml, "SAMPLE", "center_name"))
        .or_else(|| common::extract_xml_attr(xml, "RUN", "center_name"))
        .or_else(|| common::extract_xml_attr(xml, "EXPERIMENT", "center_name"))
        .or_else(|| common::extract_xml_attr(xml, "ANALYSIS", "center_name"))
        .or_else(|| common::extract_first_xml_tag(xml, &["CENTER_NAME"]));
    let alias = common::extract_xml_attr(xml, "STUDY", "alias")
        .or_else(|| common::extract_xml_attr(xml, "SAMPLE", "alias"))
        .or_else(|| common::extract_xml_attr(xml, "RUN", "alias"))
        .or_else(|| common::extract_xml_attr(xml, "EXPERIMENT", "alias"))
        .or_else(|| common::extract_xml_attr(xml, "ANALYSIS", "alias"));
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
        title: common::clean_xml_fragment(&title),
        summary: common::clean_xml_fragment(&summary),
        url: query::ena_record_url(&accession),
        record_type: Some(format!(
            "{} xml_record",
            source.ena_result().unwrap_or("ena")
        )),
        organism: common::extract_first_xml_tag(xml, &["SCIENTIFIC_NAME", "TAXON"]),
        published_date: None,
        updated_date: None,
        sample_count: None,
        platform: None,
        files: common::extract_ena_file_links(xml),
        extra,
    })
}
