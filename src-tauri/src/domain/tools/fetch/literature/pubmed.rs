use super::super::common::json_stream;
use super::super::FetchArgs;
use super::identifiers;
use crate::domain::tools::{ToolContext, ToolError};

pub(in crate::domain::tools::fetch) async fn fetch_pubmed(
    ctx: &ToolContext,
    args: &FetchArgs,
) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
    let pmid = identifiers::resolve_pubmed_pmid(args).ok_or_else(|| ToolError::InvalidArguments {
        message: "PubMed fetch expects a numeric PMID via `id`, a PubMed `url`, or a PubMed search `result`. DOI-to-PMID resolution is planned for a later version.".to_string(),
    })?;
    let client = crate::domain::search::pubmed::EntrezClient::from_tool_context(ctx)
        .map_err(|message| ToolError::ExecutionFailed { message })?;
    let detail = tokio::select! {
        _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
        r = client.fetch_by_pmid(&pmid) => r.map_err(|message| ToolError::ExecutionFailed { message })?,
    };
    Ok(json_stream(crate::domain::search::pubmed::detail_to_json(
        &detail,
    )))
}
