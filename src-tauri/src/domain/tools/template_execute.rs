use super::{ToolContext, ToolError, ToolImpl, ToolSchema};
use crate::infrastructure::streaming::{stream_single, StreamOutputItem};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub const DESCRIPTION: &str =
    "Execute an Omiga Template unit by id. Prefer this for high-level analysis and visualization workflows; use operator__* only when the user explicitly needs an atomic operator. Template execution can render local scripts, inherit backing Operator ask/preflight questions, and optionally fall back to a migrationTarget for parity-safe runs.";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TemplateExecuteArgs {
    /// Template canonicalId, short id, alias, or migrationTarget alias.
    pub id: String,
    #[serde(default)]
    pub inputs: serde_json::Map<String, serde_json::Value>,
    #[serde(default)]
    pub params: serde_json::Map<String, serde_json::Value>,
    #[serde(default)]
    pub resources: serde_json::Map<String, serde_json::Value>,
}

pub struct TemplateExecuteTool;

#[async_trait]
impl ToolImpl for TemplateExecuteTool {
    type Args = TemplateExecuteArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        let invocation = serde_json::json!({
            "inputs": args.inputs,
            "params": args.params,
            "resources": args.resources,
        });
        let arguments =
            serde_json::to_string(&invocation).map_err(|err| ToolError::InvalidArguments {
                message: format!("serialize template_execute arguments: {err}"),
            })?;
        let (output, is_error) =
            crate::domain::templates::execute_template_tool_call(ctx, &args.id, &arguments).await;
        if is_error {
            return Err(ToolError::ExecutionFailed { message: output });
        }
        Ok(stream_single(StreamOutputItem::Text(output)))
    }
}

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "template_execute",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "Template canonicalId, short id, alias, or migrationTarget alias. Use unit_search/unit_describe kind=template first when uncertain; prefer Template ids for analysis and visualization workflows."
                },
                "inputs": {
                    "type": "object",
                    "description": "Template input object; same shape as OperatorInvocation.inputs. For migrated templates this inherits the backing Operator input contract.",
                    "additionalProperties": true
                },
                "params": {
                    "type": "object",
                    "description": "Template parameter object; same shape as OperatorInvocation.params. If backing preflight questions exist, the chat path asks the user for missing or ask-state choices before execution.",
                    "additionalProperties": true
                },
                "resources": {
                    "type": "object",
                    "description": "Template resource overrides, when the backing execution supports them.",
                    "additionalProperties": true
                }
            },
            "required": ["id"]
        }),
    )
}
