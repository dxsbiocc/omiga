//! `/goal` command bridge for persistent scientific research goals.

use super::{super::CommandResult, ModelTestResult};
use crate::app_state::OmigaAppState;
use crate::domain::persistence::NewMessageRecord;
use crate::domain::research_system::{
    ParsedResearchGoalCommand, ResearchGoal, ResearchGoalAutoRunPolicyUpdate, ResearchGoalCycle,
    ResearchGoalSettingsUpdate,
};
use crate::domain::session::SessionCodec;
use crate::errors::{ChatError, OmigaError};
use crate::llm::{LlmConfig, LlmProvider};
use serde::{Deserialize, Serialize};
use std::env;
use tauri::State;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchGoalCommandRequest {
    pub session_id: String,
    pub project_path: String,
    pub content: String,
    #[serde(default)]
    pub body: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchGoalCommandResponse {
    pub session_id: String,
    pub round_id: String,
    pub user_message_id: String,
    pub assistant_message_id: String,
    pub assistant_content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub goal: Option<ResearchGoal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cycle: Option<ResearchGoalCycle>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchGoalStatusRequest {
    pub session_id: String,
    pub project_path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchGoalStatusResponse {
    pub goal: Option<ResearchGoal>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateResearchGoalCriteriaRequest {
    pub session_id: String,
    pub project_path: String,
    pub criteria: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateResearchGoalSettingsRequest {
    pub session_id: String,
    pub project_path: String,
    #[serde(default)]
    pub criteria: Option<Vec<String>>,
    #[serde(default)]
    pub max_cycles: Option<u32>,
    #[serde(default)]
    pub second_opinion_provider_entry: Option<String>,
    #[serde(default)]
    pub auto_run_policy: Option<ResearchGoalAutoRunPolicyUpdate>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestResearchGoalCriteriaResponse {
    pub criteria: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestResearchGoalSecondOpinionProviderRequest {
    pub provider_entry: String,
}

#[tauri::command]
pub async fn get_research_goal_status(
    request: ResearchGoalStatusRequest,
) -> CommandResult<ResearchGoalStatusResponse> {
    let cwd = super::resolve_session_project_root(&request.project_path);
    let goal = crate::domain::research_system::read_research_goal(&cwd, &request.session_id)
        .map_err(|e| OmigaError::Chat(ChatError::StreamError(e.to_string())))?;

    Ok(ResearchGoalStatusResponse { goal })
}

#[tauri::command]
pub async fn update_research_goal_criteria(
    request: UpdateResearchGoalCriteriaRequest,
) -> CommandResult<ResearchGoalStatusResponse> {
    let cwd = super::resolve_session_project_root(&request.project_path);
    let goal = crate::domain::research_system::update_research_goal_settings(
        &cwd,
        &request.session_id,
        ResearchGoalSettingsUpdate {
            criteria: Some(request.criteria),
            max_cycles: None,
            second_opinion_provider_entry: None,
            auto_run_policy: None,
        },
    )
    .map_err(|e| OmigaError::Chat(ChatError::StreamError(e.to_string())))?;

    Ok(ResearchGoalStatusResponse { goal: Some(goal) })
}

#[tauri::command]
pub async fn update_research_goal_settings(
    request: UpdateResearchGoalSettingsRequest,
) -> CommandResult<ResearchGoalStatusResponse> {
    if let Some(entry_name) = request.second_opinion_provider_entry.as_deref() {
        validate_second_opinion_provider_entry(entry_name)?;
    }

    let cwd = super::resolve_session_project_root(&request.project_path);
    let goal = crate::domain::research_system::update_research_goal_settings(
        &cwd,
        &request.session_id,
        ResearchGoalSettingsUpdate {
            criteria: request.criteria,
            max_cycles: request.max_cycles,
            second_opinion_provider_entry: request.second_opinion_provider_entry,
            auto_run_policy: request.auto_run_policy,
        },
    )
    .map_err(|e| OmigaError::Chat(ChatError::StreamError(e.to_string())))?;

    Ok(ResearchGoalStatusResponse { goal: Some(goal) })
}

fn validate_second_opinion_provider_entry(entry_name: &str) -> Result<(), OmigaError> {
    let entry_name = entry_name.trim();
    if entry_name.is_empty() {
        return Ok(());
    }
    load_named_second_opinion_config(entry_name, "Goal second-opinion provider entry").map(|_| ())
}

#[tauri::command]
pub async fn suggest_research_goal_criteria(
    app_state: State<'_, OmigaAppState>,
    request: ResearchGoalStatusRequest,
) -> CommandResult<SuggestResearchGoalCriteriaResponse> {
    let cwd = super::resolve_session_project_root(&request.project_path);
    let goal = crate::domain::research_system::read_research_goal(&cwd, &request.session_id)
        .map_err(|e| OmigaError::Chat(ChatError::StreamError(e.to_string())))?
        .ok_or_else(|| {
            OmigaError::Chat(ChatError::StreamError(
                "当前会话还没有科研目标，无法生成成功标准。".to_string(),
            ))
        })?;
    let llm_config = super::get_llm_config(&app_state.chat).await?;
    let client = crate::llm::create_client(llm_config)?;
    let criteria = crate::domain::research_system::suggest_research_goal_criteria_with_llm(
        client.as_ref(),
        &goal,
    )
    .await
    .map_err(|e| OmigaError::Chat(ChatError::StreamError(e.to_string())))?;

    Ok(SuggestResearchGoalCriteriaResponse { criteria })
}

#[tauri::command]
pub async fn test_research_goal_second_opinion_provider(
    request: TestResearchGoalSecondOpinionProviderRequest,
) -> CommandResult<ModelTestResult> {
    let entry_name = request.provider_entry.trim();
    if entry_name.is_empty() {
        return Ok(ModelTestResult {
            available: false,
            provider: None,
            model: None,
            latency_ms: None,
            error: Some("请选择或输入一个 provider entry。".to_string()),
        });
    }

    let config_file = match crate::llm::config::load_config_file() {
        Ok(config_file) => config_file,
        Err(err) => {
            return Ok(ModelTestResult {
                available: false,
                provider: None,
                model: None,
                latency_ms: None,
                error: Some(format!("无法读取 provider 配置：{err}")),
            });
        }
    };

    let mut config = match config_file.named_llm_config(entry_name) {
        Ok(config) => config,
        Err(reason) => {
            return Ok(ModelTestResult {
                available: false,
                provider: None,
                model: None,
                latency_ms: None,
                error: Some(reason),
            });
        }
    };
    let provider = config.provider;
    let model = config.model.clone();
    config.max_tokens = config.max_tokens.clamp(64, 128);
    config.temperature = Some(0.0);

    let client = match crate::llm::create_client(config) {
        Ok(client) => client,
        Err(err) => {
            return Ok(ModelTestResult {
                available: false,
                provider: Some(format!("{provider:?}")),
                model: Some(model),
                latency_ms: None,
                error: Some(err.to_string()),
            });
        }
    };

    let start = std::time::Instant::now();
    match crate::domain::research_system::probe_research_goal_second_opinion_provider_with_llm(
        client.as_ref(),
    )
    .await
    {
        Ok(_) => Ok(ModelTestResult {
            available: true,
            provider: Some(format!("{provider:?}")),
            model: Some(model),
            latency_ms: Some(start.elapsed().as_millis() as u64),
            error: None,
        }),
        Err(err) => Ok(ModelTestResult {
            available: false,
            provider: Some(format!("{provider:?}")),
            model: Some(model),
            latency_ms: None,
            error: Some(err),
        }),
    }
}

#[tauri::command]
pub async fn run_research_goal_command(
    app_state: State<'_, OmigaAppState>,
    request: ResearchGoalCommandRequest,
) -> CommandResult<ResearchGoalCommandResponse> {
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
                "Session not found for /goal".to_string(),
            ))
        })?;

    let cwd = super::resolve_session_project_root(&request.project_path);
    let command = crate::domain::research_system::parse_research_goal_body(&request.body);
    let result = match command {
        ParsedResearchGoalCommand::Run { .. } => {
            let llm_config = super::get_llm_config(&app_state.chat).await?;
            let goal =
                crate::domain::research_system::read_research_goal(&cwd, &request.session_id)
                    .map_err(|e| OmigaError::Chat(ChatError::StreamError(e.to_string())))?;
            let second_opinion_config = load_second_opinion_llm_config(&llm_config, goal.as_ref())?;
            let audit_client = crate::llm::create_client(llm_config)?;
            let second_opinion_client = second_opinion_config
                .map(crate::llm::create_client)
                .transpose()?;
            crate::domain::research_system::run_research_goal_command_with_llm(
                &cwd,
                &request.session_id,
                &request.body,
                audit_client.as_ref(),
                second_opinion_client.as_deref(),
            )
            .await
        }
        _ => crate::domain::research_system::run_research_goal_command(
            &cwd,
            &request.session_id,
            &request.body,
        ),
    }
    .map_err(|e| OmigaError::Chat(ChatError::StreamError(e.to_string())))?;

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
            "Failed to save /goal user message: {}",
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
            "Failed to create /goal round: {}",
            e
        )))
    })?;

    repo.save_message(NewMessageRecord {
        id: &assistant_message_id,
        session_id: &request.session_id,
        role: "assistant",
        content: &result.assistant_content,
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
            "Failed to save /goal assistant message: {}",
            e
        )))
    })?;

    repo.complete_round(&round_id, Some(&assistant_message_id))
        .await
        .map_err(|e| {
            OmigaError::Chat(ChatError::StreamError(format!(
                "Failed to complete /goal round: {}",
                e
            )))
        })?;
    repo.touch_session(&request.session_id).await.ok();

    {
        let mut sessions = app_state.chat.sessions.write().await;
        if let Some(runtime) = sessions.get_mut(&request.session_id) {
            let mut persisted_session = SessionCodec::db_to_domain(db_session);
            persisted_session.add_user_message(&request.content);
            persisted_session.add_assistant_message(&result.assistant_content);
            runtime.session = persisted_session;
        }
    }

    super::append_orchestration_event(
        repo,
        super::ChatOrchestrationEvent {
            session_id: &request.session_id,
            round_id: Some(&round_id),
            message_id: Some(&assistant_message_id),
            mode: Some("research_goal"),
            event_type: "research_goal_command_completed",
            phase: result
                .goal
                .as_ref()
                .map(|goal| goal.status.label_for_event())
                .or(Some("cleared")),
            task_id: result.cycle.as_ref().map(|cycle| cycle.cycle_id.as_str()),
            payload: serde_json::json!({
                "cwd": cwd,
                "body": request.body,
                "goalId": result.goal.as_ref().map(|goal| goal.goal_id.clone()),
                "cycleId": result.cycle.as_ref().map(|cycle| cycle.cycle_id.clone()),
            }),
        },
    )
    .await;

    Ok(ResearchGoalCommandResponse {
        session_id: request.session_id,
        round_id,
        user_message_id,
        assistant_message_id,
        assistant_content: result.assistant_content,
        goal: result.goal,
        cycle: result.cycle,
    })
}

fn load_second_opinion_llm_config(
    primary: &LlmConfig,
    goal: Option<&ResearchGoal>,
) -> Result<Option<LlmConfig>, OmigaError> {
    if let Ok(entry_name) = env::var("OMIGA_GOAL_SECOND_OPINION_PROVIDER_ENTRY") {
        let entry_name = entry_name.trim();
        if entry_name.is_empty() {
            return Ok(None);
        }
        return load_named_second_opinion_config(
            entry_name,
            "Second-opinion provider entry from environment",
        )
        .map(Some);
    }

    if let Some(entry_name) = goal
        .and_then(|goal| goal.second_opinion_provider_entry.as_deref())
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
    {
        return load_named_second_opinion_config(entry_name, "Goal second-opinion provider entry")
            .map(Some);
    }

    if let Ok(config_file) = crate::llm::config::load_config_file() {
        if let Some(entry_name) = config_file
            .settings
            .as_ref()
            .and_then(|settings| settings.goal_second_opinion_provider_entry.as_deref())
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
        {
            return config_file
                .named_llm_config(entry_name)
                .map(Some)
                .map_err(|reason| {
                    OmigaError::Config(format!(
                        "Global second-opinion provider entry `{entry_name}` is invalid: {reason}"
                    ))
                });
        }
    }

    if env::var("OMIGA_GOAL_SECOND_OPINION_PROVIDER").is_err()
        && env::var("OMIGA_GOAL_SECOND_OPINION_MODEL").is_err()
        && env::var("OMIGA_GOAL_SECOND_OPINION_API_KEY").is_err()
        && env::var("OMIGA_GOAL_SECOND_OPINION_BASE_URL").is_err()
    {
        return Ok(None);
    }

    let provider = env::var("OMIGA_GOAL_SECOND_OPINION_PROVIDER")
        .ok()
        .map(|value| value.parse::<LlmProvider>())
        .transpose()
        .map_err(|err| OmigaError::Config(format!("Invalid second-opinion provider: {err}")))?
        .unwrap_or(primary.provider);
    let provider_matches_primary = provider == primary.provider;
    let api_key = match env::var("OMIGA_GOAL_SECOND_OPINION_API_KEY") {
        Ok(value) if value.trim().is_empty() => {
            return Err(OmigaError::Config(
                "Second-opinion LLM API key is empty".to_string(),
            ));
        }
        Ok(value) => value,
        Err(_) if provider_matches_primary => primary.api_key.clone(),
        Err(_) => {
            return Err(OmigaError::Config(
                "Second-opinion LLM API key is required when provider differs from the primary LLM"
                    .to_string(),
            ));
        }
    };
    if api_key.trim().is_empty() {
        return Err(OmigaError::Config(
            "Second-opinion LLM API key is empty".to_string(),
        ));
    }

    let mut config = LlmConfig::new(provider, api_key);
    config.model = env::var("OMIGA_GOAL_SECOND_OPINION_MODEL").unwrap_or_else(|_| {
        if provider_matches_primary {
            primary.model.clone()
        } else {
            provider.default_model()
        }
    });
    config.base_url = env::var("OMIGA_GOAL_SECOND_OPINION_BASE_URL")
        .ok()
        .or_else(|| {
            if provider_matches_primary {
                primary.base_url.clone()
            } else {
                None
            }
        });
    config.temperature = env::var("OMIGA_GOAL_SECOND_OPINION_TEMPERATURE")
        .ok()
        .and_then(|value| value.parse().ok())
        .or(primary.temperature);
    config.max_tokens = env::var("OMIGA_GOAL_SECOND_OPINION_MAX_TOKENS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(primary.max_tokens);
    config.timeout_secs = env::var("OMIGA_GOAL_SECOND_OPINION_TIMEOUT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(primary.timeout_secs);
    config.secret_key = env::var("OMIGA_GOAL_SECOND_OPINION_SECRET_KEY")
        .ok()
        .or_else(|| {
            if provider_matches_primary {
                primary.secret_key.clone()
            } else {
                None
            }
        });
    config.app_id = env::var("OMIGA_GOAL_SECOND_OPINION_APP_ID")
        .ok()
        .or_else(|| {
            if provider_matches_primary {
                primary.app_id.clone()
            } else {
                None
            }
        });
    if provider_matches_primary {
        config.thinking = primary.thinking;
        config.reasoning_effort = primary.reasoning_effort.clone();
    }
    Ok(Some(config))
}

fn load_named_second_opinion_config(
    entry_name: &str,
    label: &str,
) -> Result<LlmConfig, OmigaError> {
    let entry_name = entry_name.trim();
    let config_file = crate::llm::config::load_config_file()?;
    config_file.named_llm_config(entry_name).map_err(|reason| {
        OmigaError::Config(format!("{label} `{entry_name}` is invalid: {reason}"))
    })
}

trait ResearchGoalEventLabel {
    fn label_for_event(&self) -> &'static str;
}

impl ResearchGoalEventLabel for crate::domain::research_system::ResearchGoalStatus {
    fn label_for_event(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Paused => "paused",
            Self::BudgetLimited => "budget_limited",
            Self::Complete => "complete",
        }
    }
}
