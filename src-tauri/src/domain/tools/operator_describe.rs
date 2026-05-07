use super::{ToolContext, ToolError, ToolImpl, ToolSchema};
use crate::infrastructure::streaming::{stream_single, StreamOutputItem};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub const DESCRIPTION: &str =
    "Describe an enabled Omiga operator and return its manifest-derived schema/runtime/resource summary.";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorDescribeArgs {
    /// Operator alias or tool name. Examples: fastqc, operator__fastqc.
    pub id: String,
}

pub struct OperatorDescribeTool;

#[async_trait]
impl ToolImpl for OperatorDescribeTool {
    type Args = OperatorDescribeArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        _ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        let (alias, spec) =
            crate::domain::operators::describe_operator(&args.id).map_err(|error| {
                ToolError::ExecutionFailed {
                    message: serde_json::to_string_pretty(&error)
                        .unwrap_or_else(|_| error.message.clone()),
                }
            })?;
        let tool_name = alias.as_ref().map(|alias| format!("operator__{}", alias));
        let exposed = alias.is_some();
        let output = serde_json::json!({
            "alias": alias,
            "toolName": tool_name,
            "exposed": exposed,
            "operator": {
                "id": spec.metadata.id,
                "version": spec.metadata.version,
                "name": spec.metadata.name,
                "description": spec.metadata.description,
                "sourcePlugin": spec.source.source_plugin,
                "manifestPath": spec.source.manifest_path,
            },
            "schema": crate::domain::operators::operator_parameters_schema(&spec),
            "smokeTests": spec.smoke_tests,
            "runtime": spec.runtime,
            "resources": spec.resources,
            "bindings": spec.bindings,
            "permissions": spec.permissions,
        });
        Ok(stream_single(StreamOutputItem::Text(
            serde_json::to_string_pretty(&output).unwrap_or_else(|_| "{}".to_string()),
        )))
    }
}

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "operator_describe",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "Enabled operator alias or tool name, e.g. fastqc or operator__fastqc."
                }
            },
            "required": ["id"]
        }),
    )
}
