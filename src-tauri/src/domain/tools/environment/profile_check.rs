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
        let runtime_availability = if let Some(profile) = resolution.profile.as_ref() {
            Some(runtime_availability_for_profile(ctx, profile).await)
        } else {
            None
        };
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
    .concurrency_safe()
}

async fn runtime_availability_for_profile(
    ctx: &ToolContext,
    profile: &EnvironmentProfileSummary,
) -> JsonValue {
    environment_availability::probe_profile_and_cache_async(ctx, profile)
        .await
        .as_json_value()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::streaming::StreamOutputItem;
    use futures::StreamExt;
    use serde_json::json;
    use std::sync::Mutex;
    use std::time::Duration;

    static PLUGIN_TEST_ENV_LOCK: Mutex<()> = Mutex::new(());

    struct ScopedEnvVar {
        key: &'static str,
        old: Option<String>,
    }

    impl ScopedEnvVar {
        fn set(key: &'static str, value: &str) -> Self {
            let old = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, old }
        }
    }

    impl Drop for ScopedEnvVar {
        fn drop(&mut self) {
            match &self.old {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    fn scaffold_local_environment_profile_plugin(home: &std::path::Path) -> (String, String) {
        let plugin_name = "n4a-local-profile-check";
        let plugin_id = format!("{plugin_name}@local");
        let env_id = "n4a-local-runtime-check";

        let plugin_root = home
            .join(".omiga")
            .join("plugins")
            .join("cache")
            .join("local")
            .join(plugin_name)
            .join("local");

        let manifest_root = plugin_root.join(".omiga-plugin");
        std::fs::create_dir_all(&manifest_root).expect("write plugin root");
        std::fs::write(
            manifest_root.join("plugin.json"),
            r#"{"name":"n4a-local-profile-check","version":"local","description":"N4a test plugin"}"#,
        )
        .expect("write plugin manifest");

        let environment_root = plugin_root.join("environments");
        std::fs::create_dir_all(&environment_root).expect("write environment root");
        std::fs::write(
            environment_root.join("environment.yaml"),
            r#"apiVersion: omiga.ai/environment/v1alpha1
kind: Environment
metadata:
  id: n4a-local-runtime-check
  version: 0.1.0
runtime:
  type: system
  command: /bin/sh
"#,
        )
        .expect("write environment profile");

        let config_path = home.join(".omiga").join("plugins").join("config.json");
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent).expect("write omiga home");
        }
        let config = serde_json::json!({
            "plugins": { plugin_id.clone(): { "enabled": true } },
            "marketplaces": []
        });
        std::fs::write(
            config_path,
            serde_json::to_string_pretty(&config).expect("serialize plugin config"),
        )
        .expect("write plugin config");

        (plugin_id, env_id.to_string())
    }

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

    #[tokio::test]
    async fn conda_runtime_availability_reports_install_hint_or_found_manager() {
        let ctx = ToolContext::new(std::env::temp_dir());
        let profile = profile_with_runtime(json!({ "type": "conda" }));

        let availability = runtime_availability_for_profile(&ctx, &profile).await;

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

    #[tokio::test]
    async fn docker_runtime_availability_reports_runtime_guidance() {
        let ctx = ToolContext::new(std::env::temp_dir());
        let profile = profile_with_runtime(json!({ "type": "docker" }));

        let availability = runtime_availability_for_profile(&ctx, &profile).await;

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

    #[tokio::test]
    async fn remote_runtime_availability_is_not_run_locally() {
        let ctx = ToolContext::new(std::env::temp_dir()).with_execution_environment("ssh");
        let profile = profile_with_runtime(json!({ "type": "singularity" }));

        let availability = runtime_availability_for_profile(&ctx, &profile).await;

        assert_eq!(availability["status"], "notRun");
        assert_eq!(availability["runtimeType"], "singularity");
        assert_eq!(availability["scope"], "ssh");
        assert!(availability["message"]
            .as_str()
            .unwrap()
            .contains("远端探测未能执行"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn execute_local_profile_on_current_thread_runtime_returns_runtime_availability() {
        let _plugin_env_lock = PLUGIN_TEST_ENV_LOCK.lock().expect("plugin env lock");
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path().join("home");
        let _home_env = ScopedEnvVar::set("HOME", home.to_string_lossy().as_ref());
        let _user_profile_env = ScopedEnvVar::set("USERPROFILE", home.to_string_lossy().as_ref());
        let (provider_plugin, env_ref) = scaffold_local_environment_profile_plugin(&home);

        let value = tokio::time::timeout(
            Duration::from_secs(10),
            execute_to_json(
                &ToolContext::new(std::env::temp_dir()),
                EnvironmentProfileCheckArgs {
                    env_ref,
                    provider_plugin: Some(provider_plugin),
                    run_check: false,
                },
            ),
        )
        .await
        .expect("environment_profile_check execution timed out");

        assert_eq!(value["resolution"]["status"], "resolved");
        assert!(value["runtimeAvailability"].is_object());
        assert_eq!(value["runtimeAvailability"]["scope"], "local");
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
