use super::fields;
use crate::domain::search::data::common::{self, PublicDataSource};

pub(super) fn ena_portal_query(source: PublicDataSource, query: &str) -> String {
    let query = query.trim();
    if looks_like_ena_advanced_query(query) {
        return query.to_string();
    }
    let escaped = escape_ena_query_value(query);
    fields::ena_simple_search_fields(source)
        .iter()
        .map(|field| format!("{field}=\"*{escaped}*\""))
        .collect::<Vec<_>>()
        .join(" OR ")
}

pub(super) fn looks_like_ena_advanced_query(query: &str) -> bool {
    let lower = query.to_ascii_lowercase();
    query.contains('=')
        || lower.contains(" and ")
        || lower.contains(" or ")
        || lower.contains("tax_")
        || lower.contains("country")
        || lower.contains("scientific_name")
}

pub(super) fn ena_accession_query(source: PublicDataSource, accession: &str) -> String {
    let escaped = escape_ena_query_value(accession);
    ena_accession_query_fields(source, infer_ena_source_from_accession(accession))
        .iter()
        .map(|field| format!("{field}=\"{escaped}\""))
        .collect::<Vec<_>>()
        .join(" OR ")
}

pub(super) fn ena_accession_query_fields(
    source: PublicDataSource,
    accession_source: Option<PublicDataSource>,
) -> Vec<&'static str> {
    match (source, accession_source) {
        (
            PublicDataSource::Geo
            | PublicDataSource::CbioPortal
            | PublicDataSource::Gtex
            | PublicDataSource::NcbiDatasets
            | PublicDataSource::BioSample,
            _,
        ) => vec!["accession"],
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

pub(super) fn escape_ena_query_value(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

pub(super) fn looks_like_ena_accession(value: &str) -> bool {
    let Some(accession) = common::normalize_accession(value) else {
        return false;
    };
    infer_ena_source_from_accession(&accession).is_some()
}

pub(super) fn inferred_ena_source_key(value: &str) -> Option<&'static str> {
    infer_ena_source_from_accession(value).map(PublicDataSource::as_str)
}

pub(super) fn infer_ena_source_from_accession(value: &str) -> Option<PublicDataSource> {
    let accession = common::normalize_accession(value)?;
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

pub(super) fn ena_record_url(accession: &str) -> String {
    format!("https://www.ebi.ac.uk/ena/browser/view/{accession}")
}
