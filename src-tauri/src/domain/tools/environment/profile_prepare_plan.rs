use super::{ToolContext, ToolError, ToolImpl, ToolSchema};
use crate::domain::environments::{
    check_environment_profile, discover_environment_profiles,
    resolve_environment_ref_from_profiles, EnvironmentProfileSummary, EnvironmentResolution,
};
use crate::infrastructure::streaming::{stream_single, StreamOutputItem};
use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::path::Path;
use uuid::Uuid;

pub const DESCRIPTION: &str =
    "Create an opt-in, non-installing preparation plan for a plugin Environment profile.";

const PLAN_DIR_RELATIVE: &str = ".omiga/environments/prepare-plans";
const SAFETY_NOTE: &str = "Plan-only environment preparation. This tool does not install packages, create environments, pull containers, load modules, or mutate runtime state.";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentProfilePreparePlanArgs {
    #[serde(rename = "envRef")]
    pub env_ref: String,
    #[serde(default, rename = "providerPlugin")]
    pub provider_plugin: Option<String>,
    #[serde(default, rename = "runCheck")]
    pub run_check: bool,
    #[serde(default = "default_write_plan", rename = "writePlan")]
    pub write_plan: bool,
}

impl Default for EnvironmentProfilePreparePlanArgs {
    fn default() -> Self {
        Self {
            env_ref: String::new(),
            provider_plugin: None,
            run_check: false,
            write_plan: default_write_plan(),
        }
    }
}

pub struct EnvironmentProfilePreparePlanTool;

#[async_trait]
impl ToolImpl for EnvironmentProfilePreparePlanTool {
    type Args = EnvironmentProfilePreparePlanArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        let profiles = discover_environment_profiles();
        let provider = args.provider_plugin.as_deref().unwrap_or_default();
        let resolution = resolve_environment_ref_from_profiles(&args.env_ref, provider, &profiles);
        let check = if args.run_check {
            resolution
                .profile
                .as_ref()
                .map(check_environment_profile)
                .map(serde_json::to_value)
                .transpose()
                .map_err(|err| ToolError::ExecutionFailed {
                    message: format!("serialize environment check: {err}"),
                })?
        } else {
            None
        };
        let generated_at = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let plan = build_prepare_plan(&resolution, check.as_ref());

        let mut plan_path = None;
        let mut json_path = None;
        if args.write_plan {
            let plan_dir = ctx.project_root.join(PLAN_DIR_RELATIVE);
            tokio::fs::create_dir_all(&plan_dir).await.map_err(|err| {
                ToolError::ExecutionFailed {
                    message: format!("create environment prepare plan dir: {err}"),
                }
            })?;
            let file_stem = format!(
                "environment-prepare-{}-{}",
                Utc::now().format("%Y%m%dT%H%M%SZ"),
                Uuid::new_v4().simple()
            );
            let markdown_path = plan_dir.join(format!("{file_stem}.md"));
            let snapshot_path = plan_dir.join(format!("{file_stem}.json"));
            let markdown = render_markdown_plan(&args, &generated_at, &resolution, check.as_ref());
            let snapshot = serde_json::json!({
                "status": plan["status"],
                "generatedAt": generated_at,
                "envRef": args.env_ref.clone(),
                "providerPlugin": args.provider_plugin.clone(),
                "resolution": resolution.clone(),
                "check": check.clone(),
                "plan": plan.clone(),
                "safetyNote": SAFETY_NOTE,
            });
            let snapshot_text = serde_json::to_string_pretty(&snapshot).map_err(|err| {
                ToolError::ExecutionFailed {
                    message: format!("serialize environment prepare plan snapshot: {err}"),
                }
            })?;
            tokio::fs::write(&markdown_path, markdown)
                .await
                .map_err(|err| ToolError::ExecutionFailed {
                    message: format!("write environment prepare plan: {err}"),
                })?;
            tokio::fs::write(&snapshot_path, snapshot_text)
                .await
                .map_err(|err| ToolError::ExecutionFailed {
                    message: format!("write environment prepare plan JSON snapshot: {err}"),
                })?;
            plan_path = Some(project_relative_path(&ctx.project_root, &markdown_path));
            json_path = Some(project_relative_path(&ctx.project_root, &snapshot_path));
        }

        let output = serde_json::json!({
            "status": plan["status"],
            "generatedAt": generated_at,
            "envRef": args.env_ref,
            "providerPlugin": args.provider_plugin,
            "resolution": resolution,
            "check": check,
            "plan": plan,
            "planPath": plan_path,
            "jsonPath": json_path,
            "safetyNote": SAFETY_NOTE,
        });
        Ok(stream_single(StreamOutputItem::Text(
            serde_json::to_string_pretty(&output).unwrap_or_else(|_| "{}".to_string()),
        )))
    }
}

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "environment_profile_prepare_plan",
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
                    "description": "Optional provider plugin id used to disambiguate short envRefs."
                },
                "runCheck": {
                    "type": "boolean",
                    "description": "When true, run the same allowlisted diagnostics.checkCommand as environment_profile_check before writing the plan."
                },
                "writePlan": {
                    "type": "boolean",
                    "description": "When true, write Markdown and JSON plan files under .omiga/environments/prepare-plans. Defaults to true."
                }
            },
            "required": ["envRef"]
        }),
    )
}

fn default_write_plan() -> bool {
    true
}

fn build_prepare_plan(resolution: &EnvironmentResolution, check: Option<&JsonValue>) -> JsonValue {
    let status = if resolution.status == "resolved" {
        "planned"
    } else {
        "blocked"
    };
    let mut actions = Vec::new();
    actions.push(serde_json::json!({
        "kind": "resolve",
        "status": resolution.status,
        "description": if resolution.status == "resolved" {
            "Environment profile resolved; review requirements before preparing the runtime."
        } else {
            "Environment profile did not resolve; fix envRef/providerPlugin before preparing."
        },
    }));

    match check {
        Some(value) => actions.push(serde_json::json!({
            "kind": "diagnosticsCheck",
            "status": value.get("status").and_then(JsonValue::as_str).unwrap_or("unknown"),
            "command": value.get("command").cloned().unwrap_or(JsonValue::Null),
            "description": "Allowlisted diagnostics.checkCommand was executed; use its status to decide whether manual preparation is needed.",
        })),
        None => actions.push(serde_json::json!({
            "kind": "diagnosticsCheck",
            "status": "notRun",
            "description": "Run environment_profile_check or rerun this tool with runCheck=true before executing dependency-sensitive units.",
        })),
    }

    if let Some(profile) = resolution.profile.as_ref() {
        append_profile_actions(&mut actions, profile);
    }

    serde_json::json!({
        "status": status,
        "safetyNote": SAFETY_NOTE,
        "actions": actions,
    })
}

fn append_profile_actions(actions: &mut Vec<JsonValue>, profile: &EnvironmentProfileSummary) {
    if !profile.requirements.system.is_empty() {
        actions.push(serde_json::json!({
            "kind": "systemRequirements",
            "status": "manual",
            "items": &profile.requirements.system,
            "description": "Ensure these system commands/packages are available in the selected execution environment.",
        }));
    }
    if !profile.requirements.r_packages.is_empty() {
        actions.push(serde_json::json!({
            "kind": "rPackages",
            "status": "manual",
            "items": &profile.requirements.r_packages,
            "description": "Install or make these R packages available outside Omiga, then rerun the diagnostic check or target Template.",
        }));
    }
    if let Some(hint) = profile.diagnostics.install_hint.as_ref() {
        actions.push(serde_json::json!({
            "kind": "installHint",
            "status": "manual",
            "description": hint,
        }));
    }
    for note in &profile.requirements.notes {
        actions.push(serde_json::json!({
            "kind": "note",
            "status": "manual",
            "description": note,
        }));
    }
}

fn render_markdown_plan(
    args: &EnvironmentProfilePreparePlanArgs,
    generated_at: &str,
    resolution: &EnvironmentResolution,
    check: Option<&JsonValue>,
) -> String {
    let mut out = String::new();
    out.push_str("# Environment Preparation Plan\n\n");
    out.push_str(&format!("- Generated at: `{generated_at}`\n"));
    out.push_str(&format!("- envRef: `{}`\n", inline(&args.env_ref)));
    if let Some(provider) = args.provider_plugin.as_deref() {
        out.push_str(&format!("- Provider plugin: `{}`\n", inline(provider)));
    }
    out.push_str(&format!("- Safety: {SAFETY_NOTE}\n\n"));

    out.push_str("## Resolution\n\n");
    out.push_str(&format!("- Status: `{}`\n", inline(&resolution.status)));
    if let Some(canonical_id) = resolution.canonical_id.as_deref() {
        out.push_str(&format!("- Canonical id: `{}`\n", inline(canonical_id)));
    }
    for diagnostic in &resolution.diagnostics {
        out.push_str(&format!("- Diagnostic: {}\n", inline(diagnostic)));
    }
    out.push('\n');

    if let Some(check) = check {
        out.push_str("## Diagnostic check\n\n");
        out.push_str(&format!(
            "- Status: `{}`\n",
            inline(
                check
                    .get("status")
                    .and_then(JsonValue::as_str)
                    .unwrap_or("unknown")
            )
        ));
        if let Some(command) = check.get("command").and_then(JsonValue::as_array) {
            let rendered = command
                .iter()
                .filter_map(JsonValue::as_str)
                .map(inline)
                .collect::<Vec<_>>()
                .join(" ");
            if !rendered.is_empty() {
                out.push_str(&format!("- Command: `{rendered}`\n"));
            }
        }
        out.push('\n');
    }

    out.push_str("## Manual preparation actions\n\n");
    if let Some(profile) = resolution.profile.as_ref() {
        if !profile.requirements.system.is_empty() {
            out.push_str("- Ensure system requirements are present:\n");
            for item in &profile.requirements.system {
                out.push_str(&format!("  - `{}`\n", inline(item)));
            }
        }
        if !profile.requirements.r_packages.is_empty() {
            out.push_str("- Ensure required R packages are installed/available:\n");
            for item in &profile.requirements.r_packages {
                out.push_str(&format!("  - `{}`\n", inline(item)));
            }
        }
        if let Some(hint) = profile.diagnostics.install_hint.as_deref() {
            out.push_str(&format!("- Install hint: {}\n", inline(hint)));
        }
        for note in &profile.requirements.notes {
            out.push_str(&format!("- Note: {}\n", inline(note)));
        }
    } else {
        out.push_str(
            "- Resolve the environment reference before preparing runtime dependencies.\n",
        );
    }
    out.push('\n');
    out.push_str("After manual preparation, rerun `environment_profile_check` with `runCheck=true` before dependency-sensitive Operator/Template execution.\n");
    out
}

fn inline(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn project_relative_path(project_root: &Path, path: &Path) -> String {
    path.strip_prefix(project_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;

    #[tokio::test]
    async fn writes_blocked_plan_for_missing_environment_without_installing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let value = execute_to_json(
            &ToolContext::new(tmp.path()),
            EnvironmentProfilePreparePlanArgs {
                env_ref: "__missing_env_prepare_plan__".to_string(),
                provider_plugin: Some("missing@local".to_string()),
                run_check: false,
                write_plan: true,
            },
        )
        .await;

        assert_eq!(value["status"], "blocked");
        assert!(value["safetyNote"]
            .as_str()
            .unwrap()
            .contains("does not install"));
        let plan_path = tmp.path().join(value["planPath"].as_str().unwrap());
        let json_path = tmp.path().join(value["jsonPath"].as_str().unwrap());
        assert!(plan_path.exists(), "plan path should exist");
        assert!(json_path.exists(), "json path should exist");
        let plan = std::fs::read_to_string(plan_path).expect("plan");
        assert!(plan.contains("# Environment Preparation Plan"));
        assert!(plan.contains("Resolve the environment reference"));
    }

    #[tokio::test]
    async fn plans_resolved_bundled_profile_requirements() {
        let value = execute_to_json(
            &ToolContext::new(std::env::temp_dir()),
            EnvironmentProfilePreparePlanArgs {
                env_ref: "r-base".to_string(),
                provider_plugin: Some("visualization-r@omiga-curated".to_string()),
                run_check: false,
                write_plan: false,
            },
        )
        .await;

        assert_eq!(value["status"], "planned");
        assert_eq!(value["resolution"]["status"], "resolved");
        let actions = value["plan"]["actions"].as_array().unwrap();
        assert!(actions.iter().any(|action| {
            action["kind"] == "rPackages"
                && action["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|item| item == "ggplot2")
        }));
    }

    async fn execute_to_json(
        ctx: &ToolContext,
        args: EnvironmentProfilePreparePlanArgs,
    ) -> JsonValue {
        let mut stream = EnvironmentProfilePreparePlanTool::execute(ctx, args)
            .await
            .expect("execute prepare plan");
        while let Some(item) = stream.next().await {
            if let StreamOutputItem::Text(text) = item {
                return serde_json::from_str(&text).expect("json");
            }
        }
        panic!("environment_profile_prepare_plan did not return text output");
    }
}
