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
    /// When set by the message retry button, truncate transcript after this
    /// persisted user row and reuse it instead of inserting a duplicate command.
    #[serde(default, rename = "retryFromUserMessageId")]
    pub retry_from_user_message_id: Option<String>,
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

    let mut runtime_session;
    let user_message_id;
    let is_retry = request.retry_from_user_message_id.is_some();

    if let Some(anchor) = request.retry_from_user_message_id.as_ref() {
        let anchor_row = db_session.messages.iter().find(|m| m.id == *anchor);
        let Some(anchor_row) = anchor_row else {
            return Err(OmigaError::Chat(ChatError::StreamError(
                "retry_from_user_message_id not found in session".to_string(),
            )));
        };
        if anchor_row.role != "user" {
            return Err(OmigaError::Chat(ChatError::StreamError(
                "retry_from_user_message_id must reference a user message".to_string(),
            )));
        }

        repo.delete_messages_after_anchor(&request.session_id, anchor)
            .await
            .map_err(|e| {
                OmigaError::Chat(ChatError::StreamError(format!(
                    "Failed to truncate /research session for retry: {}",
                    e
                )))
            })?;
        if anchor_row.content != request.content {
            repo.update_message_content(anchor, &request.content)
                .await
                .map_err(|e| {
                    OmigaError::Chat(ChatError::StreamError(format!(
                        "Failed to update /research retry user message: {}",
                        e
                    )))
                })?;
        }
        let reloaded = repo
            .get_session(&request.session_id)
            .await
            .map_err(|e| {
                OmigaError::Chat(ChatError::StreamError(format!(
                    "Failed to reload /research session after retry: {}",
                    e
                )))
            })?
            .ok_or_else(|| {
                OmigaError::Chat(ChatError::StreamError(
                    "Session not found after /research retry truncate".to_string(),
                ))
            })?;
        user_message_id = anchor.clone();
        runtime_session = SessionCodec::db_to_domain(reloaded);
    } else {
        user_message_id = uuid::Uuid::new_v4().to_string();
        runtime_session = SessionCodec::db_to_domain(db_session);
    }

    let cwd = super::resolve_session_project_root(&request.project_path);
    let args = parse_research_body(&request.body);
    let output = crate::domain::research_system::run_research_cli(&args, &cwd)
        .map_err(|e| OmigaError::Chat(ChatError::StreamError(e.to_string())))?;
    let assistant_content = format_research_output(&args, &output);

    if !is_retry {
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
        runtime_session.add_user_message(&request.content);
    }

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
            runtime_session.add_assistant_message(&assistant_content);
            runtime.session = runtime_session;
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

    // Persist key research conclusions to long-term memory (fire-and-forget) only for
    // real research execution results. Administrative `/research` commands should not
    // create fake "insights" from help/list output.
    if args.first().is_some_and(|arg| arg == "run") {
        persist_research_to_memory(cwd.clone(), request.session_id.clone(), &args, &output);
    }

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
                .and_then(|fo| {
                    fo.get("summary")
                        .and_then(|s| s.as_str())
                        .map(str::to_owned)
                })
                .or_else(|| {
                    v.get("task_results")
                        .and_then(|tr| tr.as_object())
                        .and_then(|map| {
                            map.values().find_map(|r| {
                                r.get("summary").and_then(|s| s.as_str()).map(str::to_owned)
                            })
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
        "approve-proposal" => {
            if rest.is_empty() {
                vec![first.to_string()]
            } else {
                vec![first.to_string(), rest.to_string()]
            }
        }
        // Do not route arbitrary/natural-language `/research` bodies into the internal
        // CLI. The frontend sends natural-language tasks through the normal chat path;
        // this backend fallback stays help-only if invoked directly.
        _ => vec!["help".to_string()],
    }
}

pub(super) fn format_research_output(args: &[String], output: &str) -> String {
    let label = args.join(" ");
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return format!("已执行 `/research {label}`，但系统没有返回可展示内容。");
    }

    match serde_json::from_str::<serde_json::Value>(trimmed) {
        Ok(value) => {
            if args.first().is_some_and(|arg| arg == "run")
                || value.get("control_plane_report").is_some()
                || value.get("task_results").is_some()
            {
                format_research_run_output(args, &value)
            } else {
                format_research_structured_output(args, &value)
            }
        }
        Err(_) => format!("已执行 `/research {label}`。\n\n{trimmed}"),
    }
}

fn format_research_run_output(args: &[String], value: &serde_json::Value) -> String {
    let request = research_request_label(args, value);
    let ambiguities = string_array_at(value, "/control_plane_report/intake_assessment/ambiguities");
    let status = if ambiguities.is_empty() {
        value
            .get("status")
            .and_then(serde_json::Value::as_str)
            .map(localize_research_status)
            .unwrap_or("已处理")
    } else {
        "需要补充信息"
    };
    let summary = extract_research_summary(value).unwrap_or_else(|| {
        if ambiguities.is_empty() {
            "科研流程已运行完成，但没有生成可直接展示的总结。".to_string()
        } else {
            "当前只完成了任务理解与流程评估，尚不能作为最终科研交付物。".to_string()
        }
    });
    let issue_items = string_array_at(value, "/issues");
    let task_count = value
        .get("task_results")
        .and_then(serde_json::Value::as_object)
        .map(|map| map.len())
        .unwrap_or(0);

    let mut sections = vec![
        if ambiguities.is_empty() {
            "## 科研任务结果".to_string()
        } else {
            "## 科研任务需要补充信息".to_string()
        },
        format!("**状态**：{status}"),
        format!("**任务**：{}", request.trim()),
        "### 当前结果".to_string(),
        summary.trim().to_string(),
    ];

    if !ambiguities.is_empty() {
        sections.push("### 还需要你补充".to_string());
        sections.extend(ambiguities.iter().map(|item| format!("- {item}")));
    }

    let next_steps = build_research_next_steps(&ambiguities, &issue_items);
    if !next_steps.is_empty() {
        sections.push("### 下一步建议".to_string());
        sections.extend(
            next_steps
                .iter()
                .enumerate()
                .map(|(index, item)| format!("{}. {item}", index + 1)),
        );
    }

    if task_count > 0 || !issue_items.is_empty() {
        sections.push("### 处理记录".to_string());
        if task_count > 0 {
            sections.push(format!("- 已处理子任务：{task_count} 个"));
        }
        sections.extend(issue_items.iter().map(|item| format!("- 注意事项：{item}")));
    }

    sections.join("\n\n")
}

fn format_research_structured_output(args: &[String], value: &serde_json::Value) -> String {
    let label = args.join(" ");
    let status = value
        .get("status")
        .and_then(serde_json::Value::as_str)
        .map(localize_research_status);
    let summary = extract_research_summary(value);
    let item_count = value.as_array().map(|items| items.len());

    let mut sections = vec![format!(
        "## Research System\n\n已执行 `/research {label}`。"
    )];

    if let Some(status) = status {
        sections.push(format!("**状态**：{status}"));
    }
    if let Some(summary) = summary {
        sections.push(format!("### 结果摘要\n\n{}", summary.trim()));
    } else if let Some(count) = item_count {
        sections.push(format!("### 结果摘要\n\n返回 {count} 条结构化记录。"));
    } else {
        sections.push("### 结果摘要\n\n已返回结构化结果；内部字段已隐藏。".to_string());
    }

    sections.join("\n\n")
}

fn research_request_label(args: &[String], value: &serde_json::Value) -> String {
    value
        .pointer("/control_plane_report/intake_assessment/user_goal")
        .and_then(serde_json::Value::as_str)
        .or_else(|| args.get(1).map(String::as_str))
        .or_else(|| args.first().map(String::as_str))
        .unwrap_or("科研任务")
        .to_string()
}

fn extract_research_summary(value: &serde_json::Value) -> Option<String> {
    value
        .get("final_output")
        .and_then(summary_from_value)
        .or_else(|| {
            value
                .get("summary")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .or_else(|| {
            value
                .get("task_results")
                .and_then(serde_json::Value::as_object)
                .and_then(|results| {
                    results.values().find_map(|result| {
                        result
                            .get("output")
                            .and_then(summary_from_value)
                            .or_else(|| result.get("summary").and_then(summary_from_value))
                    })
                })
        })
}

fn summary_from_value(value: &serde_json::Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        return non_empty_string(text);
    }

    ["summary", "conclusion", "answer", "result"]
        .iter()
        .find_map(|key| value.get(*key).and_then(serde_json::Value::as_str))
        .and_then(non_empty_string)
}

fn string_array_at(value: &serde_json::Value, pointer: &str) -> Vec<String> {
    value
        .pointer(pointer)
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    item.as_str()
                        .and_then(non_empty_string)
                        .or_else(|| summary_from_value(item))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn build_research_next_steps(ambiguities: &[String], issues: &[String]) -> Vec<String> {
    let mut steps = Vec::new();
    if ambiguities.is_empty() {
        steps.push("检查当前结论是否回答了你的科研问题。".to_string());
        steps.push("如果需要更严格证据，继续补充文献、数据或限制条件后再次运行。".to_string());
    } else {
        steps.push("补充明确的研究问题、研究对象/领域、关键词或时间范围。".to_string());
        steps.push("重新运行该科研任务，生成可引用的证据表、结论边界和阅读建议。".to_string());
    }
    if !issues.is_empty() {
        steps.push("先处理上述注意事项，再把结果用于报告、论文或课题规划。".to_string());
    }
    steps
}

fn localize_research_status(status: &str) -> &'static str {
    match status {
        "completed" => "已完成",
        "running" => "运行中",
        "pending" => "等待中",
        "needs_revision" => "需要修订",
        "blocked" => "需要补充信息",
        "approval_required" => "需要确认",
        "failed" => "执行失败",
        _ => "已处理",
    }
}

fn non_empty_string(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn format_research_output_hides_internal_json_for_run_results() {
        let output = json!({
            "graph_id": "graph-test",
            "status": "completed",
            "control_plane_report": {
                "intake_assessment": {
                    "user_goal": "请围绕以下科研问题做综述：\n\n研究问题：",
                    "ambiguities": ["未明确研究问题、研究对象或关键词。"],
                    "assumptions": ["The request benefits from external evidence collection."],
                    "complexity_score": 1,
                    "execution_route": "workflow"
                }
            },
            "task_results": {},
            "final_output": {
                "summary": "当前缺少研究问题，不能生成最终文献综述。"
            },
            "issues": []
        })
        .to_string();

        let formatted = format_research_output(
            &["run".to_string(), "请围绕以下科研问题做综述：".to_string()],
            &output,
        );

        assert!(formatted.contains("科研任务需要补充信息"));
        assert!(formatted.contains("状态**：需要补充信息"));
        assert!(formatted.contains("当前缺少研究问题"));
        assert!(formatted.contains("还需要你补充"));
        assert!(!formatted.contains("```json"));
        assert!(!formatted.contains("control_plane_report"));
        assert!(!formatted.contains("graph_id"));
        assert!(!formatted.contains("assumptions"));
    }

    #[test]
    fn format_research_output_keeps_plain_text_plain() {
        let formatted = format_research_output(&["help".to_string()], "Research help");

        assert!(formatted.contains("Research help"));
        assert!(!formatted.contains("```text"));
    }

    #[test]
    fn parse_research_body_does_not_fall_back_to_mock_run() {
        assert_eq!(
            parse_research_body("请围绕 THRSP 做文献综述"),
            vec!["help".to_string()]
        );
        assert_eq!(
            parse_research_body("plan THRSP literature review"),
            vec!["help".to_string()]
        );
        assert_eq!(
            parse_research_body("run THRSP literature review"),
            vec!["help".to_string()]
        );
    }
}
