use super::{ToolContext, ToolError, ToolImpl, ToolSchema};
use crate::infrastructure::streaming::{stream_single, StreamOutputItem};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub const DESCRIPTION: &str =
    "Search the read-only Omiga Unit Index by text, unit kind, category, tag, or stage metadata.";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UnitSearchArgs {
    /// Optional text query matched against id/name/provider/tags/stages.
    #[serde(default)]
    pub query: Option<String>,
    /// Optional unit kind: operator, template, or skill.
    #[serde(default)]
    pub kind: Option<String>,
    /// Optional category substring, for example omics/transcriptomics.
    #[serde(default)]
    pub category: Option<String>,
    /// Optional tag substring.
    #[serde(default)]
    pub tag: Option<String>,
    /// Optional stageInput/stageOutput substring.
    #[serde(default)]
    pub stage: Option<String>,
    /// Optional maximum number of entries to return.
    #[serde(default)]
    pub limit: Option<usize>,
}

pub struct UnitSearchTool;

#[async_trait]
impl ToolImpl for UnitSearchTool {
    type Args = UnitSearchArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        let kind = super::unit_list::parse_kind(args.kind.as_deref())?;
        let skills = super::unit_list::load_skills(ctx).await;
        let units = crate::domain::unit_index::build_unit_index(&skills);
        let matches = crate::domain::unit_index::filter_units(
            &units,
            &crate::domain::unit_index::UnitFilter {
                kind,
                query: args.query.clone(),
                category: args.category.clone(),
                tag: args.tag.clone(),
                stage: args.stage.clone(),
                limit: args.limit.or(Some(50)),
            },
        );
        let output = serde_json::json!({
            "query": args.query,
            "kind": args.kind,
            "category": args.category,
            "tag": args.tag,
            "stage": args.stage,
            "count": matches.len(),
            "units": matches,
            "note": "Use unit_describe with canonicalId for the full manifest/spec after narrowing candidates. Prefer template_execute for Template units and operator__* for atomic Operator units."
        });
        Ok(stream_single(StreamOutputItem::Text(
            serde_json::to_string_pretty(&output).unwrap_or_else(|_| "{}".to_string()),
        )))
    }
}

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "unit_search",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Optional free-text query matched against ids, names, descriptions, providers, tags, aliases, and stages."
                },
                "kind": {
                    "type": "string",
                    "enum": ["operator", "template", "skill"],
                    "description": "Optional Unit kind filter."
                },
                "category": {
                    "type": "string",
                    "description": "Optional category substring, e.g. omics/transcriptomics."
                },
                "tag": {
                    "type": "string",
                    "description": "Optional tag substring."
                },
                "stage": {
                    "type": "string",
                    "description": "Optional stageInput/stageOutput substring, e.g. count_matrix or diff_results."
                },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 200,
                    "description": "Maximum entries to return; defaults to 50."
                }
            }
        }),
    )
}
