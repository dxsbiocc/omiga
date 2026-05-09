use super::{ToolContext, ToolError, ToolImpl, ToolSchema};
use crate::infrastructure::streaming::{stream_single, StreamOutputItem};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub const DESCRIPTION: &str =
    "Resolve a plugin-contributed Environment profile by envRef and optionally run its safe diagnostics.checkCommand.";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentProfileCheckArgs {
    #[serde(rename = "envRef")]
    pub env_ref: String,
    #[serde(default, rename = "providerPlugin")]
    pub provider_plugin: Option<String>,
    #[serde(default, rename = "runCheck")]
    pub run_check: bool,
}

pub struct EnvironmentProfileCheckTool;

#[async_trait]
impl ToolImpl for EnvironmentProfileCheckTool {
    type Args = EnvironmentProfileCheckArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        _ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        let profiles = crate::domain::environments::discover_environment_profiles();
        let provider = args.provider_plugin.as_deref().unwrap_or_default();
        let resolution = crate::domain::environments::resolve_environment_ref_from_profiles(
            &args.env_ref,
            provider,
            &profiles,
        );
        let check = if args.run_check {
            resolution
                .profile
                .as_ref()
                .map(crate::domain::environments::check_environment_profile)
                .map(serde_json::to_value)
                .transpose()
                .map_err(|err| ToolError::ExecutionFailed {
                    message: format!("serialize environment check: {err}"),
                })?
        } else {
            None
        };
        let output = serde_json::json!({
            "envRef": args.env_ref,
            "providerPlugin": args.provider_plugin,
            "resolution": resolution,
            "check": check,
            "note": "V4 environment checks are diagnostic and allowlisted; they do not install packages, create environments, pull containers, or mutate runtime state."
        });
        Ok(stream_single(StreamOutputItem::Text(
            serde_json::to_string_pretty(&output).unwrap_or_else(|_| "{}".to_string()),
        )))
    }
}

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "environment_profile_check",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "envRef": {
                    "type": "string",
                    "description": "Environment ref such as r-bioc or a canonical provider/environment/id."
                },
                "providerPlugin": {
                    "type": "string",
                    "description": "Optional provider plugin id used to disambiguate short envRefs, for example operator-pca-r@omiga-curated."
                },
                "runCheck": {
                    "type": "boolean",
                    "description": "When true, run the profile diagnostics.checkCommand if its executable is in the safe V4 allowlist. Defaults to false."
                }
            },
            "required": ["envRef"]
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::streaming::StreamOutputItem;
    use futures::StreamExt;

    #[tokio::test]
    async fn reports_missing_environment_without_running_check_by_default() {
        let ctx = ToolContext::new(std::env::temp_dir());
        let value = execute_to_json(
            &ctx,
            EnvironmentProfileCheckArgs {
                env_ref: "__missing_env_v4__".to_string(),
                provider_plugin: Some("missing@local".to_string()),
                run_check: false,
            },
        )
        .await;

        assert_eq!(value["resolution"]["status"], "missing");
        assert!(value["check"].is_null());
        assert!(value["note"]
            .as_str()
            .unwrap()
            .contains("diagnostic and allowlisted"));
    }

    async fn execute_to_json(
        ctx: &ToolContext,
        args: EnvironmentProfileCheckArgs,
    ) -> serde_json::Value {
        let mut stream = EnvironmentProfileCheckTool::execute(ctx, args)
            .await
            .expect("execute");
        while let Some(item) = stream.next().await {
            if let StreamOutputItem::Text(text) = item {
                return serde_json::from_str(&text).expect("json");
            }
        }
        panic!("environment_profile_check did not return text output");
    }
}
