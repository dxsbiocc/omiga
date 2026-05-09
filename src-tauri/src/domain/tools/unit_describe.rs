use super::{ToolContext, ToolError, ToolImpl, ToolSchema};
use crate::infrastructure::streaming::{stream_single, StreamOutputItem};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub const DESCRIPTION: &str =
    "Describe one Omiga Unit Index entry by canonicalId, short id, or alias and return its full read-only schema/spec/reference metadata.";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnitDescribeArgs {
    /// Canonical id, short id, or alias.
    pub id: String,
}

pub struct UnitDescribeTool;

#[async_trait]
impl ToolImpl for UnitDescribeTool {
    type Args = UnitDescribeArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        let skills = super::unit_list::load_skills(ctx).await;
        let units = crate::domain::unit_index::build_unit_index(&skills);
        let matches = crate::domain::unit_index::find_unit_matches(&units, &args.id);
        let output = match matches.as_slice() {
            [] => {
                return Err(ToolError::ExecutionFailed {
                    message: format!(
                        "Unit `{}` was not found. Use unit_list or unit_search to inspect available units.",
                        args.id
                    ),
                });
            }
            [unit] => {
                let description =
                    crate::domain::unit_index::describe_unit_by_entry(unit.clone(), &skills)
                        .ok_or_else(|| ToolError::ExecutionFailed {
                            message: format!(
                        "Unit `{}` is indexed but its backing spec/reference could not be loaded.",
                        unit.canonical_id
                    ),
                        })?;
                serde_json::json!(description)
            }
            many => serde_json::json!({
                "ambiguous": true,
                "count": many.len(),
                "matches": many,
                "note": "Multiple units match this id or alias. Call unit_describe again with one canonicalId."
            }),
        };
        Ok(stream_single(StreamOutputItem::Text(
            serde_json::to_string_pretty(&output).unwrap_or_else(|_| "{}".to_string()),
        )))
    }
}

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "unit_describe",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "Canonical id, short id, or alias. Prefer canonicalId from unit_list/unit_search for unambiguous lookup."
                }
            },
            "required": ["id"]
        }),
    )
}
