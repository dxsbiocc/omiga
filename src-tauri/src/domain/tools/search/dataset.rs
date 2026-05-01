use super::super::{ToolContext, ToolError};

pub(super) async fn dataset_auto_search(
    ctx: &ToolContext,
    client: &crate::domain::search::data::PublicDataClient,
    data_args: crate::domain::search::data::DataSearchArgs,
) -> Result<crate::domain::search::data::DataSearchResponse, ToolError> {
    let enabled = ctx.web_search_api_keys.enabled_query_dataset_sources();
    let mut sources = Vec::new();
    if enabled.iter().any(|source| source == "geo") {
        sources.push(crate::domain::search::data::PublicDataSource::Geo);
    }
    if enabled.iter().any(|source| source == "ena") {
        sources.push(crate::domain::search::data::PublicDataSource::EnaStudy);
    }
    if enabled.iter().any(|source| source == "cbioportal") {
        sources.push(crate::domain::search::data::PublicDataSource::CbioPortal);
    }
    if enabled.iter().any(|source| source == "gtex") {
        sources.push(crate::domain::search::data::PublicDataSource::Gtex);
    }
    if enabled.iter().any(|source| source == "ncbi_datasets") {
        sources.push(crate::domain::search::data::PublicDataSource::NcbiDatasets);
    }
    if sources.is_empty() {
        return Ok(crate::domain::search::data::DataSearchResponse {
            query: data_args.query.trim().to_string(),
            source: "auto".to_string(),
            total: Some(0),
            results: Vec::new(),
            notes: vec!["All dataset sources are disabled in Settings → Search.".to_string()],
        });
    }
    let mut results = Vec::new();
    let mut total = 0u64;
    let mut saw_total = false;
    let mut notes = vec!["Combined enabled dataset-source search".to_string()];
    for source in sources {
        let response = tokio::select! {
            _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
            r = client.search(source, data_args.clone()) => r,
        };
        match response {
            Ok(response) => {
                if let Some(count) = response.total {
                    total = total.saturating_add(count);
                    saw_total = true;
                }
                notes.extend(response.notes);
                results.extend(response.results);
            }
            Err(err) => notes.push(format!("{} source failed: {err}", source.as_str())),
        }
    }
    results.truncate(data_args.normalized_max_results() as usize);
    Ok(crate::domain::search::data::DataSearchResponse {
        query: data_args.query.trim().to_string(),
        source: "auto".to_string(),
        total: saw_total.then_some(total),
        results,
        notes,
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
) -> Result<Option<crate::domain::search::data::PublicDataSource>, ToolError> {
    let Some(subcategory) = subcategory else {
        return Ok(None);
    };
    match subcategory {
        "expression" | "gene_expression" | "transcriptomics" | "transcriptome" => {
            Ok(Some(crate::domain::search::data::PublicDataSource::Geo))
        }
        "sequencing" | "sequence_reads" | "raw_reads" | "reads" | "sra" => {
            Ok(Some(crate::domain::search::data::PublicDataSource::EnaRun))
        }
        "genomics" | "genome" | "genomes" | "assembly" | "assemblies" => Ok(Some(
            crate::domain::search::data::PublicDataSource::EnaAssembly,
        )),
        "sample_metadata" | "sample" | "samples" | "metadata" => Ok(Some(
            crate::domain::search::data::PublicDataSource::EnaSample,
        )),
        "multi_omics" | "multiomics" | "projects" | "project" => Ok(Some(
            crate::domain::search::data::PublicDataSource::CbioPortal,
        )),
        other => Err(ToolError::InvalidArguments {
            message: format!("Unsupported dataset subcategory: {other}"),
        }),
    }
}
