use super::super::common::ensure_registry_source_can_query;
use crate::domain::retrieval_registry;
use crate::domain::search::data::PublicDataSource;
use crate::domain::tools::{ToolContext, ToolError};

pub(super) fn resolve_dataset_source(source: &str) -> Result<PublicDataSource, ToolError> {
    let registry_source = retrieval_registry::find_source("dataset", source).ok_or_else(|| {
        ToolError::InvalidArguments {
            message: format!("Unsupported dataset query source: {source}"),
        }
    })?;
    ensure_registry_source_can_query(&registry_source)?;
    PublicDataSource::parse(source).ok_or_else(|| ToolError::InvalidArguments {
        message: format!("Unsupported dataset query source: {source}"),
    })
}

pub(super) fn ensure_dataset_source_enabled(
    ctx: &ToolContext,
    source: PublicDataSource,
) -> Result<(), ToolError> {
    if ctx
        .web_search_api_keys
        .is_query_dataset_source_enabled(source.as_str())
    {
        return Ok(());
    }
    Err(ToolError::InvalidArguments {
        message: format!(
            "Dataset source `{}` is disabled in Settings → Search. Enable it before querying/fetching that source.",
            source.as_str()
        ),
    })
}

pub(super) fn dataset_subcategory_id(
    subcategory: Option<&str>,
) -> Result<Option<&'static str>, ToolError> {
    let Some(subcategory) = subcategory else {
        return Ok(None);
    };
    match subcategory {
        "expression" | "gene_expression" | "transcriptomics" | "transcriptome" => {
            Ok(Some("expression"))
        }
        "sequencing" | "sequence_reads" | "raw_reads" | "reads" | "sra" => Ok(Some("sequencing")),
        "genomics" | "genome" | "genomes" | "assembly" | "assemblies" => Ok(Some("genomics")),
        "sample_metadata" | "sample" | "samples" | "metadata" => Ok(Some("sample_metadata")),
        "multi_omics" | "multiomics" | "projects" | "project" => Ok(Some("multi_omics")),
        other => Err(ToolError::InvalidArguments {
            message: format!("Unsupported dataset subcategory: {other}"),
        }),
    }
}

pub(super) fn dataset_source_for_subcategory(
    subcategory: Option<&str>,
) -> Result<Option<PublicDataSource>, ToolError> {
    let Some(subcategory) = subcategory else {
        return Ok(None);
    };
    match subcategory {
        "expression" | "gene_expression" | "transcriptomics" | "transcriptome" => {
            Ok(Some(PublicDataSource::Geo))
        }
        "sequencing" | "sequence_reads" | "raw_reads" | "reads" | "sra" => {
            Ok(Some(PublicDataSource::EnaRun))
        }
        "genomics" | "genome" | "genomes" | "assembly" | "assemblies" => {
            Ok(Some(PublicDataSource::EnaAssembly))
        }
        "sample_metadata" | "sample" | "samples" | "metadata" => {
            Ok(Some(PublicDataSource::EnaSample))
        }
        "multi_omics" | "multiomics" | "projects" | "project" => {
            Ok(Some(PublicDataSource::CbioPortal))
        }
        other => Err(ToolError::InvalidArguments {
            message: format!("Unsupported dataset subcategory: {other}"),
        }),
    }
}

pub(super) fn infer_dataset_source(
    identifier: &str,
    subcategory: Option<&str>,
) -> Result<PublicDataSource, ToolError> {
    if crate::domain::search::data::looks_like_geo_accession(identifier)
        || identifier
            .to_ascii_lowercase()
            .contains("ncbi.nlm.nih.gov/geo")
        || identifier
            .to_ascii_lowercase()
            .contains("ncbi.nlm.nih.gov/gds")
    {
        return Ok(PublicDataSource::Geo);
    }
    if let Some(source) = crate::domain::search::data::inferred_ena_source_key(identifier) {
        return PublicDataSource::parse(source).ok_or_else(|| ToolError::InvalidArguments {
            message: format!("Unsupported inferred ENA source: {source}"),
        });
    }
    if identifier.to_ascii_lowercase().contains("ebi.ac.uk/ena") {
        return Ok(PublicDataSource::EnaStudy);
    }
    if crate::domain::search::data::looks_like_gtex_identifier(identifier)
        || identifier.to_ascii_lowercase().contains("gtexportal.org")
    {
        return Ok(PublicDataSource::Gtex);
    }
    if let Some(source) = dataset_source_for_subcategory(subcategory)? {
        return Ok(source);
    }
    Ok(PublicDataSource::Geo)
}
