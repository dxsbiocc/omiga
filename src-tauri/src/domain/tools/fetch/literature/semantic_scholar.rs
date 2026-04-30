use super::super::common::json_stream;
use super::super::FetchArgs;
use super::identifiers;
use crate::domain::tools::{ToolContext, ToolError};

pub(in crate::domain::tools::fetch) async fn fetch_semantic_scholar(
    ctx: &ToolContext,
    args: &FetchArgs,
) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
    let paper_id = identifiers::resolve_semantic_scholar_id(args).ok_or_else(|| {
        ToolError::InvalidArguments {
            message: "Semantic Scholar fetch requires a paper id, DOI/arXiv/PubMed external id, URL, or search result".to_string(),
        }
    })?;
    let client =
        crate::domain::search::semantic_scholar::SemanticScholarClient::from_tool_context(ctx)
            .map_err(|message| ToolError::ExecutionFailed { message })?;
    let paper = tokio::select! {
        _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
        r = client.fetch(&paper_id) => r.map_err(|message| ToolError::ExecutionFailed { message })?,
    };
    Ok(json_stream(
        crate::domain::search::semantic_scholar::detail_to_json(&paper),
    ))
}
