//! Research command and agent listing commands extracted from `chat/mod.rs`.

use super::super::CommandResult;
use crate::app_state::OmigaAppState;
use crate::domain::persistence::NewMessageRecord;
use crate::domain::session::SessionCodec;
use crate::errors::{ChatError, OmigaError};
use serde::{Deserialize, Serialize};
use tauri::State;

/// One built-in or custom agent entry for the composer picker.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailableAgentInfo {
    pub agent_type: String,
    pub description: String,
    /// Whether this agent always runs in the background (no foreground stream).
    pub background: bool,
}

#[derive(Debug, Serialize)]
pub struct AgentRoleInfoDto {
    pub agent_type: String,
    pub when_to_use: String,
    pub source: String,
    pub model_tier: String,
    pub explicit_model: Option<String>,
    pub background: bool,
    pub user_facing: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchCommandRequest {
    pub session_id: String,
    pub project_path: String,
    pub content: String,
    #[serde(default)]
    pub body: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchCommandResponse {
    pub session_id: String,
    pub round_id: String,
    pub user_message_id: String,
    pub assistant_message_id: String,
    pub assistant_content: String,
}

#[tauri::command]
pub fn list_available_agents() -> Vec<AvailableAgentInfo> {
    let router = crate::domain::agents::get_agent_router();
    let mut out: Vec<AvailableAgentInfo> = router
        .list_user_facing_agents()
        .into_iter()
        .map(|(t, d, bg)| AvailableAgentInfo {
            agent_type: t.to_string(),
            description: d.to_string(),
            background: bg,
        })
        .collect();
    out.sort_by(|a, b| a.agent_type.cmp(&b.agent_type));
    out
}

#[tauri::command]
pub fn list_agent_roles() -> Vec<AgentRoleInfoDto> {
    crate::domain::agents::get_agent_registry()
        .list_roles()
        .into_iter()
        .map(|role| AgentRoleInfoDto {
            agent_type: role.agent_type,
            when_to_use: role.when_to_use,
            source: format!("{:?}", role.source),
            model_tier: format!("{:?}", role.model_tier),
            explicit_model: role.explicit_model,
            background: role.background,
            user_facing: role.user_facing,
        })
        .collect()
}

#[tauri::command]
pub async fn run_research_command(
    app_state: State<'_, OmigaAppState>,
    request: ResearchCommandRequest,
) -> CommandResult<ResearchCommandResponse> {
    let repo = &*app_state.repo;
    let db_session = repo
        .get_session(&request.session_id)
        .await
        .map_err(|e| {
            OmigaError::Chat(ChatError::StreamError(format!(
                "Failed to load session: {}",
                e
            )))
        })?
        .ok_or_else(|| {
            OmigaError::Chat(ChatError::StreamError(
                "Session not found for /research".to_string(),
            ))
        })?;

    let cwd = super::resolve_session_project_root(&request.project_path);
    let args = parse_research_body(&request.body);
    let output = crate::domain::research_system::run_research_cli(&args, &cwd)
        .map_err(|e| OmigaError::Chat(ChatError::StreamError(e.to_string())))?;
    let assistant_content = format_research_output(&args, &output);

    let user_message_id = uuid::Uuid::new_v4().to_string();
    repo.save_message(NewMessageRecord {
        id: &user_message_id,
        session_id: &request.session_id,
        role: "user",
        content: &request.content,
        tool_calls: None,
        tool_call_id: None,
        token_usage_json: None,
        reasoning_content: None,
        follow_up_suggestions_json: None,
        turn_summary: None,
    })
    .await
    .map_err(|e| {
        OmigaError::Chat(ChatError::StreamError(format!(
            "Failed to save /research user message: {}",
            e
        )))
    })?;

    let round_id = uuid::Uuid::new_v4().to_string();
    let assistant_message_id = uuid::Uuid::new_v4().to_string();
    repo.create_round(
        &round_id,
        &request.session_id,
        &assistant_message_id,
        Some(&user_message_id),
    )
    .await
    .map_err(|e| {
        OmigaError::Chat(ChatError::StreamError(format!(
            "Failed to create /research round: {}",
            e
        )))
    })?;

    repo.save_message(NewMessageRecord {
        id: &assistant_message_id,
        session_id: &request.session_id,
        role: "assistant",
        content: &assistant_content,
        tool_calls: None,
        tool_call_id: None,
        token_usage_json: None,
        reasoning_content: None,
        follow_up_suggestions_json: None,
        turn_summary: None,
    })
    .await
    .map_err(|e| {
        OmigaError::Chat(ChatError::StreamError(format!(
            "Failed to save /research assistant message: {}",
            e
        )))
    })?;

    repo.complete_round(&round_id, Some(&assistant_message_id))
        .await
        .map_err(|e| {
            OmigaError::Chat(ChatError::StreamError(format!(
                "Failed to complete /research round: {}",
                e
            )))
        })?;
    repo.touch_session(&request.session_id).await.ok();

    {
        let mut sessions = app_state.chat.sessions.write().await;
        if let Some(runtime) = sessions.get_mut(&request.session_id) {
            let mut persisted_session = SessionCodec::db_to_domain(db_session);
            persisted_session.add_user_message(&request.content);
            persisted_session.add_assistant_message(&assistant_content);
            runtime.session = persisted_session;
        }
    }

    super::append_orchestration_event(
        repo,
        super::ChatOrchestrationEvent {
            session_id: &request.session_id,
            round_id: Some(&round_id),
            message_id: Some(&assistant_message_id),
            mode: Some("research"),
            event_type: "research_command_completed",
            phase: Some("complete"),
            task_id: None,
            payload: serde_json::json!({
                "cwd": cwd,
                "args": args,
            }),
        },
    )
    .await;

    // Persist key research conclusions to long-term memory (fire-and-forget).
    persist_research_to_memory(cwd.clone(), request.session_id.clone(), &args, &output);

    Ok(ResearchCommandResponse {
        session_id: request.session_id,
        round_id,
        user_message_id,
        assistant_message_id,
        assistant_content,
    })
}

/// After a research run completes, extract the goal and any conclusions and write
/// them to long-term memory as `ResearchInsight` entries.
pub(super) fn persist_research_to_memory(
    project_root: std::path::PathBuf,
    session_id: String,
    args: &[String],
    output_json: &str,
) {
    let topic = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| args.first().cloned().unwrap_or_default());
    if topic.trim().is_empty() {
        return;
    }

    let summary = serde_json::from_str::<serde_json::Value>(output_json)
        .ok()
        .and_then(|v| {
            v.get("final_output")
                .and_then(|fo| fo.get("summary").and_then(|s| s.as_str()).map(str::to_owned))
                .or_else(|| {
                    v.get("task_results")
                        .and_then(|tr| tr.as_object())
                        .and_then(|map| {
                            map.values()
                                .find_map(|r| r.get("summary").and_then(|s| s.as_str()).map(str::to_owned))
                        })
                })
        })
        .unwrap_or_else(|| format!("Research completed: {}", topic));

    tokio::spawn(async move {
        let Ok(cfg) = crate::domain::memory::load_resolved_config(&project_root).await else {
            return;
        };
        let lt_root = cfg.long_term_path(&project_root);
        let entry = crate::domain::memory::long_term::LongTermMemoryEntry {
            topic: crate::domain::memory::long_term::truncate_pub(&topic, 120),
            summary: crate::domain::memory::long_term::truncate_pub(&summary, 400),
            kind: crate::domain::memory::long_term::LongTermMemoryKind::ResearchInsight,
            entities: crate::domain::pageindex::derive_query_terms(&topic)
                .into_iter()
                .take(5)
                .collect(),
            source_sessions: vec![session_id],
            confidence: 0.75,
            stability: 0.65,
            importance: 0.75,
            reuse_probability: 0.70,
            retention_class: crate::domain::memory::long_term::RetentionClass::LongTerm,
            status: crate::domain::memory::long_term::EntryStatus::Active,
            ..Default::default()
        };
        crate::domain::memory::long_term::upsert_entry_pub(&lt_root, entry).await;
    });
}

pub(super) fn parse_research_body(body: &str) -> Vec<String> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return vec!["help".to_string()];
    }

    let mut parts = trimmed.splitn(2, char::is_whitespace);
    let first = parts.next().unwrap_or_default();
    let rest = parts.next().unwrap_or_default().trim();

    match first {
        "init" | "list-agents" | "list-proposals" | "review-traces" | "help" => {
            vec![first.to_string()]
        }
        "plan" | "run" | "approve-proposal" => {
            if rest.is_empty() {
                vec![first.to_string()]
            } else {
                vec![first.to_string(), rest.to_string()]
            }
        }
        _ => vec!["run".to_string(), trimmed.to_string()],
    }
}

pub(super) fn format_research_output(args: &[String], output: &str) -> String {
    let label = args.join(" ");
    let language = if serde_json::from_str::<serde_json::Value>(output).is_ok() {
        "json"
    } else {
        "text"
    };
    format!(
        "已执行 `/research {}`\n\n```{}\n{}\n```",
        label, language, output
    )
}
