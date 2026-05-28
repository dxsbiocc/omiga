use super::{ToolContext, ToolError, ToolImpl, ToolSchema};
use crate::infrastructure::streaming::{stream_single, StreamOutputItem};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub const DESCRIPTION: &str =
    "Execute an enabled Omiga Operator program operation. Use unit_search/unit_describe or operator_describe first when uncertain; subcommands are passed as `operation`, not as separate operator tools.";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OperatorExecuteArgs {
    /// Enabled operator alias or operator id. Examples: fastqc, aligner, quantifier.
    #[serde(alias = "program", alias = "id")]
    pub operator: String,
    /// Operation/subcommand/mode to run, e.g. sample, size, mem, index. Defaults to run when the operator has one operation.
    #[serde(default)]
    pub operation: Option<String>,
    #[serde(default)]
    pub inputs: serde_json::Map<String, serde_json::Value>,
    #[serde(default)]
    pub params: serde_json::Map<String, serde_json::Value>,
    #[serde(default)]
    pub resources: serde_json::Map<String, serde_json::Value>,
}

pub struct OperatorExecuteTool;

#[async_trait]
impl ToolImpl for OperatorExecuteTool {
    type Args = OperatorExecuteArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        let invocation = operator_execute_invocation_json(&args);
        let arguments =
            serde_json::to_string(&invocation).map_err(|err| ToolError::InvalidArguments {
                message: format!("serialize operator_execute arguments: {err}"),
            })?;
        let tool_name = format!(
            "{}{}",
            crate::domain::operators::OPERATOR_TOOL_PREFIX,
            args.operator.trim()
        );
        let (output, is_error) =
            crate::domain::operators::execute_operator_tool_call(ctx, &tool_name, &arguments).await;
        if is_error {
            return Err(ToolError::ExecutionFailed { message: output });
        }
        Ok(stream_single(StreamOutputItem::Text(output)))
    }
}

pub fn operator_execute_invocation_json(args: &OperatorExecuteArgs) -> serde_json::Value {
    serde_json::json!({
        "operation": args.operation,
        "inputs": args.inputs,
        "params": args.params,
        "resources": args.resources,
    })
}

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        crate::domain::operators::OPERATOR_EXECUTE_TOOL_NAME,
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "operator": {
                    "type": "string",
                    "description": "Enabled operator alias or operator id. Use unit_search/unit_describe or operator_describe to narrow candidates before execution."
                },
                "operation": {
                    "type": "string",
                    "description": "Operator operation/subcommand/mode. Required when the operator exposes more than one operation."
                },
                "inputs": {
                    "type": "object",
                    "description": "Operator input object. Shape depends on the selected operation; inspect with operator_describe or unit_describe.",
                    "additionalProperties": true
                },
                "params": {
                    "type": "object",
                    "description": "Operator parameter object for the selected operation. Do not encode subcommands as separate operator tools; set operation instead.",
                    "additionalProperties": true
                },
                "resources": {
                    "type": "object",
                    "description": "Operator resource overrides for the selected operation.",
                    "additionalProperties": true
                }
            },
            "required": ["operator"]
        }),
    )
}
