use super::{ToolContext, ToolError, ToolImpl, ToolSchema};
use crate::infrastructure::streaming::{stream_single, StreamOutputItem};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub const DESCRIPTION: &str =
    "Validate installed plugin-contributed Operator, Template, and Environment manifests for authoring diagnostics.";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UnitAuthoringValidateArgs {
    #[serde(default, rename = "includeOk")]
    pub include_ok: bool,
}

pub struct UnitAuthoringValidateTool;

#[async_trait]
impl ToolImpl for UnitAuthoringValidateTool {
    type Args = UnitAuthoringValidateArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        _ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        let operator_diagnostics = crate::domain::operators::list_operator_manifest_diagnostics();
        let template_diagnostics = crate::domain::templates::list_template_manifest_diagnostics();
        let environment_diagnostics =
            crate::domain::environments::list_environment_manifest_diagnostics();
        let operator_count = crate::domain::operators::list_operator_summaries().len();
        let template_count = crate::domain::templates::list_template_summaries().len();
        let environment_count = crate::domain::environments::discover_environment_profiles().len();
        let diagnostic_count =
            operator_diagnostics.len() + template_diagnostics.len() + environment_diagnostics.len();
        let mut output = serde_json::json!({
            "ok": diagnostic_count == 0,
            "diagnosticCount": diagnostic_count,
            "counts": {
                "operators": operator_count,
                "templates": template_count,
                "environments": environment_count
            },
            "diagnostics": {
                "operators": operator_diagnostics,
                "templates": template_diagnostics,
                "environments": environment_diagnostics
            },
            "note": "V4 authoring validation is read-only. It validates manifests and contribution discovery; it does not execute units or install environments."
        });
        if args.include_ok {
            output["validated"] = serde_json::json!({
                "operators": operator_count,
                "templates": template_count,
                "environments": environment_count
            });
        }
        Ok(stream_single(StreamOutputItem::Text(
            serde_json::to_string_pretty(&output).unwrap_or_else(|_| "{}".to_string()),
        )))
    }
}

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "unit_authoring_validate",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "includeOk": {
                    "type": "boolean",
                    "description": "When true, include counts of validated manifest categories even if there are no diagnostics."
                }
            }
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::streaming::StreamOutputItem;
    use futures::StreamExt;

    #[tokio::test]
    async fn validates_bundled_authoring_manifests() {
        let ctx = ToolContext::new(std::env::temp_dir());
        let value = execute_to_json(&ctx, UnitAuthoringValidateArgs { include_ok: true }).await;

        assert!(value["ok"].is_boolean());
        assert!(value["counts"]["operators"].as_u64().is_some());
        assert!(value["counts"]["templates"].as_u64().is_some());
        assert!(value["counts"]["environments"].as_u64().is_some());
        assert!(value["diagnostics"]["operators"].is_array());
        assert!(value["diagnostics"]["templates"].is_array());
        assert!(value["diagnostics"]["environments"].is_array());
    }

    async fn execute_to_json(
        ctx: &ToolContext,
        args: UnitAuthoringValidateArgs,
    ) -> serde_json::Value {
        let mut stream = UnitAuthoringValidateTool::execute(ctx, args)
            .await
            .expect("execute");
        while let Some(item) = stream.next().await {
            if let StreamOutputItem::Text(text) = item {
                return serde_json::from_str(&text).expect("json");
            }
        }
        panic!("unit_authoring_validate did not return text output");
    }
}
