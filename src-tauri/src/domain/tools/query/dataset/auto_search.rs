use crate::domain::search::data::{
    DataSearchArgs, DataSearchResponse, PublicDataClient, PublicDataSource,
};
use crate::domain::tools::{ToolContext, ToolError};

pub(super) async fn dataset_auto_search(
    ctx: &ToolContext,
    client: &PublicDataClient,
    data_args: DataSearchArgs,
) -> Result<DataSearchResponse, ToolError> {
    let enabled = ctx.web_search_api_keys.enabled_query_dataset_sources();
    let mut sources = Vec::new();
    if enabled.iter().any(|source| source == "geo") {
        sources.push(PublicDataSource::Geo);
    }
    if enabled.iter().any(|source| source == "ena") {
        sources.push(PublicDataSource::EnaStudy);
    }
    if enabled.iter().any(|source| source == "cbioportal") {
        sources.push(PublicDataSource::CbioPortal);
    }
    if enabled.iter().any(|source| source == "gtex") {
        sources.push(PublicDataSource::Gtex);
    }
    if enabled.iter().any(|source| source == "ncbi_datasets") {
        sources.push(PublicDataSource::NcbiDatasets);
    }
    if enabled.iter().any(|source| source == "biosample") {
        sources.push(PublicDataSource::BioSample);
    }
    if sources.is_empty() {
        return Err(ToolError::InvalidArguments {
            message: "All dataset sources are disabled in Settings → Search. Enable at least one dataset source before using query(category=dataset).".to_string(),
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
    Ok(DataSearchResponse {
        query: data_args.query.trim().to_string(),
        source: "auto".to_string(),
        total: saw_total.then_some(total),
        results,
        notes,
    })
}
