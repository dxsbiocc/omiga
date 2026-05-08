use super::super::FetchArgs;
use super::identifiers;
use crate::domain::tools::{ToolContext, ToolError};
use serde_json::Value as JsonValue;

pub(in crate::domain::tools::fetch) async fn fetch_public_literature_json(
    ctx: &ToolContext,
    args: &FetchArgs,
    source: &str,
) -> Result<JsonValue, ToolError> {
    let source = crate::domain::search::literature::PublicLiteratureSource::parse(source)
        .ok_or_else(|| ToolError::InvalidArguments {
            message: format!("Unsupported public literature source: {source}"),
        })?;
    let identifier = identifiers::resolve_literature_identifier(args, source.as_str()).ok_or_else(|| {
        ToolError::InvalidArguments {
            message: format!(
                "fetch(category=literature, source={}) requires `id`, `url`, DOI/arXiv/OpenAlex identifier, or a search `result`",
                source.as_str()
            ),
        }
    })?;
    let client = crate::domain::search::literature::PublicLiteratureClient::from_tool_context(ctx)
        .map_err(|message| ToolError::ExecutionFailed { message })?;
    let paper = tokio::select! {
        _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
        r = client.fetch(source, &identifier) => r.map_err(|message| ToolError::ExecutionFailed { message })?,
    };
    Ok(crate::domain::search::literature::paper_to_detail_json(
        &paper,
    ))
}
