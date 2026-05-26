//! Playbook 固化系统的 tauri 命令层(Wave 3 productionization)。
//!
//! 薄包装:把已验证的 `domain::playbooks` 逻辑接到实时算子链执行路径。
//! - `save_playbook_from_chain`:把一条(成功的)链固化为 Playbook。
//! - `list_playbooks`:列出项目内 Playbook(供前端像 Skill 一样展示)。
//! - `replay_playbook`:按 id 重放——重算当前算子版本指纹 → 失效检测 → 执行 → 验证 →
//!   回写 health(含 auto-demote)。失效 / 未找到 / 非 Active 时不执行,交由调用方回退。

use crate::app_state::OmigaAppState;
use crate::commands::operators::{
    build_operator_context, resolve_project_root, run_chain_with_context, OperatorChainResult,
};
use crate::commands::CommandResult;
use crate::domain::operators::{self, ChainStep};
use crate::domain::playbooks::{
    build_chain_playbook, execute_replay, JsonFilePlaybookStore, Playbook, PlaybookStatus,
    PlaybookStore, PlaybookVerification, Provenance, ReplayOutcome,
};
use crate::errors::AppError;
use serde::Serialize;
use std::collections::HashSet;
use std::path::Path;
use tauri::State;

const PLAYBOOKS_SUBDIR: &str = ".omiga/playbooks";

fn playbook_store(project_root: &Path) -> JsonFilePlaybookStore {
    JsonFilePlaybookStore::new(project_root.join(PLAYBOOKS_SUBDIR))
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn status_str(status: PlaybookStatus) -> String {
    match status {
        PlaybookStatus::Active => "active",
        PlaybookStatus::Stale => "stale",
        PlaybookStatus::Quarantined => "quarantined",
    }
    .to_string()
}

/// Compose a deterministic env signature from the full execution surface
/// (environment name + SSH server + sandbox backend). Any field changing alters
/// the signature, so replay against a different target invalidates.
///
/// `environment` and `sandbox_backend` are normalized to the SAME defaults
/// `build_operator_context` applies (`local` / `docker`) so an omitted surface
/// fingerprints identically to an explicit default — otherwise a playbook saved
/// with `env=local;sandbox=docker` would falsely invalidate when replayed without
/// the optional args (which still run on the defaulted target). Save and replay
/// MUST call this identically so their fingerprints agree.
fn compose_env_signature(
    environment: Option<String>,
    ssh_server: Option<String>,
    sandbox_backend: Option<String>,
) -> Option<String> {
    let normalize = |value: Option<String>| {
        value
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };
    let environment = normalize(environment).unwrap_or_else(|| "local".to_string());
    let sandbox_backend = normalize(sandbox_backend).unwrap_or_else(|| "docker".to_string());

    let mut parts = vec![format!("env={environment}")];
    if let Some(ssh) = normalize(ssh_server) {
        parts.push(format!("ssh={ssh}"));
    }
    parts.push(format!("sandbox={sandbox_backend}"));
    Some(parts.join(";"))
}

/// 解析链中每个唯一算子别名的当前算子身份(source/id@version,去重,保持首次出现顺序)。
fn resolve_chain_versions(steps: &[ChainStep]) -> Result<Vec<(String, String)>, AppError> {
    let mut versions = Vec::new();
    let mut seen = HashSet::new();
    for step in steps {
        if !seen.insert(step.alias.clone()) {
            continue;
        }
        let resolved = operators::resolve_operator_alias(&step.alias).map_err(|err| {
            AppError::Config(format!("resolve operator '{}': {}", step.alias, err.message))
        })?;
        let identity = format!(
            "{}/{}@{}",
            resolved.spec.source.source_plugin,
            resolved.spec.metadata.id,
            resolved.spec.metadata.version,
        );
        versions.push((step.alias.clone(), identity));
    }
    Ok(versions)
}

/// 把一条链固化为 Playbook 并持久化到 `.omiga/playbooks/`。
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn save_playbook_from_chain(
    playbook_id: String,
    title: String,
    steps: Vec<ChainStep>,
    expected_output_keys: Vec<String>,
    project_root: Option<String>,
    execution_environment: Option<String>,
    ssh_server: Option<String>,
    sandbox_backend: Option<String>,
) -> CommandResult<Playbook> {
    if steps.len() < 2 {
        return Err(AppError::Config(
            "a chain playbook requires at least two steps".to_string(),
        ));
    }
    // Quality is enforced at replay time (verification + auto-demote), not at save:
    // the chain editor saves a definition; a bad chain is weeded out on replay.
    let versions = resolve_chain_versions(&steps)?;
    let root = resolve_project_root(project_root);
    // Stamp the full execution surface so a later replay against a different
    // environment / SSH server / sandbox backend invalidates instead of running
    // the stored chain on the wrong target. Must match replay_playbook exactly.
    let env_signature = compose_env_signature(execution_environment, ssh_server, sandbox_backend);
    let provenance = Provenance {
        distilled_from: Vec::new(),
        proposal_id: None,
        created_at: now_rfc3339(),
    };
    let playbook = build_chain_playbook(
        playbook_id,
        title,
        &steps,
        &versions,
        expected_output_keys,
        env_signature,
        provenance,
    )
    .map_err(AppError::Config)?;

    let mut store = playbook_store(&root);
    store.save(playbook.clone()).map_err(AppError::Config)?;
    Ok(playbook)
}

/// 列出项目内全部 Playbook(含非 Active,供管理面板使用)。
#[tauri::command]
pub async fn list_playbooks(project_root: Option<String>) -> CommandResult<Vec<Playbook>> {
    let root = resolve_project_root(project_root);
    Ok(playbook_store(&root).list())
}

/// 重放结果(返回给前端)。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayPlaybookResponse {
    /// "replayed" | "invalidated" | "notFound" | "inactive"。
    pub outcome: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verified: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<OperatorChainResult>,
}

/// 按 id 重放一条链 Playbook。
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn replay_playbook(
    state: State<'_, OmigaAppState>,
    playbook_id: String,
    project_root: Option<String>,
    session_id: Option<String>,
    execution_environment: Option<String>,
    ssh_server: Option<String>,
    sandbox_backend: Option<String>,
) -> CommandResult<ReplayPlaybookResponse> {
    // Same full-surface signature as save_playbook_from_chain (clone before the
    // fields are moved into build_operator_context).
    let env_signature = compose_env_signature(
        execution_environment.clone(),
        ssh_server.clone(),
        sandbox_backend.clone(),
    );
    let ctx = build_operator_context(
        &state,
        project_root,
        session_id,
        execution_environment,
        ssh_server,
        sandbox_backend,
        120,
    )
    .await;
    let root = ctx.project_root.clone();
    let mut store = playbook_store(&root);

    // 取存储的链 steps 以解析当前算子版本(失效检测的输入)。
    let Some(stored) = store.get(&playbook_id) else {
        return Ok(ReplayPlaybookResponse {
            outcome: "notFound".to_string(),
            verified: None,
            status: None,
            result: None,
        });
    };
    let steps: Vec<ChainStep> = serde_json::from_value(stored.params.clone())
        .map_err(|err| AppError::Config(format!("decode stored chain steps: {err}")))?;
    let current_versions = match resolve_chain_versions(&steps) {
        Ok(versions) => versions,
        Err(_) => {
            return Ok(ReplayPlaybookResponse {
                outcome: "invalidated".to_string(),
                verified: None,
                status: None,
                result: None,
            });
        }
    };
    let now = now_rfc3339();

    let outcome = execute_replay(
        &mut store,
        &playbook_id,
        &current_versions,
        env_signature,
        &now,
        |chain_steps| run_chain_with_context(ctx, chain_steps),
        |result: &OperatorChainResult, v: &PlaybookVerification| {
            if !result.ok {
                return false;
            }
            if v.expected_output_keys.is_empty() {
                return true;
            }
            let mut keys = std::collections::HashSet::new();
            for step in &result.steps {
                if let Some(obj) = step
                    .result
                    .get("outputs")
                    .and_then(|outputs| outputs.as_object())
                {
                    for key in obj.keys() {
                        keys.insert(key.clone());
                    }
                }
            }
            v.expected_output_keys.iter().all(|key| keys.contains(key))
        },
    )
    .await
    .map_err(AppError::Config)?;

    Ok(match outcome {
        ReplayOutcome::Replayed {
            result,
            verified,
            status,
        } => ReplayPlaybookResponse {
            outcome: "replayed".to_string(),
            verified: Some(verified),
            status: Some(status_str(status)),
            result: Some(result),
        },
        ReplayOutcome::Invalidated => ReplayPlaybookResponse {
            outcome: "invalidated".to_string(),
            verified: None,
            status: None,
            result: None,
        },
        ReplayOutcome::NotFound => ReplayPlaybookResponse {
            outcome: "notFound".to_string(),
            verified: None,
            status: None,
            result: None,
        },
        ReplayOutcome::Inactive => ReplayPlaybookResponse {
            outcome: "inactive".to_string(),
            verified: None,
            status: None,
            result: None,
        },
    })
}
