use super::{ToolContext, ToolError, ToolImpl, ToolSchema};
use crate::domain::environments::EnvironmentProfileSummary;
use crate::infrastructure::streaming::{stream_single, StreamOutputItem};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::process::Command;

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
    let runtime_type = profile
        .runtime
        .kind
        .as_deref()
        .unwrap_or("system")
        .trim()
        .to_ascii_lowercase();
    if ctx.execution_environment != "local" {
        return serde_json::json!({
            "status": "notRun",
            "runtimeType": runtime_type,
            "scope": ctx.execution_environment,
            "message": "Runtime executable probing is local-only; run this check in the target base/virtual environment or ensure the remote target has the required runtime installed.",
            "installHint": runtime_install_hint(&runtime_type),
        });
    }

    match runtime_type.as_str() {
        "conda" | "mamba" | "micromamba" => probe_conda_manager(ctx),
        "docker" => probe_single_runtime(
            ctx,
            "docker",
            &["docker"],
            "Docker runtime is required but `docker` was not found in the active PATH/base environment/virtual environment.",
            "Install Docker Desktop/Engine, make the `docker` CLI available in the selected environment, start the Docker daemon, then retry. Operator execution will run `docker version` before use.",
        ),
        "singularity" => probe_single_runtime(
            ctx,
            "singularity",
            &["singularity", "apptainer"],
            "Singularity/Apptainer is required but neither `singularity` nor `apptainer` was found in the active PATH/base environment/virtual environment.",
            "Install SingularityCE or Apptainer and make `singularity` or `apptainer` available in the selected environment, then retry.",
        ),
        "system" | "local" | "host" => probe_system_command(ctx, profile),
        other => serde_json::json!({
            "status": "unsupported",
            "runtimeType": other,
            "message": format!("Environment runtime.type `{other}` is not supported by runtime availability probing."),
            "installHint": runtime_install_hint(other),
        }),
    }
}

fn probe_conda_manager(ctx: &ToolContext) -> JsonValue {
    let checked = vec![
        "OMIGA_MICROMAMBA",
        "$HOME/.omiga/bin/micromamba",
        "micromamba",
        "mamba",
        "conda",
    ];
    let script = r#"
if [ -n "${OMIGA_MICROMAMBA:-}" ] && [ -x "$OMIGA_MICROMAMBA" ]; then
  printf 'micromamba\t%s\n' "$OMIGA_MICROMAMBA"
  exit 0
fi
if [ -x "$HOME/.omiga/bin/micromamba" ]; then
  printf 'micromamba\t%s\n' "$HOME/.omiga/bin/micromamba"
  exit 0
fi
for name in micromamba mamba conda; do
  if command -v "$name" >/dev/null 2>&1; then
    printf '%s\t%s\n' "$name" "$(command -v "$name")"
    exit 0
  fi
done
exit 127
"#;
    match run_local_probe(ctx, script) {
        Ok((manager, path)) => serde_json::json!({
            "status": "available",
            "runtimeType": "conda",
            "manager": manager,
            "executablePath": path,
            "checked": checked,
            "message": "A conda-compatible manager was found in the active PATH/base environment/virtual environment.",
            "installHint": runtime_install_hint("conda"),
        }),
        Err(error) => serde_json::json!({
            "status": "missing",
            "runtimeType": "conda",
            "manager": JsonValue::Null,
            "executablePath": JsonValue::Null,
            "checked": checked,
            "error": error,
            "message": "No micromamba, mamba, or conda executable was found in the active PATH/base environment/virtual environment.",
            "installHint": runtime_install_hint("conda"),
        }),
    }
}

fn probe_single_runtime(
    ctx: &ToolContext,
    runtime_type: &str,
    candidates: &[&str],
    missing_message: &str,
    install_hint: &str,
) -> JsonValue {
    let script = candidates
        .iter()
        .map(|candidate| {
            format!(
                "if command -v {candidate} >/dev/null 2>&1; then printf '%s\\t%s\\n' {candidate_q} \"$(command -v {candidate})\"; exit 0; fi",
                candidate = shell_quote(candidate),
                candidate_q = shell_quote(candidate),
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\nexit 127\n";
    match run_local_probe(ctx, &script) {
        Ok((manager, path)) => serde_json::json!({
            "status": "available",
            "runtimeType": runtime_type,
            "manager": manager,
            "executablePath": path,
            "checked": candidates,
            "message": format!("`{manager}` was found in the active PATH/base environment/virtual environment."),
            "installHint": install_hint,
        }),
        Err(error) => serde_json::json!({
            "status": "missing",
            "runtimeType": runtime_type,
            "manager": JsonValue::Null,
            "executablePath": JsonValue::Null,
            "checked": candidates,
            "error": error,
            "message": missing_message,
            "installHint": install_hint,
        }),
    }
}

fn probe_system_command(ctx: &ToolContext, profile: &EnvironmentProfileSummary) -> JsonValue {
    let Some(command) = profile
        .runtime
        .command
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return serde_json::json!({
            "status": "notConfigured",
            "runtimeType": profile.runtime.kind.as_deref().unwrap_or("system"),
            "message": "System/local environment profile does not declare runtime.command; no executable probe was run.",
            "installHint": profile.diagnostics.install_hint,
        });
    };
    probe_single_runtime(
        ctx,
        "system",
        &[command],
        "The profile runtime.command was not found in the active PATH/base environment/virtual environment.",
        profile
            .diagnostics
            .install_hint
            .as_deref()
            .unwrap_or("Install the required command or make it available on PATH, then retry."),
    )
}

fn run_local_probe(ctx: &ToolContext, script: &str) -> Result<(String, String), String> {
    let command = crate::domain::tools::bash::prepend_venv_activation(
        &ctx.local_venv_type,
        &ctx.local_venv_name,
        script,
    );
    let output = Command::new("/bin/sh")
        .arg("-lc")
        .arg(command)
        .output()
        .map_err(|err| err.to_string())?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr)
            .chars()
            .take(1000)
            .collect::<String>();
        return Err(if stderr.trim().is_empty() {
            format!("probe exited with status {:?}", output.status.code())
        } else {
            stderr
        });
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut parts = stdout.trim().splitn(2, '\t');
    let manager = parts.next().unwrap_or_default().trim();
    let path = parts.next().unwrap_or_default().trim();
    if manager.is_empty() || path.is_empty() {
        return Err("probe did not return an executable path".to_string());
    }
    Ok((manager.to_string(), path.to_string()))
}

fn runtime_install_hint(runtime_type: &str) -> String {
    match runtime_type {
        "conda" | "mamba" | "micromamba" => "Install the official micromamba binary at $HOME/.omiga/bin/micromamba, or set OMIGA_MICROMAMBA=/absolute/path/to/micromamba; mamba or conda on PATH also work.".to_string(),
        "docker" => "Install Docker Desktop/Engine, make the docker CLI available in the selected environment, and start the Docker daemon.".to_string(),
        "singularity" => "Install SingularityCE or Apptainer and make singularity or apptainer available in the selected environment.".to_string(),
        _ => "Install the runtime required by this Environment profile or adjust runtime.type/runtime.command.".to_string(),
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
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
