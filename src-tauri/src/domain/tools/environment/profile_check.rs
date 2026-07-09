use super::{ToolContext, ToolError, ToolImpl, ToolSchema};
use crate::domain::environment_availability;
use crate::domain::environments::EnvironmentProfileSummary;
use crate::infrastructure::streaming::{stream_single, StreamOutputItem};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

pub const DESCRIPTION: &str =
    "Resolve a plugin-contributed Environment profile, report required runtime availability, and optionally run its safe diagnostics.checkCommand.";

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
        ctx: &ToolContext,
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
        let runtime_availability = resolution
            .profile
            .as_ref()
            .map(|profile| runtime_availability_for_profile(ctx, profile));
        let output = serde_json::json!({
            "envRef": args.env_ref,
            "providerPlugin": args.provider_plugin,
            "resolution": resolution,
            "runtimeAvailability": runtime_availability,
            "check": check,
            "note": "Environment profile checks are diagnostic and allowlisted. Runtime availability probing checks the active PATH/base environment/virtual environment but does not install packages, create environments, pull containers, or mutate runtime state."
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

fn runtime_availability_for_profile(
    ctx: &ToolContext,
    profile: &EnvironmentProfileSummary,
) -> JsonValue {
    environment_availability::probe_profile_and_cache(ctx, profile).as_json_value()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::streaming::StreamOutputItem;
    use futures::StreamExt;
    use serde_json::json;

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

    #[test]
    fn conda_runtime_availability_reports_install_hint_or_found_manager() {
        let ctx = ToolContext::new(std::env::temp_dir());
        let profile = profile_with_runtime(json!({ "type": "conda" }));

        let availability = runtime_availability_for_profile(&ctx, &profile);

        assert_eq!(availability["runtimeType"], "conda");
        assert!(matches!(
            availability["status"].as_str(),
            Some("available" | "missing")
        ));
        assert!(availability["installHint"]
            .as_str()
            .unwrap()
            .contains("$HOME/.omiga/bin/micromamba"));
    }

    #[test]
    fn docker_runtime_availability_reports_runtime_guidance() {
        let ctx = ToolContext::new(std::env::temp_dir());
        let profile = profile_with_runtime(json!({ "type": "docker" }));

        let availability = runtime_availability_for_profile(&ctx, &profile);

        assert_eq!(availability["runtimeType"], "docker");
        assert!(matches!(
            availability["status"].as_str(),
            Some("available" | "missing")
        ));
        assert!(availability["installHint"]
            .as_str()
            .unwrap()
            .contains("Docker"));
    }

    #[test]
    fn remote_runtime_availability_is_not_run_locally() {
        let ctx = ToolContext::new(std::env::temp_dir()).with_execution_environment("ssh");
        let profile = profile_with_runtime(json!({ "type": "singularity" }));

        let availability = runtime_availability_for_profile(&ctx, &profile);

        assert_eq!(availability["status"], "notRun");
        assert_eq!(availability["runtimeType"], "singularity");
        assert!(availability["message"]
            .as_str()
            .unwrap()
            .contains("local-only"));
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

    fn profile_with_runtime(runtime: serde_json::Value) -> EnvironmentProfileSummary {
        EnvironmentProfileSummary {
            id: "test-env".to_string(),
            version: "0.1.0".to_string(),
            canonical_id: "test/environment/test-env".to_string(),
            source_plugin: "test".to_string(),
            manifest_path: "/tmp/environment.yaml".to_string(),
            name: None,
            description: None,
            tags: Vec::new(),
            runtime: serde_json::from_value(runtime).expect("runtime"),
            requirements: crate::domain::environments::EnvironmentRequirements::default(),
            diagnostics: crate::domain::environments::EnvironmentDiagnostics::default(),
        }
    }
}
