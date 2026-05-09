use super::{ToolContext, ToolError, ToolImpl, ToolSchema};
use crate::infrastructure::streaming::{stream_single, StreamOutputItem};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub const DESCRIPTION: &str =
    "List read-only Omiga Unit Index entries across operators, templates, and skill references.";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UnitListArgs {
    /// Optional unit kind: operator, template, or skill.
    #[serde(default)]
    pub kind: Option<String>,
    /// Optional text query matched against id/name/provider/tags/stages.
    #[serde(default)]
    pub query: Option<String>,
    /// Optional maximum number of entries to return.
    #[serde(default)]
    pub limit: Option<usize>,
}

pub struct UnitListTool;

#[async_trait]
impl ToolImpl for UnitListTool {
    type Args = UnitListArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        let kind = parse_kind(args.kind.as_deref())?;
        let skills = load_skills(ctx).await;
        let units = crate::domain::unit_index::build_unit_index(&skills);
        let matches = crate::domain::unit_index::filter_units(
            &units,
            &crate::domain::unit_index::UnitFilter {
                kind,
                query: args.query.clone(),
                limit: args.limit.or(Some(100)),
                ..Default::default()
            },
        );
        let output = serde_json::json!({
            "count": matches.len(),
            "total": units.len(),
            "units": matches,
            "templateDiagnostics": crate::domain::templates::list_template_manifest_diagnostics(),
            "note": "unit_list is read-only; execution remains through existing operator__*, skill, and retrieval tools."
        });
        Ok(stream_single(StreamOutputItem::Text(
            serde_json::to_string_pretty(&output).unwrap_or_else(|_| "{}".to_string()),
        )))
    }
}

pub(crate) fn parse_kind(
    raw: Option<&str>,
) -> Result<Option<crate::domain::unit_index::UnitKind>, ToolError> {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    crate::domain::unit_index::UnitKind::parse(raw)
        .map(Some)
        .ok_or_else(|| ToolError::InvalidArguments {
            message: format!("Invalid unit kind `{raw}`; expected operator, template, or skill."),
        })
}

pub(crate) async fn load_skills(ctx: &ToolContext) -> Vec<crate::domain::skills::SkillEntry> {
    let cache = ctx.skill_cache.clone().unwrap_or_else(|| {
        Arc::new(std::sync::Mutex::new(
            crate::domain::skills::SkillCacheMap::default(),
        ))
    });
    crate::domain::skills::load_skills_cached(&ctx.project_root, &cache).await
}

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "unit_list",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "kind": {
                    "type": "string",
                    "enum": ["operator", "template", "skill"],
                    "description": "Optional Unit kind filter."
                },
                "query": {
                    "type": "string",
                    "description": "Optional text query matched against id, name, provider plugin, tags, aliases, and stage metadata."
                },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 200,
                    "description": "Maximum entries to return; defaults to 100."
                }
            }
        }),
    )
}
