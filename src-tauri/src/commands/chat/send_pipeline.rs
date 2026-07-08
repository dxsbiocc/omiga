use super::*;

pub(super) async fn send_message_impl(
    app: AppHandle,
    app_state: State<'_, OmigaAppState>,
    request: SendMessageRequest,
) -> CommandResult<MessageResponse> {
    let send_message_started_at = std::time::Instant::now();
    let intake = match parse_send_intake(&request).await? {
        SendIntakeOutcome::Continue(intake) => intake,
        SendIntakeOutcome::EarlyReturn(response) => return Ok(response),
    };
    let computer_use_mode = intake.computer_use_mode;
    let browser_use_mode = intake.browser_use_mode;
    let KeywordSkillRouting {
        routing_content: routing_content_owned,
        explicit_workflow_command: explicit_workflow_command_owned,
        keyword_skill_route,
        trace_mode,
    } = route_keyword_skill(&request);
    let routing_content = routing_content_owned.as_str();
    let explicit_workflow_command = explicit_workflow_command_owned.as_deref();

    let exec_env = normalize_execution_environment(request.execution_environment.as_ref());
    let sandbox_backend = normalize_sandbox_backend(request.sandbox_backend.as_ref());

    // Get or create session (database is single source of truth)
    let repo = &*app_state.repo;
    let SendSessionPreparation {
        session_id,
        session,
        user_message_id,
        project_path,
    } = prepare_send_session(&app_state, &request, &exec_env, &sandbox_backend).await?;

    let project_root = resolve_session_project_root(&project_path);
    let mode_state = resolve_send_mode_state(
        &app_state,
        &project_root,
        &session_id,
        &request,
        explicit_workflow_command,
        keyword_skill_route.as_ref(),
    )
    .await;
    begin_keyword_mode_turns(BeginKeywordModeTurnsInput {
        app_state: &app_state,
        repo,
        project_root: &project_root,
        session_id: &session_id,
        user_message_id: &user_message_id,
        request: &request,
        routing_content,
        explicit_workflow_command,
        exec_env: &exec_env,
        mode_state: &mode_state,
    })
    .await;
    let SendSchedulingOutcome {
        scheduler_result,
        mode_execution_lane,
    } = prepare_send_scheduling(PrepareSendSchedulingInput {
        app_state: &app_state,
        repo,
        project_root: &project_root,
        session_id: &session_id,
        user_message_id: &user_message_id,
        request: &request,
        routing_content,
        explicit_workflow_command,
        keyword_skill_route: keyword_skill_route.as_ref(),
        trace_mode: trace_mode.as_deref(),
        mode_state: &mode_state,
    })
    .await;
    let is_plan_mode = mode_state.is_plan_mode;
    let is_team_keyword_route = mode_state.is_team_keyword_route;
    let is_ralph_keyword_route = mode_state.is_ralph_keyword_route;
    let is_autopilot_keyword_route = mode_state.is_autopilot_keyword_route;
    let is_schedule_command = mode_state.is_schedule_command;
    let is_explicit_execution_workflow = mode_state.is_explicit_execution_workflow;
    let preflight_event_mode = trace_mode.as_deref().or(Some("preflight"));

    if let Some(response) = start_direct_schedule_if_requested(StartDirectScheduleInput {
        app: &app,
        is_schedule_command,
        scheduler_result: &scheduler_result,
        session_id: &session_id,
        user_message_id: &user_message_id,
        routing_content,
        project_root: &project_root,
    }) {
        return Ok(response);
    }

    update_autopilot_phase_after_scheduling(UpdateAutopilotAfterSchedulingInput {
        app_state: &app_state,
        repo,
        project_root: &project_root,
        session_id: &session_id,
        request: &request,
        exec_env: &exec_env,
        is_autopilot_keyword_route,
        scheduler_result: &scheduler_result,
    })
    .await;

    let SendLlmPreflight {
        mut llm_config,
        resolved_runtime_constraints,
    } = load_send_llm_preflight(LoadSendLlmPreflightInput {
        app_state: &app_state,
        repo,
        project_root: &project_root,
        session_id: &session_id,
        user_message_id: &user_message_id,
        active_provider_entry_name: request.active_provider_entry_name.as_deref(),
        preflight_event_mode,
    })
    .await?;
    let SendPromptPreflight {
        integrations_cfg,
        skills_exist,
        memory_ctx,
        memory_nav,
        skills_system_section,
    } = load_send_prompt_preflight(LoadSendPromptPreflightInput {
        app_state: &app_state,
        repo,
        project_root: &project_root,
        session_id: &session_id,
        user_message_id: &user_message_id,
        request: &request,
        preflight_event_mode,
    })
    .await;

    llm_config = build_send_system_prompt(BuildSendSystemPromptInput {
        llm_config,
        session: &session,
        request: &request,
        repo,
        project_root: &project_root,
        session_id: &session_id,
        user_message_id: &user_message_id,
        preflight_event_mode,
        mode_execution_lane,
        scheduler_result: &scheduler_result,
        keyword_skill_route: keyword_skill_route.as_ref(),
        is_plan_mode,
        is_team_keyword_route,
        is_ralph_keyword_route,
        is_autopilot_keyword_route,
        exec_env: &exec_env,
        sandbox_backend: &sandbox_backend,
        skills_exist,
        memory_ctx,
        memory_nav,
        skills_system_section,
    })
    .await;

    let SendRoundPreparation {
        messages,
        llm_config_for_agent,
        client,
        compact_log_for_stream,
        round_id,
        message_id,
        cancel_flag,
        round_cancel,
    } = compact_session_and_prepare_round(CompactAndRoundInput {
        app: &app,
        app_state: &app_state,
        repo,
        session,
        llm_config,
        session_id: &session_id,
        user_message_id: &user_message_id,
        request: &request,
        preflight_event_mode,
        trace_mode: trace_mode.as_deref(),
        send_message_started_at,
        computer_use_mode,
        browser_use_mode,
        scheduler_result: &scheduler_result,
    })
    .await?;

    let tools: Vec<ToolSchema> = prepare_send_tools(PrepareSendToolsInput {
        app_state: &app_state,
        repo,
        project_root: &project_root,
        session_id: &session_id,
        user_message_id: &user_message_id,
        request: &request,
        integrations_cfg: &integrations_cfg,
        trace_mode: trace_mode.as_deref(),
        computer_use_mode,
        browser_use_mode,
    })
    .await;

    // Convert messages to LLM format
    let mut llm_messages: Vec<LlmMessage> = messages
        .iter()
        .map(|msg| LlmMessage {
            role: match msg.role {
                Role::User => LlmRole::User,
                Role::Assistant => LlmRole::Assistant,
            },
            content: msg
                .content
                .iter()
                .map(|block| match block {
                    ContentBlock::Text { text } => LlmContent::Text { text: text.clone() },
                    ContentBlock::ToolUse { id, name, input } => {
                        let (name, arguments) = normalize_llm_tool_history_for_model(name, input);
                        LlmContent::ToolUse {
                            id: id.clone(),
                            name,
                            arguments,
                        }
                    }
                    ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        is_error,
                    } => LlmContent::ToolResult {
                        tool_use_id: tool_use_id.clone(),
                        content: content.clone(),
                        is_error: *is_error,
                    },
                })
                .collect(),
            name: None,
            tool_calls: None,
            reasoning_content: msg.reasoning_content.clone(),
        })
        .collect();
    let request_image_attachments = load_request_image_attachments(&project_root, &[]).await;
    append_image_attachments_to_latest_user_message(&mut llm_messages, &request_image_attachments);

    // Start streaming in background
    let app_clone = app.clone();
    let message_id_clone = message_id.clone();
    let round_id_clone = round_id.clone();
    let session_id_clone = session_id.clone();
    let pending_tools_clone = app_state.chat.pending_tools.clone();
    let ask_user_waiters_clone = app_state.chat.ask_user_waiters.clone();
    let active_rounds_clone = app_state.chat.active_rounds.clone();
    let active_orchestrations_clone = app_state.chat.active_orchestrations.clone();
    let sessions_clone = app_state.chat.sessions.clone();
    let repo_clone = app_state.repo.clone();
    let context_llm_config = llm_config_for_agent;
    let skill_task_context = request.content.clone();
    let request_text_for_constraints = request.content.clone();
    let project_root_for_constraints = project_root.clone();
    let web_search_api_keys = app_state.chat.web_search_api_keys.lock().await.clone();
    let context_skill_cache = app_state.skill_cache.clone();
    // 回合开始前预判：短确认类输入可跳过回合结束后的 Output Formatter，加快到 Complete。
    let preflight_skip_turn_summary =
        crate::domain::agents::output_formatter::preflight_skip_turn_summary(&request.content);

    let project_root_for_ralph = project_root.clone();
    let project_root_for_autopilot = project_root.clone();
    let project_root_for_team = project_root.clone();
    let turn_spawn_context = build_turn_spawn_context(BuildTurnSpawnContextInput {
        llm_config: context_llm_config,
        skill_cache: context_skill_cache,
        scheduler_result: &scheduler_result,
        is_plan_mode,
        is_explicit_execution_workflow,
        project_root: &project_root,
        is_team_mode: is_team_keyword_route,
        is_ralph_mode: is_ralph_keyword_route,
        is_autopilot_mode: is_autopilot_keyword_route,
        exec_env: exec_env.as_str(),
        ssh_server: request.ssh_server.as_deref(),
        local_venv_type: request.local_venv_type.as_deref().unwrap_or(""),
        local_venv_name: request.local_venv_name.as_deref().unwrap_or(""),
    });

    let round_cancel_spawn = round_cancel.clone();
    let turn_spawn_task = TurnSpawnTask {
        app_clone,
        message_id_clone,
        round_id_clone,
        session_id_clone,
        pending_tools_clone,
        ask_user_waiters_clone,
        active_rounds_clone,
        active_orchestrations_clone,
        sessions_clone,
        repo_clone,
        client,
        compact_log_for_stream,
        project_root,
        cancel_flag,
        round_cancel_spawn,
        resolved_runtime_constraints,
        llm_messages,
        request_text_for_constraints,
        project_root_for_constraints,
        tools,
        request_image_attachments,
        turn_spawn_context,
        project_root_for_ralph,
        project_root_for_autopilot,
        project_root_for_team,
        skill_task_context,
        web_search_api_keys,
        computer_use_mode,
        browser_use_mode,
        preflight_skip_turn_summary,
        keyword_skill_route,
    };
    tokio::spawn(run_turn_spawn(turn_spawn_task));

    // 如果是 Plan mode，生成初始 todo items
    let initial_todos = if is_plan_mode {
        scheduler_result.as_ref().map(|result| {
            result
                .plan
                .subtasks
                .iter()
                .enumerate()
                .map(|(idx, subtask)| InitialTodoItem {
                    id: format!("plan-todo-{}", idx),
                    content: subtask.description.clone(),
                    status: if idx == 0 {
                        "in_progress".to_string()
                    } else {
                        "pending".to_string()
                    },
                })
                .collect()
        })
    } else {
        None
    };

    Ok(MessageResponse {
        message_id,
        session_id,
        round_id,
        user_message_id: Some(user_message_id),
        input_kind: None,
        scheduler_plan: scheduler_result,
        initial_todos,
    })
}

struct TurnSpawnTask {
    app_clone: AppHandle,
    message_id_clone: String,
    round_id_clone: String,
    session_id_clone: String,
    pending_tools_clone: Arc<Mutex<HashMap<String, PendingToolCall>>>,
    ask_user_waiters_clone: Arc<Mutex<HashMap<String, AskUserWaiter>>>,
    active_rounds_clone: Arc<Mutex<HashMap<String, RoundCancellationState>>>,
    active_orchestrations_clone:
        Arc<Mutex<HashMap<String, HashMap<String, tokio_util::sync::CancellationToken>>>>,
    sessions_clone: Arc<RwLock<HashMap<String, SessionRuntimeState>>>,
    repo_clone: Arc<crate::domain::persistence::SessionRepository>,
    client: Box<dyn LlmClient>,
    compact_log_for_stream: Option<String>,
    project_root: PathBuf,
    cancel_flag: Arc<RwLock<bool>>,
    round_cancel_spawn: tokio_util::sync::CancellationToken,
    resolved_runtime_constraints:
        crate::domain::runtime_constraints::ResolvedRuntimeConstraintConfig,
    llm_messages: Vec<LlmMessage>,
    request_text_for_constraints: String,
    project_root_for_constraints: PathBuf,
    tools: Vec<ToolSchema>,
    request_image_attachments: Vec<RequestImageAttachment>,
    turn_spawn_context: TurnSpawnContext,
    project_root_for_ralph: PathBuf,
    project_root_for_autopilot: PathBuf,
    project_root_for_team: PathBuf,
    skill_task_context: String,
    web_search_api_keys: crate::domain::tools::WebSearchApiKeys,
    computer_use_mode: crate::domain::computer_use::ComputerUseMode,
    browser_use_mode: crate::domain::browser_operator::BrowserUseMode,
    preflight_skip_turn_summary: bool,
    keyword_skill_route: Option<crate::domain::routing::SkillRoute>,
}

struct SendSessionPreparation {
    session_id: String,
    session: Session,
    user_message_id: String,
    project_path: String,
}

async fn prepare_send_session(
    app_state: &OmigaAppState,
    request: &SendMessageRequest,
    exec_env: &String,
    sandbox_backend: &String,
) -> CommandResult<SendSessionPreparation> {
    let repo = &*app_state.repo;

    if let Some(ref id) = request.session_id {
        // Load existing session from database
        let db_session = repo.get_session(id).await.map_err(|e| {
            OmigaError::Chat(ChatError::StreamError(format!(
                "Failed to load session: {}",
                e
            )))
        })?;

        if let Some(db_session) = db_session {
            let mut session;
            let msg_id: String;

            if let Some(ref anchor) = request.retry_from_user_message_id {
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
                repo.delete_messages_after_anchor(id, anchor)
                    .await
                    .map_err(|e| {
                        OmigaError::Chat(ChatError::StreamError(format!(
                            "Failed to truncate session for retry: {}",
                            e
                        )))
                    })?;
                if anchor_row.content != request.content {
                    repo.update_message_content(anchor, &request.content)
                        .await
                        .map_err(|e| {
                            OmigaError::Chat(ChatError::StreamError(format!(
                                "Failed to update user message for retry: {}",
                                e
                            )))
                        })?;
                }
                let db_session = repo
                    .get_session(id)
                    .await
                    .map_err(|e| {
                        OmigaError::Chat(ChatError::StreamError(format!(
                            "Failed to reload session after retry: {}",
                            e
                        )))
                    })?
                    .ok_or_else(|| {
                        OmigaError::Chat(ChatError::StreamError(
                            "Session not found after retry truncate".to_string(),
                        ))
                    })?;
                session = SessionCodec::db_to_domain(db_session);
                msg_id = anchor.clone();
            } else {
                session = SessionCodec::db_to_domain(db_session);
                session.add_user_message(&request.content);

                msg_id = uuid::Uuid::new_v4().to_string();
                repo.save_message(NewMessageRecord {
                    id: &msg_id,
                    session_id: &session.id,
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
                        "Failed to save message: {}",
                        e
                    )))
                })?;
            }

            // Update session timestamp
            repo.touch_session(&session.id).await.ok();

            // Cache in memory — keep todo/task Arcs if already present; else load from SQLite
            {
                let mut sessions = app_state.chat.sessions.write().await;
                let ssh_server = request.ssh_server.clone();
                if let Some(runtime) = sessions.get_mut(&session.id) {
                    runtime.session = session.clone();
                    runtime.active_round_ids.clear();
                    runtime.execution_environment = exec_env.clone();
                    runtime.ssh_server = ssh_server.clone();
                    runtime.sandbox_backend = sandbox_backend.clone();
                    runtime.local_venv_type = request.local_venv_type.clone().unwrap_or_default();
                    runtime.local_venv_name = request.local_venv_name.clone().unwrap_or_default();
                } else {
                    let (todos_v, tasks_v) = repo
                        .get_session_tool_state(&session.id)
                        .await
                        .map_err(|e| {
                            OmigaError::Chat(ChatError::StreamError(format!(
                                "Failed to load session tool state: {}",
                                e
                            )))
                        })?;
                    // Consume the pre-warmed EnvStore from load_session if available,
                    // so the SSH connection is already established on first tool use.
                    let env_store = app_state
                        .chat
                        .pending_env_stores
                        .lock()
                        .await
                        .remove(&session.id)
                        .unwrap_or_else(crate::domain::tools::env_store::EnvStore::new);
                    sessions.insert(
                        session.id.clone(),
                        SessionRuntimeState {
                            session: session.clone(),
                            active_round_ids: vec![],
                            todos: Arc::new(tokio::sync::Mutex::new(todos_v)),
                            agent_tasks: Arc::new(tokio::sync::Mutex::new(tasks_v)),
                            plan_mode: Arc::new(Mutex::new(false)),
                            execution_environment: exec_env.clone(),
                            ssh_server: ssh_server.clone(),
                            sandbox_backend: sandbox_backend.clone(),
                            local_venv_type: request.local_venv_type.clone().unwrap_or_default(),
                            local_venv_name: request.local_venv_name.clone().unwrap_or_default(),
                            env_store,
                            artifact_registry:
                                crate::domain::session::artifacts::ArtifactRegistry::default(),
                        },
                    );
                }
            }

            let session_id_cloned = session.id.clone();
            let project_path_cloned = session.project_path.clone();
            Ok(SendSessionPreparation {
                session_id: session_id_cloned,
                session,
                user_message_id: msg_id,
                project_path: project_path_cloned,
            })
        } else {
            Err(OmigaError::Chat(ChatError::StreamError(
                "Session not found".to_string(),
            )))
        }
    } else {
        // Create new session with explicit metadata
        let project_path = request
            .project_path
            .clone()
            .unwrap_or_else(|| ".".to_string());
        let session_name = request.session_name.clone().unwrap_or_else(|| {
            crate::domain::chat_session_title::fallback_title_from_message(&request.content)
        });

        let mut session = Session::new(session_name, project_path);
        session.add_user_message(&request.content);

        // Save session to database
        repo.create_session(&session.id, &session.name, &session.project_path)
            .await
            .map_err(|e| {
                OmigaError::Chat(ChatError::StreamError(format!(
                    "Failed to create session: {}",
                    e
                )))
            })?;

        // Save user message
        let msg_id = uuid::Uuid::new_v4().to_string();
        repo.save_message(NewMessageRecord {
            id: &msg_id,
            session_id: &session.id,
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
                "Failed to save message: {}",
                e
            )))
        })?;

        // Cache in memory — reuse pre-warmed EnvStore from load_session if present
        let ssh_server = request.ssh_server.clone();
        let env_store_for_new = app_state
            .chat
            .pending_env_stores
            .lock()
            .await
            .remove(&session.id)
            .unwrap_or_else(crate::domain::tools::env_store::EnvStore::new);
        let runtime_state = SessionRuntimeState {
            session: session.clone(),
            active_round_ids: vec![],
            todos: Arc::new(tokio::sync::Mutex::new(vec![])),
            agent_tasks: Arc::new(tokio::sync::Mutex::new(vec![])),
            plan_mode: Arc::new(Mutex::new(false)),
            execution_environment: exec_env.clone(),
            ssh_server: ssh_server.clone(),
            sandbox_backend: sandbox_backend.clone(),
            local_venv_type: request.local_venv_type.clone().unwrap_or_default(),
            local_venv_name: request.local_venv_name.clone().unwrap_or_default(),
            env_store: env_store_for_new,
            artifact_registry: crate::domain::session::artifacts::ArtifactRegistry::default(),
        };
        {
            let mut sessions = app_state.chat.sessions.write().await;
            sessions.insert(session.id.clone(), runtime_state);
        }

        let session_id_cloned = session.id.clone();
        let project_path_cloned = session.project_path.clone();
        Ok(SendSessionPreparation {
            session_id: session_id_cloned,
            session,
            user_message_id: msg_id,
            project_path: project_path_cloned,
        })
    }
}

struct SendModeState {
    has_existing_ralph_state: bool,
    has_existing_autopilot_state: bool,
    has_existing_team_state: bool,
    is_plan_mode: bool,
    is_team_keyword_route: bool,
    is_ralph_keyword_route: bool,
    is_autopilot_keyword_route: bool,
    is_plan_command: bool,
    is_schedule_command: bool,
    is_explicit_execution_workflow: bool,
    is_default_general_route: bool,
}

async fn resolve_send_mode_state(
    app_state: &OmigaAppState,
    project_root: &Path,
    session_id: &str,
    request: &SendMessageRequest,
    explicit_workflow_command: Option<&str>,
    keyword_skill_route: Option<&crate::domain::routing::SkillRoute>,
) -> SendModeState {
    let has_existing_ralph_state = crate::domain::ralph_state::read_state(project_root, session_id)
        .await
        .is_some();
    let has_existing_autopilot_state =
        crate::domain::autopilot_state::read_state(project_root, session_id)
            .await
            .is_some();
    let has_existing_team_state = crate::domain::team_state::read_state(project_root, session_id)
        .await
        .is_some();

    // Composer「权限模式」→ PermissionManager：无用户规则命中时按本会话立场硬拦截（与前端输入框同步）
    app_state
        .permission_manager
        .set_session_composer_stance(session_id, request.permission_mode.as_deref())
        .await;

    let session_plan_mode_flag = {
        let sessions = app_state.chat.sessions.read().await;
        sessions
            .get(session_id)
            .map(|runtime| runtime.plan_mode.clone())
    };
    let session_plan_mode_active = match session_plan_mode_flag {
        Some(flag) => *flag.lock().await,
        None => false,
    };

    // 检测是否为 Plan mode（Composer Plan Agent、显式 /plan 命令、或 EnterPlanMode 后的后续轮次）
    let is_plan_mode = request.composer_agent_type.as_deref() == Some("Plan")
        || matches!(explicit_workflow_command, Some("plan"))
        || session_plan_mode_active;

    // ===== 智能调度系统集成 =====
    // 检测是否使用自动调度模式（用户选择 auto 或未指定特定 Agent）
    // Team mode keyword routing also triggers the scheduler so parallel workers are spawned.
    let is_team_keyword_route = keyword_skill_route
        .map(|r| r.skill_name == "team")
        .unwrap_or(false);
    let is_ralph_keyword_route = keyword_skill_route
        .map(|r| r.skill_name == "ralph")
        .unwrap_or(false);
    let is_autopilot_keyword_route = keyword_skill_route
        .map(|r| r.skill_name == "autopilot")
        .unwrap_or(false);
    let is_plan_command = matches!(explicit_workflow_command, Some("plan"));
    let is_schedule_command = matches!(explicit_workflow_command, Some("schedule"));
    let is_explicit_execution_workflow = is_schedule_command
        || is_team_keyword_route
        || is_autopilot_keyword_route
        || is_ralph_keyword_route;
    let is_default_general_route = !is_plan_command
        && !is_explicit_execution_workflow
        && request
            .composer_agent_type
            .as_deref()
            .map(|t| t == "auto" || t == "general-purpose" || t.is_empty())
            .unwrap_or(true);

    SendModeState {
        has_existing_ralph_state,
        has_existing_autopilot_state,
        has_existing_team_state,
        is_plan_mode,
        is_team_keyword_route,
        is_ralph_keyword_route,
        is_autopilot_keyword_route,
        is_plan_command,
        is_schedule_command,
        is_explicit_execution_workflow,
        is_default_general_route,
    }
}

struct BeginKeywordModeTurnsInput<'a> {
    app_state: &'a OmigaAppState,
    repo: &'a crate::domain::persistence::SessionRepository,
    project_root: &'a Path,
    session_id: &'a str,
    user_message_id: &'a str,
    request: &'a SendMessageRequest,
    routing_content: &'a str,
    explicit_workflow_command: Option<&'a str>,
    exec_env: &'a str,
    mode_state: &'a SendModeState,
}

async fn begin_keyword_mode_turns(input: BeginKeywordModeTurnsInput<'_>) {
    let BeginKeywordModeTurnsInput {
        app_state,
        repo,
        project_root,
        session_id,
        user_message_id,
        request,
        routing_content,
        explicit_workflow_command,
        exec_env,
        mode_state,
    } = input;

    if mode_state.is_ralph_keyword_route {
        if looks_like_resume_request(routing_content) || mode_state.has_existing_ralph_state {
            append_orchestration_event(
                repo,
                ChatOrchestrationEvent {
                    session_id,
                    round_id: None,
                    message_id: Some(user_message_id),
                    mode: Some("ralph"),
                    event_type: "resume_requested",
                    phase: None,
                    task_id: None,
                    payload: serde_json::json!({ "goal": request.content }),
                },
            )
            .await;
        }
        begin_ralph_turn_if_needed(
            mode_lifecycle_context!(
                true,
                &app_state.chat.sessions,
                repo,
                project_root,
                session_id,
                ralph_runtime_env_label(
                    exec_env,
                    request.ssh_server.as_deref(),
                    request.local_venv_type.as_deref().unwrap_or(""),
                    request.local_venv_name.as_deref().unwrap_or(""),
                ),
                None,
            ),
            &request.content,
        )
        .await;
        append_orchestration_event(
            repo,
            ChatOrchestrationEvent {
                session_id,
                round_id: None,
                message_id: Some(user_message_id),
                mode: Some("ralph"),
                event_type: "mode_requested",
                phase: Some("planning"),
                task_id: None,
                payload: serde_json::json!({ "goal": request.content }),
            },
        )
        .await;
    }
    if mode_state.is_autopilot_keyword_route {
        if matches!(explicit_workflow_command, Some("autopilot"))
            || looks_like_resume_request(routing_content)
            || mode_state.has_existing_autopilot_state
        {
            append_orchestration_event(
                repo,
                ChatOrchestrationEvent {
                    session_id,
                    round_id: None,
                    message_id: Some(user_message_id),
                    mode: Some("autopilot"),
                    event_type: "resume_requested",
                    phase: None,
                    task_id: None,
                    payload: serde_json::json!({ "goal": request.content }),
                },
            )
            .await;
        }
        begin_autopilot_turn_if_needed(
            mode_lifecycle_context!(
                true,
                &app_state.chat.sessions,
                repo,
                project_root,
                session_id,
                ralph_runtime_env_label(
                    exec_env,
                    request.ssh_server.as_deref(),
                    request.local_venv_type.as_deref().unwrap_or(""),
                    request.local_venv_name.as_deref().unwrap_or(""),
                ),
                None,
            ),
            &request.content,
        )
        .await;
        append_orchestration_event(
            repo,
            ChatOrchestrationEvent {
                session_id,
                round_id: None,
                message_id: Some(user_message_id),
                mode: Some("autopilot"),
                event_type: "mode_requested",
                phase: Some("intake"),
                task_id: None,
                payload: serde_json::json!({ "goal": request.content }),
            },
        )
        .await;
    }
    if mode_state.is_team_keyword_route {
        if matches!(explicit_workflow_command, Some("team"))
            || looks_like_resume_request(routing_content)
            || mode_state.has_existing_team_state
        {
            append_orchestration_event(
                repo,
                ChatOrchestrationEvent {
                    session_id,
                    round_id: None,
                    message_id: Some(user_message_id),
                    mode: Some("team"),
                    event_type: "resume_requested",
                    phase: None,
                    task_id: None,
                    payload: serde_json::json!({ "goal": request.content }),
                },
            )
            .await;
        }
        begin_team_turn_if_needed(true, repo, project_root, session_id, &request.content, None)
            .await;
        append_orchestration_event(
            repo,
            ChatOrchestrationEvent {
                session_id,
                round_id: None,
                message_id: Some(user_message_id),
                mode: Some("team"),
                event_type: "mode_requested",
                phase: Some("planning"),
                task_id: None,
                payload: serde_json::json!({ "goal": request.content }),
            },
        )
        .await;
    }
}

struct PrepareSendSchedulingInput<'a> {
    app_state: &'a OmigaAppState,
    repo: &'a crate::domain::persistence::SessionRepository,
    project_root: &'a Path,
    session_id: &'a str,
    user_message_id: &'a str,
    request: &'a SendMessageRequest,
    routing_content: &'a str,
    explicit_workflow_command: Option<&'a str>,
    keyword_skill_route: Option<&'a crate::domain::routing::SkillRoute>,
    trace_mode: Option<&'a str>,
    mode_state: &'a SendModeState,
}

struct SendSchedulingOutcome {
    scheduler_result: Option<crate::domain::agents::scheduler::SchedulingResult>,
    mode_execution_lane: Option<crate::domain::orchestration::ExecutionLane>,
}

async fn prepare_send_scheduling(input: PrepareSendSchedulingInput<'_>) -> SendSchedulingOutcome {
    let PrepareSendSchedulingInput {
        app_state,
        repo,
        project_root,
        session_id,
        user_message_id,
        request,
        routing_content,
        explicit_workflow_command,
        keyword_skill_route,
        trace_mode,
        mode_state,
    } = input;

    let mode_strategy_override = if let Some(route) = keyword_skill_route {
        crate::domain::mode_resume::suggested_mode_strategy(
            project_root,
            session_id,
            &route.skill_name,
        )
        .await
    } else {
        None
    };
    let mode_execution_lane = if let Some(route) = keyword_skill_route {
        match route.skill_name.as_str() {
            "ralph" => {
                crate::domain::orchestration::ralph::RalphOrchestrator::current_execution_lane(
                    project_root,
                    session_id,
                )
                .await
            }
            "autopilot" => crate::domain::orchestration::autopilot::AutopilotOrchestrator::current_execution_lane(
                project_root,
                session_id,
            )
            .await,
            "team" => {
                crate::domain::orchestration::team::TeamOrchestrator::current_execution_lane(
                    project_root,
                    session_id,
                )
                .await
            }
            _ => None,
        }
    } else {
        None
    };

    // Detect strategy-specific keyword routes (phased / competitive / verification-first)
    let keyword_strategy: Option<SchedulingStrategy> = keyword_skill_route
        .and_then(|r| {
            match r.skill_name.as_str() {
                "team" => Some(SchedulingStrategy::Team),
                "plan" => Some(SchedulingStrategy::Phased),
                // No dedicated keyword rules for competitive yet — reserved for future skill routing
                _ => None,
            }
        })
        .or(if mode_state.is_schedule_command {
            Some(SchedulingStrategy::Phased)
        } else {
            None
        })
        .or(mode_strategy_override);

    let use_scheduler = mode_state.is_schedule_command
        || mode_state.is_team_keyword_route
        || (!mode_state.is_plan_mode
            && request
                .composer_agent_type
                .as_deref()
                .map(|t| t == "auto" || t == "general-purpose" || t.is_empty())
                .unwrap_or(true));

    // 如果是自动模式或 Team 关键词路由，检测任务复杂度并可能进行任务分解
    // Pre-fetch LLM config for the planner (needed before llm_config is built below).
    let planner_llm_config = crate::commands::chat::get_llm_config(&app_state.chat)
        .await
        .ok();

    let scheduler_result = if use_scheduler && request.use_tools {
        let scheduler = AgentScheduler::new();
        let scheduler_stage_started_at = std::time::Instant::now();
        // Strategy priority: keyword route > composer_agent_type > Auto
        let (strategy, force_decompose) = if let Some(s) = keyword_strategy {
            (s, true)
        } else {
            match request.composer_agent_type.as_deref() {
                Some(t) if t != "auto" && t != "general-purpose" && !t.is_empty() => {
                    let s = SchedulingStrategy::from_planner_hint(t);
                    let force = s != SchedulingStrategy::Auto;
                    (s, force)
                }
                _ => (SchedulingStrategy::Auto, false),
            }
        };

        let scheduling_req = SchedulingRequest::new(routing_content)
            .with_project_root(project_root.to_string_lossy().as_ref())
            .with_mode_hint(match keyword_skill_route {
                Some(route) => route.skill_name.clone(),
                None => explicit_workflow_command.unwrap_or_default().to_string(),
            })
            .with_strategy(strategy)
            .with_auto_decompose(force_decompose);

        match scheduler
            .schedule(scheduling_req, planner_llm_config.as_ref())
            .await
        {
            Ok(result) => {
                let classified_complex = result.selected_agents.len() > 1
                    || !matches!(
                        result.recommended_strategy,
                        SchedulingStrategy::Single | SchedulingStrategy::Auto
                    );
                append_orchestration_event(
                    repo,
                    ChatOrchestrationEvent {
                        session_id,
                        round_id: None,
                        message_id: Some(user_message_id),
                        mode: keyword_skill_route.map(|r| r.skill_name.as_str()).or(
                            if mode_state.is_schedule_command {
                                Some("schedule")
                            } else if mode_state.is_plan_command
                                || mode_state.is_default_general_route
                            {
                                Some("plan")
                            } else {
                                None
                            },
                        ),
                        event_type: "leader_intent_classified",
                        phase: if classified_complex {
                            Some("planning")
                        } else {
                            Some("solo")
                        },
                        task_id: None,
                        payload: serde_json::json!({
                            "entryAgentType": "general-purpose",
                            "classification": if classified_complex { "complex" } else { "simple" },
                            "strategy": format!("{:?}", result.recommended_strategy),
                            "taskCount": result.plan.subtasks.len(),
                            "agentCount": result.selected_agents.len(),
                            "willAutoExecute": mode_state.is_explicit_execution_workflow,
                        }),
                    },
                )
                .await;
                if trace_mode.is_some() {
                    append_preflight_stage_event(
                        repo,
                        session_id,
                        user_message_id,
                        trace_mode,
                        "scheduler_plan",
                        scheduler_stage_started_at.elapsed().as_millis(),
                        serde_json::json!({
                            "taskCount": result.plan.subtasks.len(),
                            "agentCount": result.selected_agents.len(),
                            "strategy": format!("{:?}", result.recommended_strategy),
                        }),
                    )
                    .await;
                }
                // Accept the result when:
                //  a) explicit team keyword route (user typed /team)
                //  b) planner produced > 1 agent (real multi-agent plan)
                //  c) planner recommended a non-single strategy
                let is_real_multiagent = result.selected_agents.len() > 1;
                let strategy_demands_orchestration = !matches!(
                    result.recommended_strategy,
                    SchedulingStrategy::Single | SchedulingStrategy::Auto
                );
                if mode_state.is_plan_command
                    || mode_state.is_team_keyword_route
                    || is_real_multiagent
                    || strategy_demands_orchestration
                {
                    tracing::info!(
                        target: "omiga::scheduler",
                        task_count = result.plan.subtasks.len(),
                        agents = ?result.selected_agents,
                        recommended_strategy = ?result.recommended_strategy,
                        team_mode = mode_state.is_team_keyword_route,
                        "Task decomposed into subtasks"
                    );
                    append_orchestration_event(
                        repo,
                        ChatOrchestrationEvent {
                            session_id,
                            round_id: None,
                            message_id: Some(user_message_id),
                            mode: keyword_skill_route.map(|r| r.skill_name.as_str()).or(
                                if mode_state.is_schedule_command {
                                    Some("schedule")
                                } else {
                                    None
                                },
                            ),
                            event_type: "schedule_plan_created",
                            phase: None,
                            task_id: None,
                            payload: serde_json::json!({
                                "planId": result.plan.plan_id,
                                "taskCount": result.plan.subtasks.len(),
                                "agents": result.selected_agents,
                                "strategy": format!("{:?}", result.recommended_strategy),
                            }),
                        },
                    )
                    .await;
                    if mode_state.is_plan_command || mode_state.is_default_general_route {
                        append_orchestration_event(
                            repo,
                            ChatOrchestrationEvent {
                                session_id,
                                round_id: None,
                                message_id: Some(user_message_id),
                                mode: Some("plan"),
                                event_type: "plan_ready_for_approval",
                                phase: Some("planning"),
                                task_id: None,
                                payload: serde_json::json!({
                                    "planId": result.plan.plan_id,
                                    "entryAgentType": result.plan.entry_agent_type.clone(),
                                    "executionSupervisorAgentType": result.plan.execution_supervisor_agent_type.clone(),
                                    "taskCount": result.plan.subtasks.len(),
                                    "approvalSurface": "plan_card_buttons",
                                }),
                            },
                        )
                        .await;
                    }
                    Some(result)
                } else {
                    None
                }
            }
            Err(e) => {
                tracing::warn!(target: "omiga::scheduler", "Scheduling failed: {}", e);
                if trace_mode.is_some() {
                    append_preflight_stage_failed_event(
                        repo,
                        session_id,
                        user_message_id,
                        trace_mode,
                        "scheduler_plan",
                        scheduler_stage_started_at.elapsed().as_millis(),
                        &e,
                    )
                    .await;
                }
                None
            }
        }
    } else {
        None
    };

    SendSchedulingOutcome {
        scheduler_result,
        mode_execution_lane,
    }
}

struct StartDirectScheduleInput<'a> {
    app: &'a AppHandle,
    is_schedule_command: bool,
    scheduler_result: &'a Option<crate::domain::agents::scheduler::SchedulingResult>,
    session_id: &'a str,
    user_message_id: &'a str,
    routing_content: &'a str,
    project_root: &'a Path,
}

fn start_direct_schedule_if_requested(
    input: StartDirectScheduleInput<'_>,
) -> Option<MessageResponse> {
    let StartDirectScheduleInput {
        app,
        is_schedule_command,
        scheduler_result,
        session_id,
        user_message_id,
        routing_content,
        project_root,
    } = input;
    if is_schedule_command {
        if let Some(schedule_result) = scheduler_result.clone() {
            let stream_message_id = uuid::Uuid::new_v4().to_string();
            let schedule_round_id = uuid::Uuid::new_v4().to_string();
            let session_id_for_bg = session_id.to_string();
            let project_root_for_bg = project_root.to_string_lossy().to_string();
            let request_for_bg = crate::commands::chat::RunAgentScheduleRequest {
                user_request: routing_content.to_string(),
                project_root: project_root_for_bg.clone(),
                session_id: session_id_for_bg.clone(),
                max_agents: Some(schedule_result.plan.subtasks.len()),
                auto_decompose: true,
                strategy: Some(SchedulingStrategy::Phased),
                mode_hint: Some("schedule".to_string()),
                skip_confirmation: true,
            };
            let app_for_bg = app.clone();
            let stream_message_id_for_bg = stream_message_id.clone();
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                let _ = app_for_bg.emit(
                    &format!("chat-stream-{}", stream_message_id_for_bg),
                    &StreamOutputItem::Start,
                );
                let _ = app_for_bg.emit(
                    &format!("chat-stream-{}", stream_message_id_for_bg),
                    &StreamOutputItem::Complete,
                );
                if let Some(state) = app_for_bg.try_state::<OmigaAppState>() {
                    if let Err(e) = self::provider::run_agent_schedule_inner(
                        app_for_bg.clone(),
                        &state,
                        request_for_bg,
                    )
                    .await
                    {
                        tracing::warn!(
                            target: "omiga::scheduler",
                            session_id = %session_id_for_bg,
                            error = %e,
                            "Direct /schedule orchestration failed"
                        );
                    }
                } else {
                    tracing::warn!(target: "omiga::scheduler", "OmigaAppState unavailable for direct /schedule orchestration");
                }
            });

            return Some(MessageResponse {
                message_id: stream_message_id,
                session_id: session_id.to_string(),
                round_id: schedule_round_id,
                user_message_id: Some(user_message_id.to_string()),
                input_kind: Some("schedule_orchestration_started".to_string()),
                scheduler_plan: Some(schedule_result),
                initial_todos: None,
            });
        }
    }

    None
}

struct UpdateAutopilotAfterSchedulingInput<'a> {
    app_state: &'a OmigaAppState,
    repo: &'a crate::domain::persistence::SessionRepository,
    project_root: &'a Path,
    session_id: &'a str,
    request: &'a SendMessageRequest,
    exec_env: &'a str,
    is_autopilot_keyword_route: bool,
    scheduler_result: &'a Option<crate::domain::agents::scheduler::SchedulingResult>,
}

async fn update_autopilot_phase_after_scheduling(input: UpdateAutopilotAfterSchedulingInput<'_>) {
    let UpdateAutopilotAfterSchedulingInput {
        app_state,
        repo,
        project_root,
        session_id,
        request,
        exec_env,
        is_autopilot_keyword_route,
        scheduler_result,
    } = input;

    if is_autopilot_keyword_route {
        if let Some(phase) = crate::domain::orchestration::autopilot::AutopilotOrchestrator::phase_for_scheduler_result(
            project_root,
            session_id,
            scheduler_result.is_some(),
        )
        .await
        {
            update_autopilot_phase_if_needed(
                mode_lifecycle_context!(
                    true,
                    &app_state.chat.sessions,
                    repo,
                    project_root,
                    session_id,
                    ralph_runtime_env_label(
                        exec_env,
                        request.ssh_server.as_deref(),
                        request.local_venv_type.as_deref().unwrap_or(""),
                        request.local_venv_name.as_deref().unwrap_or(""),
                    ),
                    None,
                ),
                phase,
            )
            .await;
        }
    }
}

struct LoadSendLlmPreflightInput<'a> {
    app_state: &'a OmigaAppState,
    repo: &'a crate::domain::persistence::SessionRepository,
    project_root: &'a Path,
    session_id: &'a str,
    user_message_id: &'a str,
    active_provider_entry_name: Option<&'a str>,
    preflight_event_mode: Option<&'a str>,
}

struct SendLlmPreflight {
    llm_config: LlmConfig,
    resolved_runtime_constraints:
        crate::domain::runtime_constraints::ResolvedRuntimeConstraintConfig,
}

async fn load_send_llm_preflight(
    input: LoadSendLlmPreflightInput<'_>,
) -> CommandResult<SendLlmPreflight> {
    let LoadSendLlmPreflightInput {
        app_state,
        repo,
        project_root,
        session_id,
        user_message_id,
        active_provider_entry_name,
        preflight_event_mode,
    } = input;

    // Lazy provider restoration: if this session has a stored provider that differs from the
    // current global, restore it now (first message after session switch).  This moves the
    // ~100 ms config-file read out of load_session (which blocks the UI) and into send_message
    // (where the user is already waiting for the LLM response anyway).
    if let Some(desired) = active_provider_entry_name {
        let desired = desired.trim();
        if !desired.is_empty() {
            let provider_restore_started_at = std::time::Instant::now();
            let current = app_state
                .chat
                .active_provider_entry_name
                .lock()
                .await
                .clone();
            let matches = current.as_deref().map(str::trim) == Some(desired);
            drop(current);
            if !matches {
                match tokio::time::timeout(
                    std::time::Duration::from_secs(3),
                    apply_named_provider_runtime(app_state, desired),
                )
                .await
                {
                    Ok(Ok(_)) => {
                        append_preflight_stage_event(
                            repo,
                            session_id,
                            user_message_id,
                            preflight_event_mode,
                            "provider_restore",
                            provider_restore_started_at.elapsed().as_millis(),
                            serde_json::json!({ "provider": desired, "status": "ok" }),
                        )
                        .await;
                    }
                    Ok(Err(e)) => {
                        tracing::warn!(
                            target: "omiga::llm",
                            "Lazy provider restore for session {} failed ({}), using current config",
                            session_id, e
                        );
                        append_preflight_stage_failed_event(
                            repo,
                            session_id,
                            user_message_id,
                            preflight_event_mode,
                            "provider_restore",
                            provider_restore_started_at.elapsed().as_millis(),
                            &e.to_string(),
                        )
                        .await;
                    }
                    Err(_) => {
                        tracing::warn!(
                            target: "omiga::llm",
                            "Lazy provider restore for session {} timed out; using current config",
                            session_id
                        );
                        append_preflight_stage_failed_event(
                            repo,
                            session_id,
                            user_message_id,
                            preflight_event_mode,
                            "provider_restore",
                            provider_restore_started_at.elapsed().as_millis(),
                            "provider restore timed out",
                        )
                        .await;
                    }
                }
            }
        }
    }

    let llm_config_started_at = std::time::Instant::now();
    let llm_config = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        get_llm_config(&app_state.chat),
    )
    .await
    .map_err(|_| {
        OmigaError::Chat(ChatError::StreamError(
            "Timed out while loading LLM configuration".to_string(),
        ))
    })??;
    append_preflight_stage_event(
        repo,
        session_id,
        user_message_id,
        preflight_event_mode,
        "llm_config",
        llm_config_started_at.elapsed().as_millis(),
        serde_json::json!({ "provider": format!("{:?}", llm_config.provider), "model": llm_config.model }),
    )
    .await;
    let session_runtime_cfg = crate::domain::session::load_session_config(session_id);
    let resolved_runtime_constraints =
        crate::domain::runtime_constraints::resolve_runtime_constraint_config(
            project_root,
            session_runtime_cfg.runtime_constraints.as_ref(),
        );

    Ok(SendLlmPreflight {
        llm_config,
        resolved_runtime_constraints,
    })
}

struct LoadSendPromptPreflightInput<'a> {
    app_state: &'a OmigaAppState,
    repo: &'a crate::domain::persistence::SessionRepository,
    project_root: &'a Path,
    session_id: &'a str,
    user_message_id: &'a str,
    request: &'a SendMessageRequest,
    preflight_event_mode: Option<&'a str>,
}

struct SendPromptPreflight {
    integrations_cfg: integrations_config::IntegrationsConfig,
    skills_exist: bool,
    memory_ctx: Option<String>,
    memory_nav: String,
    skills_system_section: String,
}

async fn load_send_prompt_preflight(
    input: LoadSendPromptPreflightInput<'_>,
) -> SendPromptPreflight {
    let LoadSendPromptPreflightInput {
        app_state,
        repo,
        project_root,
        session_id,
        user_message_id,
        request,
        preflight_event_mode,
    } = input;

    let integrations_cfg = {
        let hit = app_state
            .integrations_config_cache
            .lock()
            .expect("integrations config cache poisoned")
            .get(project_root)
            .filter(|s| s.cached_at.elapsed() < INTEGRATIONS_CONFIG_CACHE_TTL)
            .map(|s| s.config.clone());
        hit.unwrap_or_else(|| {
            // Lock is released above; safe to do a blocking file read here.
            let cfg = integrations_config::load_integrations_config(project_root);
            app_state
                .integrations_config_cache
                .lock()
                .expect("integrations config cache poisoned")
                .insert(
                    project_root.to_path_buf(),
                    IntegrationsConfigCacheSlot {
                        config: cfg.clone(),
                        cached_at: std::time::Instant::now(),
                    },
                );
            cfg
        })
    };
    let working_memory_started_at = std::time::Instant::now();
    match tokio::time::timeout(
        std::time::Duration::from_secs(2),
        crate::domain::memory::working_memory::mark_user_turn_started(repo, session_id),
    )
    .await
    {
        Ok(Ok(_)) => {
            append_preflight_stage_event(
                repo,
                session_id,
                user_message_id,
                preflight_event_mode,
                "working_memory_mark",
                working_memory_started_at.elapsed().as_millis(),
                serde_json::json!({ "status": "ok" }),
            )
            .await;
        }
        Ok(Err(e)) => {
            tracing::warn!(
                target: "omiga::memory",
                session_id = %session_id,
                error = %e,
                "Working memory mark_user_turn_started failed; continuing without blocking chat"
            );
            append_preflight_stage_failed_event(
                repo,
                session_id,
                user_message_id,
                preflight_event_mode,
                "working_memory_mark",
                working_memory_started_at.elapsed().as_millis(),
                &e.to_string(),
            )
            .await;
        }
        Err(_) => {
            tracing::warn!(
                target: "omiga::memory",
                session_id = %session_id,
                "Working memory mark_user_turn_started timed out; continuing without blocking chat"
            );
            append_preflight_stage_failed_event(
                repo,
                session_id,
                user_message_id,
                preflight_event_mode,
                "working_memory_mark",
                working_memory_started_at.elapsed().as_millis(),
                "working memory mark timed out",
            )
            .await;
        }
    }
    // Run independent async I/O in parallel to reduce pre-LLM latency.
    let skill_cache_ref = &app_state.skill_cache;
    let memory_lookup_started_at = std::time::Instant::now();
    let (skills_exist, memory_ctx, memory_nav) =
        match tokio::time::timeout(std::time::Duration::from_secs(3), async {
            tokio::join!(
                skills::skills_any_exist(project_root, skill_cache_ref),
                crate::commands::memory::get_memory_context_cached(
                    repo,
                    project_root,
                    Some(session_id),
                    &request.content,
                    3,
                    Some(&app_state.memory_preflight_cache),
                ),
                crate::commands::memory::memory_navigation_section(project_root),
            )
        })
        .await
        {
            Ok(result) => {
                append_preflight_stage_event(
                    repo,
                    session_id,
                    user_message_id,
                    preflight_event_mode,
                    "memory_context",
                    memory_lookup_started_at.elapsed().as_millis(),
                    serde_json::json!({
                        "skillsExist": result.0,
                        "memoryContext": result.1.is_some(),
                        "memoryNavChars": result.2.len(),
                    }),
                )
                .await;
                result
            }
            Err(_) => {
                tracing::warn!(
                    target: "omiga::memory",
                    session_id = %session_id,
                    "Memory/skill preflight timed out; continuing with no injected memory context"
                );
                append_preflight_stage_failed_event(
                    repo,
                    session_id,
                    user_message_id,
                    preflight_event_mode,
                    "memory_context",
                    memory_lookup_started_at.elapsed().as_millis(),
                    "memory context timed out",
                )
                .await;
                (false, None, String::new())
            }
        };

    let skills_system_section = if skills_exist {
        let icfg = integrations_config::load_integrations_config(project_root);
        let loaded = skills::load_skills_cached(project_root, skill_cache_ref).await;
        let filtered = integrations_config::filter_skill_entries(loaded, &icfg);
        skills::format_skills_index_system_section(project_root, &filtered)
    } else {
        String::new()
    };

    SendPromptPreflight {
        integrations_cfg,
        skills_exist,
        memory_ctx,
        memory_nav,
        skills_system_section,
    }
}

struct BuildSendSystemPromptInput<'a> {
    llm_config: LlmConfig,
    session: &'a Session,
    request: &'a SendMessageRequest,
    repo: &'a crate::domain::persistence::SessionRepository,
    project_root: &'a Path,
    session_id: &'a str,
    user_message_id: &'a str,
    preflight_event_mode: Option<&'a str>,
    mode_execution_lane: Option<crate::domain::orchestration::ExecutionLane>,
    scheduler_result: &'a Option<crate::domain::agents::scheduler::SchedulingResult>,
    keyword_skill_route: Option<&'a crate::domain::routing::SkillRoute>,
    is_plan_mode: bool,
    is_team_keyword_route: bool,
    is_ralph_keyword_route: bool,
    is_autopilot_keyword_route: bool,
    exec_env: &'a str,
    sandbox_backend: &'a str,
    skills_exist: bool,
    memory_ctx: Option<String>,
    memory_nav: String,
    skills_system_section: String,
}

async fn build_send_system_prompt(input: BuildSendSystemPromptInput<'_>) -> LlmConfig {
    let BuildSendSystemPromptInput {
        mut llm_config,
        session,
        request,
        repo,
        project_root,
        session_id,
        user_message_id,
        preflight_event_mode,
        mode_execution_lane,
        scheduler_result,
        keyword_skill_route,
        is_plan_mode,
        is_team_keyword_route,
        is_ralph_keyword_route,
        is_autopilot_keyword_route,
        exec_env,
        sandbox_backend,
        skills_exist,
        memory_ctx,
        memory_nav,
        skills_system_section,
    } = input;

    // Ported agent system prompt from `src/constants/prompts.ts` — injected when tools are enabled.
    let mut prompt_parts: Vec<String> = Vec::new();
    if request.use_tools {
        prompt_parts.push(agent_prompt::build_system_prompt(
            project_root,
            &llm_config.model,
        ));
        if is_plan_mode {
            prompt_parts.push(agent_prompt::active_plan_mode_turn_addendum().to_string());
        }
        if coordinator::is_coordinator_mode() {
            prompt_parts.push(agent_prompt::coordinator_mode_addendum().to_string());
        }
    }
    // 用户级 SOUL / MEMORY / USER 与 ~/.omiga + 项目 .omiga 人格配置（compose_full_agent_system_prompt 会读取同目录下的 personalities）
    let user_omiga_ctx = crate::domain::agents::load_user_omiga_context();
    for sec in user_omiga_ctx.main_system_prompt_sections() {
        prompt_parts.push(sec);
    }
    if let Some(ref u) = llm_config.system_prompt {
        let t = u.trim();
        if !t.is_empty() {
            prompt_parts.push(t.to_string());
        }
    }
    if skills_exist {
        prompt_parts.push(skills_system_section);
    }
    let plugin_load_outcome = crate::domain::plugins::plugin_load_outcome();
    if let Some(plugins_system_section) =
        crate::domain::plugins::format_plugins_system_section(&plugin_load_outcome)
    {
        prompt_parts.push(plugins_system_section);
    }
    if let Some(operator_tools_system_section) =
        crate::domain::operators::format_enabled_operator_tools_system_section()
    {
        prompt_parts.push(operator_tools_system_section);
    }
    if let Some(selected_plugins_system_section) =
        crate::domain::plugins::format_selected_plugins_system_section(
            &plugin_load_outcome,
            &request.selected_plugin_ids,
        )
    {
        prompt_parts.push(selected_plugins_system_section);
    }
    let connector_catalog = crate::domain::connectors::list_connector_catalog();
    if let Some(connectors_system_section) =
        crate::domain::connectors::format_connectors_system_section(&connector_catalog)
    {
        prompt_parts.push(connectors_system_section);
    }
    // Memory navigation guide — always injected to override the model's default
    // "I have no cross-session memory" belief and tell it where to look.
    let nav = memory_nav.trim().to_string();
    if !nav.is_empty() {
        prompt_parts.push(nav);
    }
    if let Some(ctx) = memory_ctx {
        prompt_parts.push(ctx);
    }
    let overlay_started_at = std::time::Instant::now();
    match tokio::time::timeout(
        std::time::Duration::from_secs(1),
        crate::domain::agents::build_runtime_overlay(project_root),
    )
    .await
    {
        Ok(Some(overlay)) => {
            prompt_parts.push(overlay);
            append_preflight_stage_event(
                repo,
                session_id,
                user_message_id,
                preflight_event_mode,
                "runtime_overlay",
                overlay_started_at.elapsed().as_millis(),
                serde_json::json!({ "status": "ok" }),
            )
            .await;
        }
        Ok(None) => {}
        Err(_) => {
            tracing::warn!(
                target: "omiga::overlay",
                session_id = %session_id,
                "Runtime overlay preflight timed out; continuing without overlay"
            );
            append_preflight_stage_failed_event(
                repo,
                session_id,
                user_message_id,
                preflight_event_mode,
                "runtime_overlay",
                overlay_started_at.elapsed().as_millis(),
                "runtime overlay timed out",
            )
            .await;
        }
    }
    if is_ralph_keyword_route {
        if let Some(resume_ctx) =
            crate::domain::mode_resume::build_ralph_resume_context(project_root, session_id).await
        {
            prompt_parts.push(resume_ctx);
        }
        if let Some(phase_guidance) =
            crate::domain::mode_resume::build_ralph_phase_guidance(project_root, session_id).await
        {
            prompt_parts.push(phase_guidance);
        }
    }
    if is_autopilot_keyword_route {
        if let Some(resume_ctx) =
            crate::domain::mode_resume::build_autopilot_resume_context(project_root, session_id)
                .await
        {
            prompt_parts.push(resume_ctx);
        }
        if let Some(phase_guidance) =
            crate::domain::mode_resume::build_autopilot_phase_guidance(project_root, session_id)
                .await
        {
            prompt_parts.push(phase_guidance);
        }
    }
    if is_team_keyword_route {
        if let Some(resume_ctx) =
            crate::domain::mode_resume::build_team_resume_context(project_root, session_id).await
        {
            prompt_parts.push(resume_ctx);
        }
        if let Some(phase_guidance) =
            crate::domain::mode_resume::build_team_phase_guidance(project_root, session_id).await
        {
            prompt_parts.push(phase_guidance);
        }
    }
    if let Some(lane) = mode_execution_lane {
        prompt_parts.push(format!(
            "## Execution Lane: {}\n{}",
            lane.lane_id, lane.instructions
        ));
    }

    // 如果有调度计划（任务分解），添加到 system prompt
    if let Some(ref schedule_result) = scheduler_result {
        let plan_description = format_scheduler_plan(schedule_result);
        prompt_parts.push(plan_description);
    }

    // Keyword-routed skill: inject SKILL.md body directly into system prompt.
    // This implements OMX-style auto-invocation — the skill's instructions are active
    // from token 0, no LLM decision needed to call the Skill tool first.
    if let Some(route) = keyword_skill_route {
        let skill_body =
            crate::domain::routing::load_skill_body(&route.skill_name, &route.args, project_root)
                .await;
        if let Some(body) = skill_body {
            tracing::info!(
                target: "omiga::routing",
                skill = %route.skill_name,
                body_len = body.len(),
                "Injected skill body into system prompt"
            );
            prompt_parts.push(format!(
                "## Active Skill: {}\n\n{}\n\n---\nThe user's task (for $ARGUMENTS context): {}",
                route.skill_name, body, route.args
            ));
        } else {
            // Skill file not found — fall back to a plain instruction
            tracing::warn!(
                target: "omiga::routing",
                skill = %route.skill_name,
                "Skill body not found on disk; falling back to hint"
            );
            prompt_parts.push(format!(
                "## Active Skill: {}\n\nInvoke the `{}` skill immediately as your first action to handle this task.",
                route.skill_name, route.skill_name
            ));
        }
    }

    if request.use_tools {
        let selected_composer = request
            .composer_agent_type
            .as_deref()
            .map(str::trim)
            .unwrap_or("");
        if let Some(lane) = mode_execution_lane {
            if (selected_composer.is_empty()
                || selected_composer == "auto"
                || selected_composer == "general-purpose")
                && (lane.preferred_agent_type.is_some()
                    || !lane.supplemental_agent_types.is_empty())
            {
                let router = crate::domain::agents::get_agent_router();
                let tool_ctx = ToolContext::new(project_root.to_path_buf())
                    .with_execution_environment(exec_env.to_string())
                    .with_ssh_server(request.ssh_server.clone())
                    .with_sandbox_backend(sandbox_backend.to_string())
                    .with_local_venv(
                        request.local_venv_type.as_deref().unwrap_or(""),
                        request.local_venv_name.as_deref().unwrap_or(""),
                    );
                let mut injected: Vec<&str> = Vec::new();
                if let Some(primary) = lane.preferred_agent_type {
                    injected.push(primary);
                }
                for supplemental in lane.supplemental_agent_types {
                    if !injected.contains(supplemental) {
                        injected.push(supplemental);
                    }
                }
                for agent_type in injected {
                    if let Some(agent) = router.get_agent(agent_type) {
                        prompt_parts.push(crate::domain::agents::compose_full_agent_system_prompt(
                            agent, &tool_ctx,
                        ));
                    }
                }
            }
        }
        if let Some(ref at) = request.composer_agent_type {
            let t = at.trim();
            if !t.is_empty() && t != "general-purpose" {
                let router = crate::domain::agents::get_agent_router();
                let agent = router.select_agent(Some(t));
                let tool_ctx = ToolContext::new(project_root.to_path_buf())
                    .with_execution_environment(exec_env.to_string())
                    .with_ssh_server(request.ssh_server.clone())
                    .with_sandbox_backend(sandbox_backend.to_string())
                    .with_local_venv(
                        request.local_venv_type.as_deref().unwrap_or(""),
                        request.local_venv_name.as_deref().unwrap_or(""),
                    );
                prompt_parts.push(crate::domain::agents::compose_full_agent_system_prompt(
                    agent, &tool_ctx,
                ));
            }
        }
        if let Some(line) = composer_execution_addendum(
            exec_env,
            request.ssh_server.as_deref(),
            request.local_venv_type.as_deref().unwrap_or(""),
            request.local_venv_name.as_deref().unwrap_or(""),
        ) {
            prompt_parts.push(line);
        }
    }
    // Intent-based routing hint — appended only when a specialist agent is relevant
    if let Some(hint) = crate::domain::agents::intent_classifier::build_system_hint(
        &crate::domain::agents::intent_classifier::classify(&request.content),
    ) {
        prompt_parts.push(hint);
    }

    llm_config.system_prompt = if prompt_parts.is_empty() {
        None
    } else {
        Some(prompt_parts.join("\n\n"))
    };
    llm_config.prompt_cache_key = Some(session.id.clone());
    llm_config
}

struct CompactAndRoundInput<'a> {
    app: &'a AppHandle,
    app_state: &'a OmigaAppState,
    repo: &'a crate::domain::persistence::SessionRepository,
    session: Session,
    llm_config: LlmConfig,
    session_id: &'a str,
    user_message_id: &'a str,
    request: &'a SendMessageRequest,
    preflight_event_mode: Option<&'a str>,
    trace_mode: Option<&'a str>,
    send_message_started_at: std::time::Instant,
    computer_use_mode: crate::domain::computer_use::ComputerUseMode,
    browser_use_mode: crate::domain::browser_operator::BrowserUseMode,
    scheduler_result: &'a Option<crate::domain::agents::scheduler::SchedulingResult>,
}

struct SendRoundPreparation {
    messages: Vec<crate::api::Message>,
    llm_config_for_agent: LlmConfig,
    client: Box<dyn LlmClient>,
    compact_log_for_stream: Option<String>,
    round_id: String,
    message_id: String,
    cancel_flag: Arc<RwLock<bool>>,
    round_cancel: tokio_util::sync::CancellationToken,
}

async fn compact_session_and_prepare_round(
    input: CompactAndRoundInput<'_>,
) -> CommandResult<SendRoundPreparation> {
    let CompactAndRoundInput {
        app,
        app_state,
        repo,
        mut session,
        llm_config,
        session_id,
        user_message_id,
        request,
        preflight_event_mode,
        trace_mode,
        send_message_started_at,
        computer_use_mode,
        browser_use_mode,
        scheduler_result,
    } = input;

    let compaction_context = crate::domain::auto_compact::CompactionContext {
        last_turn_input_tokens: last_turn_input_tokens_for_compaction(&session.messages),
    };
    if let Some(removed_messages) =
        crate::domain::auto_compact::preview_removed_messages_for_compaction(
            &session.messages,
            &llm_config,
            request.use_tools,
            compaction_context,
        )
    {
        let should_prepare =
            match crate::domain::memory::working_memory::should_prepare_for_auto_compact(
                repo, session_id,
            )
            .await
            {
                Ok(value) => value,
                Err(e) => {
                    tracing::warn!(
                        target: "omiga::memory",
                        session_id = %session_id,
                        error = %e,
                        "checking pre-compact preparation throttle failed; preparing memory once defensively"
                    );
                    true
                }
            };
        if should_prepare {
            let op_id = format!("memory-precompact-{}", uuid::Uuid::new_v4());
            emit_activity_operation(
                app,
                session_id,
                &op_id,
                "压缩前摘要",
                "running",
                Some(format!(
                    "准备提炼 {} 条即将压缩的消息",
                    removed_messages.len()
                )),
            );
            match tokio::time::timeout(
                std::time::Duration::from_secs(3),
                crate::domain::memory::working_memory::prepare_for_auto_compact(
                    repo,
                    session_id,
                    &removed_messages,
                ),
            )
            .await
            {
                Ok(Ok(compact_state)) => {
                    emit_activity_operation(
                        app,
                        session_id,
                        &op_id,
                        "压缩前摘要",
                        "done",
                        Some("已提炼即将被压缩的上下文".to_string()),
                    );
                    let _ = compact_state;
                    // Pre-compact memory is only a scratchpad used to avoid losing
                    // context during this active turn. Do not promote/archive it here:
                    // failed or tool-limit turns must not silently update long-term memory.
                }
                Ok(Err(e)) => {
                    tracing::warn!(
                        target: "omiga::memory",
                        session_id = %session_id,
                        error = %e,
                        "Working memory prepare_for_auto_compact failed; continuing without blocking chat"
                    );
                    emit_activity_operation(
                        app,
                        session_id,
                        &op_id,
                        "压缩前摘要",
                        "error",
                        Some(e.to_string()),
                    );
                }
                Err(_) => {
                    tracing::warn!(
                        target: "omiga::memory",
                        session_id = %session_id,
                        "Working memory prepare_for_auto_compact timed out; continuing without blocking chat"
                    );
                    emit_activity_operation(
                        app,
                        session_id,
                        &op_id,
                        "压缩前摘要",
                        "error",
                        Some("prepare_for_auto_compact timed out".to_string()),
                    );
                }
            }
        }
    }

    let compact_started_at = std::time::Instant::now();
    let compact_outcome = match tokio::time::timeout(
        std::time::Duration::from_secs(5),
        crate::domain::auto_compact::compact_session_and_persist(
            repo,
            session_id,
            &mut session,
            &llm_config,
            request.use_tools,
            compaction_context,
            user_message_id,
        ),
    )
    .await
    {
        Ok(Ok(outcome)) => {
            append_preflight_stage_event(
                repo,
                session_id,
                user_message_id,
                preflight_event_mode,
                "auto_compact",
                compact_started_at.elapsed().as_millis(),
                serde_json::json!({ "compacted": outcome.is_some() }),
            )
            .await;
            outcome
        }
        Ok(Err(e)) => {
            return Err(OmigaError::Chat(ChatError::StreamError(format!(
                "Auto-compact failed: {}",
                e
            ))));
        }
        Err(_) => {
            tracing::warn!(
                target: "omiga::auto_compact",
                session_id = %session_id,
                "Auto-compact timed out; continuing with current transcript"
            );
            append_preflight_stage_failed_event(
                repo,
                session_id,
                user_message_id,
                preflight_event_mode,
                "auto_compact",
                compact_started_at.elapsed().as_millis(),
                "auto compact timed out",
            )
            .await;
            None
        }
    };

    let user_message_id_for_round = compact_outcome
        .as_ref()
        .map(|p| p.last_user_message_id.clone())
        .unwrap_or_else(|| user_message_id.to_string());

    {
        let mut sessions = app_state.chat.sessions.write().await;
        if let Some(runtime) = sessions.get_mut(session_id) {
            runtime.session = session.clone();
        }
    }

    let messages = SessionCodec::to_api_messages(&session.messages);

    let llm_config_for_agent = llm_config.clone();
    let client = create_client(llm_config)?;

    let compact_log_for_stream = compact_outcome.map(|p| p.log_line);

    if trace_mode.is_some() {
        append_preflight_stage_event(
            repo,
            session_id,
            user_message_id,
            trace_mode,
            "send_message_ready",
            send_message_started_at.elapsed().as_millis(),
            serde_json::json!({
                "toolsEnabled": request.use_tools,
                "computerUseMode": computer_use_mode.as_str(),
                "computerUseEnabled": computer_use_mode.is_enabled(),
                "browserUseMode": browser_use_mode.as_str(),
                "browserUseEnabled": browser_use_mode.is_enabled(),
                "schedulerBuiltPlan": scheduler_result.is_some(),
            }),
        )
        .await;
    }

    // Generate round and message IDs
    let round_id = uuid::Uuid::new_v4().to_string();
    let message_id = uuid::Uuid::new_v4().to_string();

    // Create conversation round record
    tokio::time::timeout(
        std::time::Duration::from_secs(3),
        repo.create_round(
            &round_id,
            session_id,
            &message_id,
            Some(&user_message_id_for_round),
        ),
    )
    .await
    .map_err(|_| {
        OmigaError::Chat(ChatError::StreamError(
            "Timed out while creating conversation round".to_string(),
        ))
    })?
    .map_err(|e| {
        OmigaError::Chat(ChatError::StreamError(format!(
            "Failed to create round: {}",
            e
        )))
    })?;

    // Set up cancellation tracking
    let cancel_flag = Arc::new(RwLock::new(false));
    let round_cancel = tokio_util::sync::CancellationToken::new();
    let cancellation_state = RoundCancellationState {
        round_id: round_id.clone(),
        message_id: message_id.clone(),
        session_id: session_id.to_string(),
        cancelled: cancel_flag.clone(),
        round_cancel: round_cancel.clone(),
    };

    {
        let mut active_rounds = app_state.chat.active_rounds.lock().await;
        active_rounds.insert(message_id.clone(), cancellation_state);
    }

    // Update runtime state with active round
    {
        let mut sessions = app_state.chat.sessions.write().await;
        if let Some(runtime) = sessions.get_mut(session_id) {
            runtime.active_round_ids.push(round_id.clone());
        }
    }

    Ok(SendRoundPreparation {
        messages,
        llm_config_for_agent,
        client,
        compact_log_for_stream,
        round_id,
        message_id,
        cancel_flag,
        round_cancel,
    })
}

struct PrepareSendToolsInput<'a> {
    app_state: &'a OmigaAppState,
    repo: &'a crate::domain::persistence::SessionRepository,
    project_root: &'a Path,
    session_id: &'a str,
    user_message_id: &'a str,
    request: &'a SendMessageRequest,
    integrations_cfg: &'a integrations_config::IntegrationsConfig,
    trace_mode: Option<&'a str>,
    computer_use_mode: crate::domain::computer_use::ComputerUseMode,
    browser_use_mode: crate::domain::browser_operator::BrowserUseMode,
}

async fn prepare_send_tools(input: PrepareSendToolsInput<'_>) -> Vec<ToolSchema> {
    let PrepareSendToolsInput {
        app_state,
        repo,
        project_root,
        session_id,
        user_message_id,
        request,
        integrations_cfg,
        trace_mode,
        computer_use_mode,
        browser_use_mode,
    } = input;

    // Prepare tools if enabled (`list_skills` + `skill` when skills exist on disk).
    // Merge MCP `tools/list` from Omiga MCP config (stdio / HTTP), same naming as Claude Code (`mcp__server__tool`).
    // Filter with `permissions.deny` from Claude-style settings (`filterToolsByDenyRules` parity).
    if request.use_tools {
        let tool_schema_stage_started_at = std::time::Instant::now();
        let deny_entries = {
            let hit = app_state
                .chat
                .permission_deny_cache
                .lock()
                .await
                .get(project_root)
                .filter(|e| e.cached_at.elapsed() < PERMISSION_DENY_CACHE_TTL)
                .map(|e| e.entries.clone());
            match hit {
                Some(entries) => {
                    tracing::debug!(target: "omiga::permissions", "permission deny rules served from cache");
                    entries
                }
                None => {
                    // Lock released above; safe to do blocking file reads here.
                    let entries = load_merged_permission_deny_rule_entries(project_root);
                    app_state.chat.permission_deny_cache.lock().await.insert(
                        project_root.to_path_buf(),
                        PermissionDenyCache {
                            entries: entries.clone(),
                            cached_at: std::time::Instant::now(),
                        },
                    );
                    entries
                }
            }
        };
        validate_permission_deny_entries(&deny_entries);
        // Always expose skill tools. A slow/failed preflight should not hide
        // `list_skills` / `skill_view` / `skill` and push the model toward bash.
        let mut all_schemas = all_tool_schemas(true);
        if computer_use_mode.is_enabled() {
            all_schemas.extend(crate::domain::computer_use::facade_tool_schemas());
        }
        if browser_use_mode.is_enabled() {
            all_schemas.extend(crate::domain::browser_operator::facade_tool_schemas());
        }
        let n_builtin_before = all_schemas.len();
        let mut built = filter_tool_schemas_by_deny_rule_entries(all_schemas, &deny_entries);
        let n_builtin_after = built.len();
        if n_builtin_after < n_builtin_before {
            tracing::debug!(
                target: "omiga::permissions",
                before = n_builtin_before,
                after = n_builtin_after,
                "built-in tool schemas after permissions.deny filter"
            );
        }
        sort_tool_schemas_for_model(&mut built);
        let operator_schemas = crate::domain::operators::enabled_operator_tool_schemas();
        let n_operator_before = operator_schemas.len();
        let operator_after_deny =
            filter_tool_schemas_by_deny_rule_entries(operator_schemas, &deny_entries);
        let n_operator_after = operator_after_deny.len();
        if n_operator_after < n_operator_before {
            tracing::debug!(
                target: "omiga::operators",
                before = n_operator_before,
                after = n_operator_after,
                "operator tool schemas after permissions.deny filter"
            );
        }
        let mut base_names: HashSet<String> = built.iter().map(|t| t.name.clone()).collect();
        let operator_filtered: Vec<_> = operator_after_deny
            .into_iter()
            .filter(|schema| base_names.insert(schema.name.clone()))
            .collect();
        let mcp_stage_started_at = std::time::Instant::now();
        let current_mcp_config_signature =
            crate::domain::mcp::merged_mcp_servers_signature(project_root);
        let (mcp_tools, mcp_cache_status) = {
            let cached = app_state
                .chat
                .mcp_tool_cache
                .lock()
                .await
                .get(project_root)
                .map(|e| {
                    (
                        e.schemas.clone(),
                        e.cached_at.elapsed() < MCP_TOOL_CACHE_TTL,
                        e.config_signature == current_mcp_config_signature,
                    )
                });
            match cached {
                Some((schemas, true, true)) => {
                    tracing::debug!(target: "omiga::mcp", "MCP tool schemas served from cache");
                    (schemas, "fresh")
                }
                Some((schemas, true, false)) => {
                    tracing::info!(
                        target: "omiga::mcp",
                        cached = schemas.len(),
                        "MCP tool cache config signature changed; ignoring stale schemas and refreshing in background"
                    );
                    let mcp_tool_cache = app_state.chat.mcp_tool_cache.clone();
                    let root = project_root.to_path_buf();
                    tokio::spawn(async move {
                        let config_signature =
                            crate::domain::mcp::merged_mcp_servers_signature(&root);
                        let schemas = crate::domain::mcp::tool_pool::discover_mcp_tool_schemas(
                            &root,
                            std::time::Duration::from_secs(10),
                        )
                        .await;
                        mcp_tool_cache.lock().await.insert(
                            root,
                            McpToolCache {
                                schemas,
                                cached_at: std::time::Instant::now(),
                                config_signature,
                            },
                        );
                    });
                    (vec![], "config-changed")
                }
                Some((schemas, false, true)) => {
                    tracing::info!(
                        target: "omiga::mcp",
                        cached = schemas.len(),
                        "MCP tool cache stale; withholding stale schemas and refreshing in background"
                    );
                    let mcp_tool_cache = app_state.chat.mcp_tool_cache.clone();
                    let root = project_root.to_path_buf();
                    tokio::spawn(async move {
                        let config_signature =
                            crate::domain::mcp::merged_mcp_servers_signature(&root);
                        let schemas = crate::domain::mcp::tool_pool::discover_mcp_tool_schemas(
                            &root,
                            std::time::Duration::from_secs(10),
                        )
                        .await;
                        mcp_tool_cache.lock().await.insert(
                            root,
                            McpToolCache {
                                schemas,
                                cached_at: std::time::Instant::now(),
                                config_signature,
                            },
                        );
                    });
                    (vec![], "stale-refreshing")
                }
                Some((schemas, false, false)) => {
                    tracing::info!(
                        target: "omiga::mcp",
                        cached = schemas.len(),
                        "MCP tool cache stale and config signature changed; ignoring schemas and refreshing in background"
                    );
                    let mcp_tool_cache = app_state.chat.mcp_tool_cache.clone();
                    let root = project_root.to_path_buf();
                    tokio::spawn(async move {
                        let config_signature =
                            crate::domain::mcp::merged_mcp_servers_signature(&root);
                        let schemas = crate::domain::mcp::tool_pool::discover_mcp_tool_schemas(
                            &root,
                            std::time::Duration::from_secs(10),
                        )
                        .await;
                        mcp_tool_cache.lock().await.insert(
                            root,
                            McpToolCache {
                                schemas,
                                cached_at: std::time::Instant::now(),
                                config_signature,
                            },
                        );
                    });
                    (vec![], "config-changed")
                }
                None => {
                    tracing::info!(
                        target: "omiga::mcp",
                        "MCP tool cache cold; warming in background without blocking first response"
                    );
                    let mcp_tool_cache = app_state.chat.mcp_tool_cache.clone();
                    let root = project_root.to_path_buf();
                    tokio::spawn(async move {
                        let config_signature =
                            crate::domain::mcp::merged_mcp_servers_signature(&root);
                        let schemas = crate::domain::mcp::tool_pool::discover_mcp_tool_schemas(
                            &root,
                            std::time::Duration::from_secs(10),
                        )
                        .await;
                        mcp_tool_cache.lock().await.insert(
                            root,
                            McpToolCache {
                                schemas,
                                cached_at: std::time::Instant::now(),
                                config_signature,
                            },
                        );
                    });
                    (vec![], "cold")
                }
            }
        };
        if trace_mode.is_some() {
            append_preflight_stage_event(
                repo,
                session_id,
                user_message_id,
                trace_mode,
                "mcp_tools",
                mcp_stage_started_at.elapsed().as_millis(),
                serde_json::json!({
                    "toolCount": mcp_tools.len(),
                    "cacheStatus": mcp_cache_status,
                }),
            )
            .await;
        }
        let n_mcp_before = mcp_tools.len();
        let mcp_current = crate::domain::mcp::tool_pool::filter_mcp_tool_schemas_for_current_config(
            project_root,
            mcp_tools,
        );
        if mcp_current.len() < n_mcp_before {
            tracing::info!(
                target: "omiga::mcp",
                before = n_mcp_before,
                after = mcp_current.len(),
                "filtered MCP tool schemas that no longer belong to the effective MCP config"
            );
        }
        let mcp_after_deny = filter_tool_schemas_by_deny_rule_entries(mcp_current, &deny_entries);
        let n_mcp_after = mcp_after_deny.len();
        if n_mcp_after < n_mcp_before {
            tracing::debug!(
                target: "omiga::permissions",
                before = n_mcp_before,
                after = n_mcp_after,
                "MCP tool schemas after permissions.deny filter"
            );
        }
        let mcp_filtered: Vec<_> = mcp_after_deny
            .into_iter()
            .filter(|t| !base_names.contains(&t.name))
            .collect();
        let mcp_filtered =
            integrations_config::filter_mcp_tools_by_integrations(mcp_filtered, integrations_cfg);
        let mut combined: Vec<ToolSchema> = built
            .into_iter()
            .chain(operator_filtered)
            .chain(mcp_filtered)
            .collect();
        if coordinator::is_coordinator_mode() {
            let before = combined.len();
            combined = coordinator::filter_coordinator_tool_schemas(combined);
            tracing::info!(
                target: "omiga::coordinator",
                before,
                after = combined.len(),
                "coordinator mode: tool list restricted to orchestration tools"
            );
        }
        if trace_mode.is_some() {
            append_preflight_stage_event(
                repo,
                session_id,
                user_message_id,
                trace_mode,
                "tool_schemas",
                tool_schema_stage_started_at.elapsed().as_millis(),
                serde_json::json!({
                    "toolCount": combined.len(),
                    "builtinCount": n_builtin_after,
                    "operatorCount": n_operator_after,
                    "mcpCount": n_mcp_after,
                }),
            )
            .await;
        }
        combined
    } else {
        vec![]
    }
}

struct PrepareTurnRuntimeInput<'a> {
    app: &'a AppHandle,
    message_id: &'a str,
    round_id: &'a str,
    session_id: &'a str,
    sessions: &'a Arc<RwLock<HashMap<String, SessionRuntimeState>>>,
    repo: &'a Arc<crate::domain::persistence::SessionRepository>,
    compact_log_for_stream: Option<String>,
    project_root: &'a Path,
    cancel_flag: &'a Arc<RwLock<bool>>,
    round_cancel: &'a tokio_util::sync::CancellationToken,
    pending_tools: &'a Arc<Mutex<HashMap<String, PendingToolCall>>>,
    resolved_runtime_constraints:
        crate::domain::runtime_constraints::ResolvedRuntimeConstraintConfig,
    llm_messages: &'a [LlmMessage],
    request_text_for_constraints: &'a str,
    project_root_for_constraints: &'a Path,
    tools: &'a [ToolSchema],
    turn_spawn_context: &'a TurnSpawnContext,
    project_root_for_ralph: &'a Path,
    project_root_for_autopilot: &'a Path,
}

struct TurnRuntimePreparation {
    tool_results_dir: PathBuf,
    agent_runtime: AgentLlmRuntime,
    constraint_harness: RuntimeConstraintHarness,
    constraint_state: RuntimeConstraintState,
    initial_llm_messages: Vec<LlmMessage>,
}

async fn prepare_turn_runtime(input: PrepareTurnRuntimeInput<'_>) -> TurnRuntimePreparation {
    let PrepareTurnRuntimeInput {
        app,
        message_id,
        round_id,
        session_id,
        sessions,
        repo,
        compact_log_for_stream,
        project_root,
        cancel_flag,
        round_cancel,
        pending_tools,
        resolved_runtime_constraints,
        llm_messages,
        request_text_for_constraints,
        project_root_for_constraints,
        tools,
        turn_spawn_context,
        project_root_for_ralph,
        project_root_for_autopilot,
    } = input;

    // Give the frontend a short window to receive `message_id` from `send_message`
    // and subscribe to `chat-stream-{message_id}` before the first stream event.
    // Without this, very fast failures/responses can be emitted before the listener
    // exists, leaving the UI stuck on “waiting for response”.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let _ = app.emit(
        &format!("chat-stream-{}", message_id),
        &StreamOutputItem::Start,
    );

    if let Some(note) = compact_log_for_stream {
        let _ = app.emit(
            &format!("chat-stream-{}", message_id),
            &StreamOutputItem::Metadata {
                key: "omiga_auto_compact".to_string(),
                value: note,
            },
        );
    }

    // Store tool output artifacts inside the project so they're visible to the user.
    // Fall back to app_data_dir only when the project root is unavailable.
    let tool_results_dir = {
        let sessions_guard = sessions.read().await;
        let pr = sessions_guard
            .get(session_id)
            .map(|r| resolve_session_project_root(&r.session.project_path));
        drop(sessions_guard);
        match pr {
            Some(p) if p.as_os_str() != std::ffi::OsStr::new(".") => {
                p.join(".omiga").join("tool-results").join(session_id)
            }
            _ => tool_results_dir_for_session(app, session_id),
        }
    };

    let (
        plan_mode_flag,
        execution_environment,
        ssh_server_rt,
        sandbox_backend_rt,
        local_venv_type_rt,
        local_venv_name_rt,
        env_store_rt,
    ) = {
        let sessions_guard = sessions.read().await;
        let s = sessions_guard.get(session_id);
        (
            s.map(|x| x.plan_mode.clone()),
            s.map(|x| x.execution_environment.clone())
                .unwrap_or_else(|| "local".to_string()),
            s.and_then(|x| x.ssh_server.clone()),
            s.map(|x| x.sandbox_backend.clone())
                .unwrap_or_else(|| "docker".to_string()),
            s.map(|x| x.local_venv_type.clone()).unwrap_or_default(),
            s.map(|x| x.local_venv_name.clone()).unwrap_or_default(),
            s.map(|x| x.env_store.clone())
                .unwrap_or_else(crate::domain::tools::env_store::EnvStore::new),
        )
    };

    // Pre-warm the SSH/sandbox connection so the first tool call doesn't pay the
    // full SSH handshake cost on top of the 45 s outer tool timeout.
    // warmup() is fire-and-forget (non-blocking) — the session continues immediately.
    if execution_environment != "local" {
        let warmup_ctx = ToolContext::new(project_root.to_path_buf())
            .with_execution_environment(execution_environment.clone())
            .with_ssh_server(ssh_server_rt.clone())
            .with_sandbox_backend(sandbox_backend_rt.clone());
        env_store_rt.warmup(&warmup_ctx).await;
    }

    let agent_runtime = AgentLlmRuntime {
        llm_config: turn_spawn_context.llm_config.clone(),
        round_id: round_id.to_string(),
        cancel_flag: cancel_flag.clone(),
        pending_tools: pending_tools.clone(),
        repo: repo.clone(),
        plan_mode_flag,
        allow_nested_agent: env_allow_nested_agent(),
        round_cancel: round_cancel.clone(),
        execution_environment,
        ssh_server: ssh_server_rt,
        sandbox_backend: sandbox_backend_rt,
        local_venv_type: local_venv_type_rt,
        local_venv_name: local_venv_name_rt,
        env_store: env_store_rt,
        runtime_constraints_config: resolved_runtime_constraints.clone(),
    };

    let constraint_harness =
        RuntimeConstraintHarness::from_config(agent_runtime.runtime_constraints_config.clone());
    let mut constraint_state = RuntimeConstraintState::default();
    let (initial_llm_messages, initial_notices) = augment_llm_messages_with_runtime_constraints(
        llm_messages,
        &constraint_harness,
        &mut constraint_state,
        request_text_for_constraints,
        project_root_for_constraints,
        !tools.is_empty(),
        false,
    );
    emit_runtime_constraint_metadata(
        app,
        repo,
        session_id,
        round_id,
        message_id,
        "runtime_constraints.config",
        serde_json::json!({
            "enabled": agent_runtime.runtime_constraints_config.enabled,
            "buffer_responses": agent_runtime.runtime_constraints_config.buffer_responses,
            "policy_pack": agent_runtime.runtime_constraints_config.policy_pack,
            "registry": constraint_harness.registry().into_iter().map(|m| serde_json::json!({
                "id": m.id,
                "severity": m.severity,
                "enabled": m.enabled,
            })).collect::<Vec<_>>(),
        }),
    )
    .await;
    if !initial_notices.is_empty() {
        emit_runtime_constraint_metadata(
            app,
            repo,
            session_id,
            round_id,
            message_id,
            "runtime_constraints.notices",
            serde_json::json!({ "ids": initial_notices }),
        )
        .await;
    }

    update_ralph_phase_if_needed(
        mode_lifecycle_context!(
            turn_spawn_context.is_ralph_mode,
            sessions,
            repo,
            project_root_for_ralph,
            session_id,
            turn_spawn_context.ralph_env.clone(),
            Some(round_id),
        ),
        crate::domain::ralph_state::RalphPhase::EnvCheck,
    )
    .await;
    update_autopilot_phase_if_needed(
        mode_lifecycle_context!(
            turn_spawn_context.is_autopilot_mode,
            sessions,
            repo,
            project_root_for_autopilot,
            session_id,
            turn_spawn_context.autopilot_env.clone(),
            Some(round_id),
        ),
        crate::domain::autopilot_state::AutopilotPhase::Design,
    )
    .await;

    TurnRuntimePreparation {
        tool_results_dir,
        agent_runtime,
        constraint_harness,
        constraint_state,
        initial_llm_messages,
    }
}

type CompletedToolCall = (String, String, String);
type ToolResultRow = (String, String, bool);

struct StreamAssistantTurnInput<'a> {
    client: &'a dyn LlmClient,
    app: &'a AppHandle,
    message_id: &'a str,
    round_id: &'a str,
    messages: &'a [LlmMessage],
    tools: &'a [ToolSchema],
    emit_text_chunks: bool,
    pending_tools: &'a Arc<Mutex<HashMap<String, PendingToolCall>>>,
    cancel_flag: &'a Arc<RwLock<bool>>,
    repo: &'a Arc<crate::domain::persistence::SessionRepository>,
    sessions: &'a Arc<RwLock<HashMap<String, SessionRuntimeState>>>,
    session_id: &'a str,
    turn_spawn_context: &'a TurnSpawnContext,
    project_root_for_ralph: &'a Path,
    project_root_for_autopilot: &'a Path,
    project_root_for_team: &'a Path,
    ralph_failure_phase: crate::domain::ralph_state::RalphPhase,
    autopilot_failure_phase: crate::domain::autopilot_state::AutopilotPhase,
}

struct AssistantStreamResult {
    tool_calls: Vec<CompletedToolCall>,
    text: String,
    reasoning: String,
    was_cancelled: bool,
    usage: Option<crate::llm::TokenUsage>,
}

async fn stream_assistant_turn_with_cancel(
    input: StreamAssistantTurnInput<'_>,
) -> Option<AssistantStreamResult> {
    let StreamAssistantTurnInput {
        client,
        app,
        message_id,
        round_id,
        messages,
        tools,
        emit_text_chunks,
        pending_tools,
        cancel_flag,
        repo,
        sessions,
        session_id,
        turn_spawn_context,
        project_root_for_ralph,
        project_root_for_autopilot,
        project_root_for_team,
        ralph_failure_phase,
        autopilot_failure_phase,
    } = input;

    match stream_llm_response_with_cancel(StreamLlmRequest {
        client,
        app,
        message_id,
        round_id,
        messages,
        tools,
        emit_text_chunks,
        pending_tools,
        cancel_flag,
        repo: repo.clone(),
    })
    .await
    {
        Ok((tool_calls, text, reasoning, was_cancelled, usage)) => Some(AssistantStreamResult {
            tool_calls,
            text,
            reasoning,
            was_cancelled,
            usage,
        }),
        Err(e) => {
            let repo_ref = &**repo;
            let _ = repo_ref.cancel_round(round_id, Some(&e.to_string())).await;
            fail_ralph_turn_if_needed(
                mode_lifecycle_context!(
                    turn_spawn_context.is_ralph_mode,
                    sessions,
                    repo,
                    project_root_for_ralph,
                    session_id,
                    turn_spawn_context.ralph_env.clone(),
                    Some(round_id),
                ),
                ralph_failure_phase,
                &e.to_string(),
            )
            .await;
            fail_autopilot_turn_if_needed(
                mode_lifecycle_context!(
                    turn_spawn_context.is_autopilot_mode,
                    sessions,
                    repo,
                    project_root_for_autopilot,
                    session_id,
                    turn_spawn_context.autopilot_env.clone(),
                    Some(round_id),
                ),
                autopilot_failure_phase,
                &e.to_string(),
            )
            .await;
            fail_team_turn_if_needed(
                turn_spawn_context.is_team_mode,
                repo,
                project_root_for_team,
                session_id,
                &e.to_string(),
                Some(round_id),
            )
            .await;

            let _ = app.emit(
                &format!("chat-stream-{}", message_id),
                &StreamOutputItem::Error {
                    message: e.to_string(),
                    code: None,
                },
            );
            None
        }
    }
}

struct StreamCancelledTurnInput<'a> {
    app: &'a AppHandle,
    message_id: &'a str,
    round_id: &'a str,
    session_id: &'a str,
    sessions: &'a Arc<RwLock<HashMap<String, SessionRuntimeState>>>,
    repo: &'a Arc<crate::domain::persistence::SessionRepository>,
    turn_spawn_context: &'a TurnSpawnContext,
    project_root_for_ralph: &'a Path,
    project_root_for_autopilot: &'a Path,
    ralph_phase: crate::domain::ralph_state::RalphPhase,
    autopilot_phase: crate::domain::autopilot_state::AutopilotPhase,
}

async fn handle_stream_cancelled_turn(input: StreamCancelledTurnInput<'_>) {
    let StreamCancelledTurnInput {
        app,
        message_id,
        round_id,
        session_id,
        sessions,
        repo,
        turn_spawn_context,
        project_root_for_ralph,
        project_root_for_autopilot,
        ralph_phase,
        autopilot_phase,
    } = input;

    persist_session_tool_state(sessions, repo, session_id).await;
    update_ralph_phase_if_needed(
        mode_lifecycle_context!(
            turn_spawn_context.is_ralph_mode,
            sessions,
            repo,
            project_root_for_ralph,
            session_id,
            turn_spawn_context.ralph_env.clone(),
            Some(round_id),
        ),
        ralph_phase,
    )
    .await;
    update_autopilot_phase_if_needed(
        mode_lifecycle_context!(
            turn_spawn_context.is_autopilot_mode,
            sessions,
            repo,
            project_root_for_autopilot,
            session_id,
            turn_spawn_context.autopilot_env.clone(),
            Some(round_id),
        ),
        autopilot_phase,
    )
    .await;
    let _ = app.emit(
        &format!("chat-stream-{}", message_id),
        &StreamOutputItem::Text("\n\n[Cancelled]".to_string()),
    );
    let _ = app.emit(
        &format!("chat-stream-{}", message_id),
        &StreamOutputItem::Cancelled,
    );
}

struct RuntimeToolGateInput<'a> {
    app: &'a AppHandle,
    client: &'a dyn LlmClient,
    repo: &'a Arc<crate::domain::persistence::SessionRepository>,
    sessions: &'a Arc<RwLock<HashMap<String, SessionRuntimeState>>>,
    session_id: &'a str,
    round_id: &'a str,
    message_id: &'a str,
    request_text: &'a str,
    assistant_text: &'a str,
    assistant_reasoning: &'a str,
    tool_calls: &'a [CompletedToolCall],
    constraint_harness: &'a RuntimeConstraintHarness,
    constraint_state: &'a mut RuntimeConstraintState,
    tool_results_dir: &'a Path,
    ask_user_waiters: &'a Arc<Mutex<HashMap<String, AskUserWaiter>>>,
    cancel_flag: &'a Arc<RwLock<bool>>,
    preflight_skip_turn_summary: bool,
    turn_token_usage: &'a Option<crate::llm::TokenUsage>,
    provider_name: String,
}

async fn handle_runtime_tool_gate(input: RuntimeToolGateInput<'_>) -> bool {
    let RuntimeToolGateInput {
        app,
        client,
        repo,
        sessions,
        session_id,
        round_id,
        message_id,
        request_text,
        assistant_text,
        assistant_reasoning,
        tool_calls,
        constraint_harness,
        constraint_state,
        tool_results_dir,
        ask_user_waiters,
        cancel_flag,
        preflight_skip_turn_summary,
        turn_token_usage,
        provider_name,
    } = input;

    let pending_tool_names: Vec<String> =
        tool_calls.iter().map(|(_, name, _)| name.clone()).collect();
    if let Some(block) = constraint_harness.tool_gate(
        &ToolConstraintContext {
            request_text,
            assistant_text,
            pending_tool_names: &pending_tool_names,
            is_subagent: false,
        },
        constraint_state,
    ) {
        constraint_state.mark_clarification_requested();
        emit_runtime_constraint_metadata(
            app,
            repo,
            session_id,
            round_id,
            message_id,
            "runtime_constraints.gate",
            serde_json::json!({
                "id": block.id,
                "assistant_response": block.assistant_response,
            }),
        )
        .await;
        handle_runtime_constraint_block_main(RuntimeConstraintBlockRequest {
            app,
            client,
            repo: repo.clone(),
            sessions,
            session_id,
            round_id,
            message_id,
            user_message: request_text,
            assistant_text,
            assistant_reasoning,
            tool_calls,
            block: &block,
            tool_results_dir,
            ask_user_waiters: ask_user_waiters.clone(),
            cancel_flag: cancel_flag.clone(),
            preflight_skip_turn_summary,
            turn_token_usage,
            provider_name: &provider_name,
            persist_original_assistant: true,
        })
        .await;
        return true;
    }

    false
}

struct RuntimePostResponseBlockInput<'a> {
    app: &'a AppHandle,
    client: &'a dyn LlmClient,
    repo: &'a Arc<crate::domain::persistence::SessionRepository>,
    sessions: &'a Arc<RwLock<HashMap<String, SessionRuntimeState>>>,
    session_id: &'a str,
    round_id: &'a str,
    message_id: &'a str,
    request_text: &'a str,
    assistant_text: &'a str,
    assistant_reasoning: &'a str,
    tool_calls: &'a [CompletedToolCall],
    constraint_harness: &'a RuntimeConstraintHarness,
    constraint_state: &'a mut RuntimeConstraintState,
    tool_results_dir: &'a Path,
    ask_user_waiters: &'a Arc<Mutex<HashMap<String, AskUserWaiter>>>,
    cancel_flag: &'a Arc<RwLock<bool>>,
    preflight_skip_turn_summary: bool,
    turn_token_usage: &'a Option<crate::llm::TokenUsage>,
    provider_name: String,
}

async fn handle_runtime_post_response_block(input: RuntimePostResponseBlockInput<'_>) -> bool {
    let RuntimePostResponseBlockInput {
        app,
        client,
        repo,
        sessions,
        session_id,
        round_id,
        message_id,
        request_text,
        assistant_text,
        assistant_reasoning,
        tool_calls,
        constraint_harness,
        constraint_state,
        tool_results_dir,
        ask_user_waiters,
        cancel_flag,
        preflight_skip_turn_summary,
        turn_token_usage,
        provider_name,
    } = input;

    let no_pending_tool_names: Vec<String> = Vec::new();
    if let Some(block) = constraint_harness.post_response_block(
        &crate::domain::runtime_constraints::PostResponseConstraintContext {
            request_text,
            assistant_text,
            pending_tool_names: &no_pending_tool_names,
            is_subagent: false,
        },
        constraint_state,
    ) {
        constraint_state.mark_clarification_requested();
        emit_runtime_constraint_metadata(
            app,
            repo,
            session_id,
            round_id,
            message_id,
            "runtime_constraints.post_response_block",
            serde_json::json!({
                "id": block.id,
                "assistant_response": block.assistant_response,
            }),
        )
        .await;
        handle_runtime_constraint_block_main(RuntimeConstraintBlockRequest {
            app,
            client,
            repo: repo.clone(),
            sessions,
            session_id,
            round_id,
            message_id,
            user_message: request_text,
            assistant_text,
            assistant_reasoning,
            tool_calls,
            block: &block,
            tool_results_dir,
            ask_user_waiters: ask_user_waiters.clone(),
            cancel_flag: cancel_flag.clone(),
            preflight_skip_turn_summary,
            turn_token_usage,
            provider_name: &provider_name,
            persist_original_assistant: false,
        })
        .await;
        return true;
    }

    false
}

struct PersistAssistantMessageInput<'a> {
    repo: &'a Arc<crate::domain::persistence::SessionRepository>,
    sessions: &'a Arc<RwLock<HashMap<String, SessionRuntimeState>>>,
    session_id: &'a str,
    content: &'a str,
    tool_calls: &'a [CompletedToolCall],
    reasoning: &'a str,
    warn_context: &'a str,
}

async fn persist_assistant_message(input: PersistAssistantMessageInput<'_>) -> String {
    let PersistAssistantMessageInput {
        repo,
        sessions,
        session_id,
        content,
        tool_calls,
        reasoning,
        warn_context,
    } = input;

    let assistant_id = uuid::Uuid::new_v4().to_string();
    let tool_calls_json = tool_calls_json_opt(tool_calls);
    let reasoning_save = (!reasoning.is_empty()).then_some(reasoning);
    {
        let repo_ref = &**repo;
        if let Err(e) = repo_ref
            .save_message(NewMessageRecord {
                id: &assistant_id,
                session_id,
                role: "assistant",
                content,
                tool_calls: tool_calls_json.as_deref(),
                tool_call_id: None,
                token_usage_json: None,
                reasoning_content: reasoning_save,
                follow_up_suggestions_json: None,
                turn_summary: None,
            })
            .await
        {
            tracing::warn!("{}: {}", warn_context, e);
        }
    }

    {
        let mut sessions = sessions.write().await;
        if let Some(runtime) = sessions.get_mut(session_id) {
            let tc = completed_to_tool_calls(tool_calls);
            let rc = (!reasoning.is_empty()).then(|| reasoning.to_string());
            runtime
                .session
                .add_assistant_message_with_tools(content, tc, rc);
        }
    }

    assistant_id
}

struct PostResponseRetryOutcome {
    assistant_id: String,
    assistant_text: String,
}

struct PostResponseRetryInput<'a> {
    app: &'a AppHandle,
    client: &'a dyn LlmClient,
    repo: &'a Arc<crate::domain::persistence::SessionRepository>,
    sessions: &'a Arc<RwLock<HashMap<String, SessionRuntimeState>>>,
    session_id: &'a str,
    round_id: &'a str,
    message_id: &'a str,
    request_text: &'a str,
    assistant_text: &'a str,
    constraint_harness: &'a RuntimeConstraintHarness,
    constraint_state: &'a mut RuntimeConstraintState,
    request_image_attachments: &'a [RequestImageAttachment],
    pending_tools: &'a Arc<Mutex<HashMap<String, PendingToolCall>>>,
    cancel_flag: &'a Arc<RwLock<bool>>,
    turn_token_usage: &'a mut Option<crate::llm::TokenUsage>,
}

async fn run_post_response_retry_if_needed(
    input: PostResponseRetryInput<'_>,
) -> Option<PostResponseRetryOutcome> {
    let PostResponseRetryInput {
        app,
        client,
        repo,
        sessions,
        session_id,
        round_id,
        message_id,
        request_text,
        assistant_text,
        constraint_harness,
        constraint_state,
        request_image_attachments,
        pending_tools,
        cancel_flag,
        turn_token_usage,
    } = input;

    let no_pending_tool_names: Vec<String> = Vec::new();
    if let Some(action) = constraint_harness.post_response_action(
        &crate::domain::runtime_constraints::PostResponseConstraintContext {
            request_text,
            assistant_text,
            pending_tool_names: &no_pending_tool_names,
            is_subagent: false,
        },
        constraint_state,
    ) {
        constraint_state.mark_post_action_attempted(action.id);
        emit_runtime_constraint_metadata(
            app,
            repo,
            session_id,
            round_id,
            message_id,
            "runtime_constraint_retry",
            serde_json::json!({ "id": action.id }),
        )
        .await;
        let updated_messages = {
            let sessions = sessions.read().await;
            sessions
                .get(session_id)
                .map(|r| SessionCodec::to_api_messages(&r.session.messages))
                .unwrap_or_default()
        };
        let mut updated_llm_messages = api_messages_to_llm(&updated_messages);
        append_image_attachments_to_latest_user_message(
            &mut updated_llm_messages,
            request_image_attachments,
        );
        match run_post_response_retry_text_only(PostResponseRetryRequest {
            client,
            app,
            message_id,
            round_id,
            base_messages: &updated_llm_messages,
            instruction: &action.instruction,
            pending_tools,
            cancel_flag,
            repo: repo.clone(),
        })
        .await
        {
            Ok((retry_text, retry_reasoning, usage_retry)) if !retry_text.trim().is_empty() => {
                merge_turn_token_usage(turn_token_usage, usage_retry);
                let retry_id = persist_assistant_message(PersistAssistantMessageInput {
                    repo,
                    sessions,
                    session_id,
                    content: &retry_text,
                    tool_calls: &[],
                    reasoning: &retry_reasoning,
                    warn_context: "Failed to save runtime retry assistant message",
                })
                .await;
                return Some(PostResponseRetryOutcome {
                    assistant_id: retry_id,
                    assistant_text: retry_text,
                });
            }
            Ok(_) => {}
            Err(e) => {
                tracing::warn!("Runtime post-response retry failed: {}", e);
            }
        }
    }

    None
}

struct FinalAssistantTurnInput<'a> {
    app: &'a AppHandle,
    client: &'a dyn LlmClient,
    repo: &'a Arc<crate::domain::persistence::SessionRepository>,
    sessions: &'a Arc<RwLock<HashMap<String, SessionRuntimeState>>>,
    session_id: &'a str,
    round_id: &'a str,
    message_id: &'a str,
    assistant_message_id: &'a str,
    final_reply: &'a str,
    user_request: &'a str,
    turn_spawn_context: &'a TurnSpawnContext,
    project_root_for_ralph: &'a Path,
    project_root_for_autopilot: &'a Path,
    project_root_for_team: &'a Path,
    preflight_skip_turn_summary: bool,
    turn_token_usage: &'a Option<crate::llm::TokenUsage>,
    turn_had_tool_errors: bool,
    buffer_responses: bool,
}

async fn complete_final_assistant_turn(input: FinalAssistantTurnInput<'_>) {
    let FinalAssistantTurnInput {
        app,
        client,
        repo,
        sessions,
        session_id,
        round_id,
        message_id,
        assistant_message_id,
        final_reply,
        user_request,
        turn_spawn_context,
        project_root_for_ralph,
        project_root_for_autopilot,
        project_root_for_team,
        preflight_skip_turn_summary,
        turn_token_usage,
        turn_had_tool_errors,
        buffer_responses,
    } = input;

    if buffer_responses {
        emit_buffered_assistant_text(app, message_id, final_reply);
        emit_runtime_constraint_metadata(
            app,
            repo,
            session_id,
            round_id,
            message_id,
            "runtime_constraints.commit",
            serde_json::json!({
                "mode": "buffered",
                "phase": "final",
            }),
        )
        .await;
    }

    persist_session_tool_state(sessions, repo, session_id).await;
    update_ralph_phase_if_needed(
        mode_lifecycle_context!(
            turn_spawn_context.is_ralph_mode,
            sessions,
            repo,
            project_root_for_ralph,
            session_id,
            turn_spawn_context.ralph_env.clone(),
            Some(round_id),
        ),
        crate::domain::ralph_state::RalphPhase::Verifying,
    )
    .await;
    update_autopilot_phase_if_needed(
        mode_lifecycle_context!(
            turn_spawn_context.is_autopilot_mode,
            sessions,
            repo,
            project_root_for_autopilot,
            session_id,
            turn_spawn_context.autopilot_env.clone(),
            Some(round_id),
        ),
        crate::domain::autopilot_state::AutopilotPhase::Validation,
    )
    .await;

    {
        let repo_ref = &**repo;
        if let Err(e) = repo_ref
            .complete_round(round_id, Some(assistant_message_id))
            .await
        {
            tracing::warn!("Failed to complete round: {}", e);
        }
    } // repo guard dropped before emit_post_turn_meta_then_complete to avoid deadlock
    persist_and_emit_turn_token_usage(
        app,
        repo,
        assistant_message_id,
        message_id,
        turn_token_usage,
        &turn_spawn_context.llm_config.provider.to_string(),
    )
    .await;
    complete_ralph_turn_if_needed(
        turn_spawn_context.is_ralph_mode,
        sessions,
        repo,
        project_root_for_ralph,
        session_id,
        Some(round_id),
    )
    .await;
    complete_autopilot_turn_if_needed(
        turn_spawn_context.is_autopilot_mode,
        sessions,
        repo,
        project_root_for_autopilot,
        session_id,
        Some(round_id),
    )
    .await;
    complete_team_turn_if_needed(
        turn_spawn_context.is_team_mode,
        repo,
        project_root_for_team,
        session_id,
        Some(round_id),
    )
    .await;
    let should_update_memory = should_update_memory_after_turn(final_reply, turn_had_tool_errors);
    if should_update_memory {
        spawn_memory_sync(MemorySyncRequest {
            app,
            sessions,
            repo,
            session_id,
            client,
            user_message: user_request,
            assistant_reply: final_reply,
            allow_long_term_promotion: true,
        });
    }
    emit_post_turn_meta_then_complete(PostTurnCompletionRequest {
        app,
        session_id,
        stream_message_id: message_id,
        assistant_message_id,
        client,
        final_reply,
        skip_summary: preflight_skip_turn_summary,
        skip_follow_up: false,
        user_request,
        suggestions_reply: final_reply,
        repo: repo.clone(),
    })
    .await;
    if should_update_memory {
        spawn_chat_indexing(app, sessions, repo, session_id);
    }
}

struct ToolRoundExecutionInput<'a> {
    app: &'a AppHandle,
    message_id: &'a str,
    session_id: &'a str,
    tool_results_dir: &'a Path,
    pending_tool_calls: &'a [CompletedToolCall],
    sessions: &'a Arc<RwLock<HashMap<String, SessionRuntimeState>>>,
    repo: &'a Arc<crate::domain::persistence::SessionRepository>,
    agent_runtime: &'a AgentLlmRuntime,
    constraint_state: &'a mut RuntimeConstraintState,
    skill_task_context: &'a str,
    web_search_api_keys: &'a crate::domain::tools::WebSearchApiKeys,
    turn_spawn_context: &'a TurnSpawnContext,
    computer_use_mode: &'a crate::domain::computer_use::ComputerUseMode,
    browser_use_mode: &'a crate::domain::browser_operator::BrowserUseMode,
}

async fn execute_and_persist_tool_round(input: ToolRoundExecutionInput<'_>) -> Vec<ToolResultRow> {
    let ToolRoundExecutionInput {
        app,
        message_id,
        session_id,
        tool_results_dir,
        pending_tool_calls,
        sessions,
        repo,
        agent_runtime,
        constraint_state,
        skill_task_context,
        web_search_api_keys,
        turn_spawn_context,
        computer_use_mode,
        browser_use_mode,
    } = input;

    let (project_root, todos_for_tools, agent_tasks_for_tools, artifact_registry_for_tools) = {
        let sessions = sessions.read().await;
        let project_root = sessions
            .get(session_id)
            .map(|r| resolve_session_project_root(&r.session.project_path))
            .unwrap_or_else(|| {
                std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
            });
        let todos = sessions.get(session_id).map(|r| r.todos.clone());
        let agent_tasks = sessions.get(session_id).map(|r| r.agent_tasks.clone());
        let artifact_registry = sessions
            .get(session_id)
            .map(|r| std::sync::Arc::new(r.artifact_registry.clone()));
        (project_root, todos, agent_tasks, artifact_registry)
    };

    constraint_state.record_tool_names(
        pending_tool_calls
            .iter()
            .map(|(_, tool_name, _)| tool_name.as_str()),
    );

    let mut tool_results = execute_tool_calls(ToolExecutionRequest {
        tool_calls: pending_tool_calls,
        app,
        message_id,
        session_id,
        tool_results_dir,
        project_root: &project_root,
        session_todos: todos_for_tools,
        session_agent_tasks: agent_tasks_for_tools,
        agent_runtime: Some(agent_runtime),
        subagent_depth: 0,
        skill_task_context: Some(skill_task_context),
        web_search_api_keys: web_search_api_keys.clone(),
        skill_cache: turn_spawn_context.skill_cache.clone(),
        execution_environment: agent_runtime.execution_environment.clone(),
        ssh_server: agent_runtime.ssh_server.clone(),
        sandbox_backend: agent_runtime.sandbox_backend.clone(),
        local_venv_type: agent_runtime.local_venv_type.clone(),
        local_venv_name: agent_runtime.local_venv_name.clone(),
        env_store: agent_runtime.env_store.clone(),
        computer_use_enabled: computer_use_mode.is_enabled(),
        browser_use_enabled: browser_use_mode.is_enabled(),
        artifact_registry: artifact_registry_for_tools,
    })
    .await;

    // Preserve the provider-required assistant(tool_calls) -> tool(...) sequence even
    // when a cancellation-aware tool exits without returning a row for every call.
    let returned_tool_ids: HashSet<String> = tool_results
        .iter()
        .map(|(tool_use_id, _, _)| tool_use_id.clone())
        .collect();
    for (tool_use_id, tool_name, _) in pending_tool_calls {
        if !returned_tool_ids.contains(tool_use_id) {
            tool_results.push((
                tool_use_id.clone(),
                format!("Tool `{tool_name}` was cancelled before it returned a result."),
                true,
            ));
        }
    }

    {
        let repo_ref = &**repo;
        // Write all tool results in a single transaction (one fsync instead of N).
        let batch: Vec<(String, String, bool, Option<String>)> = tool_results
            .iter()
            .map(|(id, out, is_error)| (id.clone(), out.clone(), *is_error, None))
            .collect();
        if let Err(e) = repo_ref.save_tool_results_batch(session_id, &batch).await {
            tracing::warn!("Failed to save tool results batch: {}", e);
        }
    }

    {
        let mut sessions = sessions.write().await;
        if let Some(runtime) = sessions.get_mut(session_id) {
            for (tool_use_id, output, is_error) in &tool_results {
                runtime
                    .session
                    .add_tool_result_with_error(tool_use_id, output, Some(*is_error));
            }
        }
    }

    persist_session_tool_state(sessions, repo, session_id).await;
    tool_results
}

struct ToolRoundCancellationInput<'a> {
    app: &'a AppHandle,
    message_id: &'a str,
    round_id: &'a str,
    repo: &'a Arc<crate::domain::persistence::SessionRepository>,
    cancel_flag: &'a Arc<RwLock<bool>>,
    round_cancel: &'a tokio_util::sync::CancellationToken,
}

async fn handle_tool_round_cancelled(input: ToolRoundCancellationInput<'_>) -> bool {
    let ToolRoundCancellationInput {
        app,
        message_id,
        round_id,
        repo,
        cancel_flag,
        round_cancel,
    } = input;

    if *cancel_flag.read().await || round_cancel.is_cancelled() {
        let _ = repo.cancel_round(round_id, Some("User cancelled")).await;
        let _ = app.emit(
            &format!("chat-stream-{}", message_id),
            &StreamOutputItem::Text("\n\n[Cancelled]".to_string()),
        );
        let _ = app.emit(
            &format!("chat-stream-{}", message_id),
            &StreamOutputItem::Cancelled,
        );
        return true;
    }

    false
}

struct AutopilotQaLimitInput<'a> {
    app: &'a AppHandle,
    client: &'a dyn LlmClient,
    repo: &'a Arc<crate::domain::persistence::SessionRepository>,
    sessions: &'a Arc<RwLock<HashMap<String, SessionRuntimeState>>>,
    session_id: &'a str,
    round_id: &'a str,
    message_id: &'a str,
    request_text: &'a str,
    turn_spawn_context: &'a TurnSpawnContext,
    project_root_for_autopilot: &'a Path,
    preflight_skip_turn_summary: bool,
    turn_token_usage: &'a Option<crate::llm::TokenUsage>,
    state: crate::domain::autopilot_state::AutopilotState,
}

async fn handle_autopilot_qa_limit_if_needed(input: AutopilotQaLimitInput<'_>) -> bool {
    let AutopilotQaLimitInput {
        app,
        client,
        repo,
        sessions,
        session_id,
        round_id,
        message_id,
        request_text,
        turn_spawn_context,
        project_root_for_autopilot,
        preflight_skip_turn_summary,
        turn_token_usage,
        state,
    } = input;

    if !state.qa_limit_reached() {
        return false;
    }

    let stop_text = format!(
        "Autopilot stopped after exceeding max argumentation cycles ({}/{}). Last known goal: {}",
        state.qa_cycles, state.max_qa_cycles, state.goal
    );
    let stop_msg_id = uuid::Uuid::new_v4().to_string();
    let reasoning_save = None::<&str>;
    if let Err(e) = repo
        .save_message(NewMessageRecord {
            id: &stop_msg_id,
            session_id,
            role: "assistant",
            content: &stop_text,
            tool_calls: None,
            tool_call_id: None,
            token_usage_json: None,
            reasoning_content: reasoning_save,
            follow_up_suggestions_json: None,
            turn_summary: None,
        })
        .await
    {
        tracing::warn!(
            target: "omiga::autopilot",
            "Failed to save autopilot argumentation limit stop message: {}",
            e
        );
    }
    {
        let mut sessions = sessions.write().await;
        if let Some(runtime) = sessions.get_mut(session_id) {
            runtime
                .session
                .add_assistant_message_with_tools(&stop_text, None, None);
        }
    }
    persist_session_tool_state(sessions, repo, session_id).await;
    fail_autopilot_turn_if_needed(
        mode_lifecycle_context!(
            true,
            sessions,
            repo,
            project_root_for_autopilot,
            session_id,
            turn_spawn_context.autopilot_env.clone(),
            Some(round_id),
        ),
        crate::domain::autopilot_state::AutopilotPhase::Qa,
        &stop_text,
    )
    .await;
    {
        let repo_ref = &**repo;
        let _ = repo_ref.complete_round(round_id, Some(&stop_msg_id)).await;
    }
    persist_and_emit_turn_token_usage(
        app,
        repo,
        &stop_msg_id,
        message_id,
        turn_token_usage,
        &turn_spawn_context.llm_config.provider.to_string(),
    )
    .await;
    emit_post_turn_meta_then_complete(PostTurnCompletionRequest {
        app,
        session_id,
        stream_message_id: message_id,
        assistant_message_id: &stop_msg_id,
        client,
        final_reply: &stop_text,
        skip_summary: preflight_skip_turn_summary,
        skip_follow_up: false,
        user_request: request_text,
        suggestions_reply: &stop_text,
        repo: repo.clone(),
    })
    .await;
    true
}

struct ToolLoopCompactionInput<'a> {
    app: &'a AppHandle,
    sessions: &'a Arc<RwLock<HashMap<String, SessionRuntimeState>>>,
    repo: &'a Arc<crate::domain::persistence::SessionRepository>,
    session_id: &'a str,
    llm_config: &'a LlmConfig,
    tools_enabled: bool,
}

async fn compact_tool_loop_history(input: ToolLoopCompactionInput<'_>) {
    let ToolLoopCompactionInput {
        app,
        sessions,
        repo,
        session_id,
        llm_config,
        tools_enabled,
    } = input;

    let mut sessions = sessions.write().await;
    if let Some(runtime) = sessions.get_mut(session_id) {
        let repo_ref = &**repo;
        let compaction_context = crate::domain::auto_compact::CompactionContext {
            last_turn_input_tokens: last_turn_input_tokens_for_compaction(
                &runtime.session.messages,
            ),
        };
        if let Some(removed_messages) =
            crate::domain::auto_compact::preview_removed_messages_for_compaction(
                &runtime.session.messages,
                llm_config,
                tools_enabled,
                compaction_context,
            )
        {
            let should_prepare =
                match crate::domain::memory::working_memory::should_prepare_for_auto_compact(
                    repo, session_id,
                )
                .await
                {
                    Ok(value) => value,
                    Err(e) => {
                        tracing::warn!(
                            target: "omiga::working_memory",
                            "checking tool-loop pre-compact preparation throttle failed: {}",
                            e
                        );
                        true
                    }
                };
            if should_prepare {
                let op_id = format!("memory-precompact-{}", uuid::Uuid::new_v4());
                emit_activity_operation(
                    app,
                    session_id,
                    &op_id,
                    "压缩前摘要",
                    "running",
                    Some(format!(
                        "准备提炼 {} 条即将压缩的消息",
                        removed_messages.len()
                    )),
                );
                match tokio::time::timeout(
                    std::time::Duration::from_secs(3),
                    crate::domain::memory::working_memory::prepare_for_auto_compact(
                        repo,
                        session_id,
                        &removed_messages,
                    ),
                )
                .await
                {
                    Ok(Err(e)) => {
                        tracing::warn!(
                            target: "omiga::working_memory",
                            "tool-loop pre-compact summary failed: {}",
                            e
                        );
                        emit_activity_operation(
                            app,
                            session_id,
                            &op_id,
                            "压缩前摘要",
                            "error",
                            Some(e.to_string()),
                        );
                    }
                    Ok(Ok(compact_state)) => {
                        emit_activity_operation(
                            app,
                            session_id,
                            &op_id,
                            "压缩前摘要",
                            "done",
                            Some("已提炼即将被压缩的上下文".to_string()),
                        );
                        let _ = compact_state;
                        // This block already holds the sessions write lock. Keep
                        // pre-compact memory as scratchpad only; archiving/promoting
                        // while the tool loop is still active polluted long-term memory
                        // when a later tool-limit or failure stopped the turn.
                    }
                    Err(_) => {
                        tracing::warn!(
                            target: "omiga::working_memory",
                            "tool-loop pre-compact summary timed out; continuing without blocking chat"
                        );
                        emit_activity_operation(
                            app,
                            session_id,
                            &op_id,
                            "压缩前摘要",
                            "error",
                            Some("prepare_for_auto_compact timed out".to_string()),
                        );
                    }
                }
            }
        }
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            crate::domain::auto_compact::compact_session_and_persist(
                repo_ref,
                session_id,
                &mut runtime.session,
                llm_config,
                tools_enabled,
                compaction_context,
                "",
            ),
        )
        .await
        {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => {
                tracing::warn!(
                    target: "omiga::auto_compact",
                    "tool-loop auto-compact failed: {}",
                    e
                );
            }
            Err(_) => {
                tracing::warn!(
                    target: "omiga::auto_compact",
                    "tool-loop auto-compact timed out; continuing with current transcript"
                );
            }
        }
    }
}

struct FollowupMessagesInput<'a> {
    app: &'a AppHandle,
    repo: &'a Arc<crate::domain::persistence::SessionRepository>,
    sessions: &'a Arc<RwLock<HashMap<String, SessionRuntimeState>>>,
    session_id: &'a str,
    round_id: &'a str,
    message_id: &'a str,
    request_image_attachments: &'a [RequestImageAttachment],
    constraint_harness: &'a RuntimeConstraintHarness,
    constraint_state: &'a mut RuntimeConstraintState,
    request_text: &'a str,
    project_root_for_constraints: &'a Path,
    tools_enabled: bool,
}

async fn build_constrained_followup_messages(input: FollowupMessagesInput<'_>) -> Vec<LlmMessage> {
    let FollowupMessagesInput {
        app,
        repo,
        sessions,
        session_id,
        round_id,
        message_id,
        request_image_attachments,
        constraint_harness,
        constraint_state,
        request_text,
        project_root_for_constraints,
        tools_enabled,
    } = input;

    let updated_messages = {
        let sessions = sessions.read().await;
        if let Some(runtime) = sessions.get(session_id) {
            SessionCodec::to_api_messages(&runtime.session.messages)
        } else {
            let repo_ref = &**repo;
            if let Ok(Some(db_session)) = repo_ref.get_session(session_id).await {
                let session = SessionCodec::db_to_domain(db_session);
                SessionCodec::to_api_messages(&session.messages)
            } else {
                vec![]
            }
        }
    };

    let mut updated_llm_messages: Vec<LlmMessage> = api_messages_to_llm(&updated_messages);
    append_image_attachments_to_latest_user_message(
        &mut updated_llm_messages,
        request_image_attachments,
    );
    let (constrained_followup_messages, followup_notices) =
        augment_llm_messages_with_runtime_constraints(
            &updated_llm_messages,
            constraint_harness,
            constraint_state,
            request_text,
            project_root_for_constraints,
            tools_enabled,
            false,
        );
    if !followup_notices.is_empty() {
        emit_runtime_constraint_metadata(
            app,
            repo,
            session_id,
            round_id,
            message_id,
            "runtime_constraints.notices",
            serde_json::json!({ "ids": followup_notices }),
        )
        .await;
    }

    constrained_followup_messages
}

struct ToolRoundLimitInput<'a> {
    app: &'a AppHandle,
    client: &'a dyn LlmClient,
    repo: &'a Arc<crate::domain::persistence::SessionRepository>,
    sessions: &'a Arc<RwLock<HashMap<String, SessionRuntimeState>>>,
    session_id: &'a str,
    round_id: &'a str,
    message_id: &'a str,
    assistant_message_id: &'a str,
    final_reply: &'a str,
    request_text: &'a str,
    turn_spawn_context: &'a TurnSpawnContext,
    project_root_for_ralph: &'a Path,
    project_root_for_autopilot: &'a Path,
    project_root_for_team: &'a Path,
    preflight_skip_turn_summary: bool,
    turn_token_usage: &'a Option<crate::llm::TokenUsage>,
}

async fn complete_tool_round_limit_turn(input: ToolRoundLimitInput<'_>) {
    let ToolRoundLimitInput {
        app,
        client,
        repo,
        sessions,
        session_id,
        round_id,
        message_id,
        assistant_message_id,
        final_reply,
        request_text,
        turn_spawn_context,
        project_root_for_ralph,
        project_root_for_autopilot,
        project_root_for_team,
        preflight_skip_turn_summary,
        turn_token_usage,
    } = input;

    persist_session_tool_state(sessions, repo, session_id).await;
    let max_rounds_error = format!("Exceeded maximum tool rounds ({MAX_TOOL_ROUNDS})");
    fail_ralph_turn_if_needed(
        mode_lifecycle_context!(
            turn_spawn_context.is_ralph_mode,
            sessions,
            repo,
            project_root_for_ralph,
            session_id,
            turn_spawn_context.ralph_env.clone(),
            Some(round_id),
        ),
        crate::domain::ralph_state::RalphPhase::Executing,
        &max_rounds_error,
    )
    .await;
    fail_autopilot_turn_if_needed(
        mode_lifecycle_context!(
            turn_spawn_context.is_autopilot_mode,
            sessions,
            repo,
            project_root_for_autopilot,
            session_id,
            turn_spawn_context.autopilot_env.clone(),
            Some(round_id),
        ),
        crate::domain::autopilot_state::AutopilotPhase::Qa,
        &max_rounds_error,
    )
    .await;
    fail_team_turn_if_needed(
        turn_spawn_context.is_team_mode,
        repo,
        project_root_for_team,
        session_id,
        &max_rounds_error,
        Some(round_id),
    )
    .await;

    let stop_text = tool_round_limit_message(MAX_TOOL_ROUNDS);
    let _ = app.emit(
        &format!("chat-stream-{}", message_id),
        &StreamOutputItem::Text(stop_text.clone()),
    );
    let _ = app.emit(
        &format!("chat-stream-{}", message_id),
        &StreamOutputItem::FollowUpSuggestions(tool_round_limit_follow_ups()),
    );
    {
        let repo_ref = &**repo;
        let _ = repo_ref
            .complete_round(round_id, Some(assistant_message_id))
            .await;
    } // repo guard dropped before emit_post_turn_meta_then_complete to avoid deadlock
    persist_and_emit_turn_token_usage(
        app,
        repo,
        assistant_message_id,
        message_id,
        turn_token_usage,
        &turn_spawn_context.llm_config.provider.to_string(),
    )
    .await;
    let final_reply_with_stop = final_reply.to_string() + &stop_text;
    emit_post_turn_meta_then_complete(PostTurnCompletionRequest {
        app,
        session_id,
        stream_message_id: message_id,
        assistant_message_id,
        client,
        final_reply: &final_reply_with_stop,
        skip_summary: preflight_skip_turn_summary,
        skip_follow_up: true,
        user_request: request_text,
        suggestions_reply: &stop_text,
        repo: repo.clone(),
    })
    .await;
}

struct ScheduledOrchestrationInput<'a> {
    app: &'a AppHandle,
    repo: &'a Arc<crate::domain::persistence::SessionRepository>,
    pending_tools: &'a Arc<Mutex<HashMap<String, PendingToolCall>>>,
    active_orchestrations:
        &'a Arc<Mutex<HashMap<String, HashMap<String, tokio_util::sync::CancellationToken>>>>,
    session_id: &'a str,
    agent_runtime: &'a AgentLlmRuntime,
    turn_spawn_context: TurnSpawnContext,
    keyword_skill_route: Option<crate::domain::routing::SkillRoute>,
}

fn start_scheduled_orchestration_if_needed(input: ScheduledOrchestrationInput<'_>) {
    let ScheduledOrchestrationInput {
        app,
        repo,
        pending_tools,
        active_orchestrations,
        session_id,
        agent_runtime,
        turn_spawn_context,
        keyword_skill_route,
    } = input;

    // After the main LLM turn completes, fire real multi-agent orchestration if the
    // scheduler produced a multi-subtask plan. Each sub-agent runs in its own background
    // session and emits independent stream events; we use a fresh runtime so the
    // sub-agents' cancel token is independent from the parent turn's.
    // Team keyword route always fires orchestration (even 1 subtask) so the worker agent
    // runs with the Architect -> review loop rather than the inline skill SKILL.md path.
    if let Some(sched) = turn_spawn_context.scheduler {
        if turn_spawn_context.is_team_mode || sched.plan.subtasks.len() > 1 {
            // Confirmation gate: when the plan is large and skip_confirmation was not
            // set (team keyword route always skips since the user explicitly requested it),
            // emit the confirmation event and defer execution.
            let needs_confirm = sched.requires_confirmation
                && !turn_spawn_context.is_team_mode
                && !turn_spawn_context.is_explicit_execution_workflow;
            if needs_confirm {
                let pending_plan = sched.plan.clone();
                let pending_plan_id = pending_plan.plan_id.clone();
                let pending_original_request = pending_plan.original_request.clone();
                let pending_task_count = pending_plan.subtasks.len();
                let pending_mode_hint = keyword_skill_route
                    .as_ref()
                    .map(|r| r.skill_name.clone())
                    .unwrap_or_else(|| "schedule".to_string());
                let _ = app.emit(
                    "agent-schedule-confirmation-required",
                    serde_json::json!({
                        "sessionId": session_id.to_string(),
                        "planId": pending_plan_id,
                        "summary": sched.confirmation_message
                            .as_deref()
                            .unwrap_or("此计划需要用户确认后才能执行"),
                        "estimatedMinutes": sched.estimated_duration_secs.div_ceil(60),
                        "agents": sched.selected_agents.clone(),
                        // Send the reviewed plan so confirmation executes exactly this decomposition.
                        "plan": pending_plan,
                        "projectRoot": turn_spawn_context.project_root_str.clone(),
                        "strategy": sched.recommended_strategy,
                        "modeHint": pending_mode_hint.clone(),
                        "originalRequest": {
                            "userRequest": pending_original_request,
                            "projectRoot": turn_spawn_context.project_root_str.clone(),
                            "sessionId": session_id.to_string(),
                            "maxAgents": pending_task_count,
                            "autoDecompose": true,
                            "strategy": serde_json::to_value(
                                sched.recommended_strategy
                            ).unwrap_or(serde_json::Value::Null),
                            "modeHint": pending_mode_hint,
                            "skipConfirmation": true,
                        }
                    }),
                );
            } else {
                let app_for_orch = app.clone();
                let session_id_for_orch = session_id.to_string();
                let sched_plan = sched.plan.clone();
                let original_request = sched_plan.original_request.clone();
                let orch_runtime = AgentLlmRuntime {
                    llm_config: turn_spawn_context.llm_config.clone(),
                    round_id: uuid::Uuid::new_v4().to_string(),
                    cancel_flag: std::sync::Arc::new(tokio::sync::RwLock::new(false)),
                    pending_tools: Arc::clone(pending_tools),
                    repo: Arc::clone(repo),
                    plan_mode_flag: None,
                    allow_nested_agent: env_allow_nested_agent(),
                    round_cancel: tokio_util::sync::CancellationToken::new(),
                    execution_environment: agent_runtime.execution_environment.clone(),
                    ssh_server: agent_runtime.ssh_server.clone(),
                    sandbox_backend: agent_runtime.sandbox_backend.clone(),
                    local_venv_type: agent_runtime.local_venv_type.clone(),
                    local_venv_name: agent_runtime.local_venv_name.clone(),
                    env_store: agent_runtime.env_store.clone(),
                    runtime_constraints_config: agent_runtime.runtime_constraints_config.clone(),
                };
                let active_orchestrations = Arc::clone(active_orchestrations);
                tokio::spawn(async move {
                    use crate::domain::agents::scheduler::{AgentScheduler, SchedulingRequest};

                    // Register cancel token so cancel_agent_schedule can abort this orchestration.
                    let orch_cancel = tokio_util::sync::CancellationToken::new();
                    let orch_id = uuid::Uuid::new_v4().to_string();
                    {
                        let mut map = active_orchestrations.lock().await;
                        map.entry(session_id_for_orch.clone())
                            .or_default()
                            .insert(orch_id.clone(), orch_cancel.clone());
                    }

                    let sched_req = SchedulingRequest::new(original_request)
                        .with_project_root(turn_spawn_context.project_root_str)
                        .with_mode_hint(
                            keyword_skill_route
                                .as_ref()
                                .map(|r| r.skill_name.clone())
                                .unwrap_or_default(),
                        )
                        .with_strategy(turn_spawn_context.strategy);
                    let scheduler = AgentScheduler::new();
                    let orch_result = scheduler
                        .execute_plan_with_runtime(
                            &sched_plan,
                            &sched_req,
                            &app_for_orch,
                            &orch_runtime,
                            &session_id_for_orch,
                            orch_cancel,
                        )
                        .await;

                    // Deregister cancel token.
                    {
                        let mut map = active_orchestrations.lock().await;
                        if let Some(inner) = map.get_mut(&session_id_for_orch) {
                            inner.remove(&orch_id);
                            if inner.is_empty() {
                                map.remove(&session_id_for_orch);
                            }
                        }
                    }

                    match orch_result {
                        Ok(result) => {
                            // Inject summary message and fire agent-schedule-complete event
                            // so the frontend refreshes the conversation history.
                            crate::commands::chat::inject_schedule_summary_message(
                                &app_for_orch,
                                &session_id_for_orch,
                                &sched_req.user_request,
                                &result,
                                &orch_runtime,
                            )
                            .await;
                        }
                        Err(e) => {
                            tracing::error!(
                                target: "omiga::scheduler",
                                "Multi-agent orchestration failed: {}",
                                e
                            );
                        }
                    }
                });
            } // close else { (needs_confirm false path)
        }
    }
}

struct InitialAssistantTurnState {
    pending_tool_calls: Vec<CompletedToolCall>,
    final_reply_for_follow_up: String,
    last_assistant_id: String,
}

enum InitialAssistantTurnOutcome {
    Continue(InitialAssistantTurnState),
    Complete,
}

struct InitialAssistantTurnInput<'a> {
    app: &'a AppHandle,
    client: &'a dyn LlmClient,
    repo: &'a Arc<crate::domain::persistence::SessionRepository>,
    sessions: &'a Arc<RwLock<HashMap<String, SessionRuntimeState>>>,
    session_id: &'a str,
    round_id: &'a str,
    message_id: &'a str,
    request_text: &'a str,
    pending_tool_calls: Vec<CompletedToolCall>,
    assistant_text: String,
    assistant_reasoning: String,
    was_cancelled: bool,
    constraint_harness: &'a RuntimeConstraintHarness,
    constraint_state: &'a mut RuntimeConstraintState,
    tool_results_dir: &'a Path,
    ask_user_waiters: &'a Arc<Mutex<HashMap<String, AskUserWaiter>>>,
    cancel_flag: &'a Arc<RwLock<bool>>,
    preflight_skip_turn_summary: bool,
    turn_token_usage: &'a mut Option<crate::llm::TokenUsage>,
    provider_name: String,
    request_image_attachments: &'a [RequestImageAttachment],
    pending_tools: &'a Arc<Mutex<HashMap<String, PendingToolCall>>>,
    turn_spawn_context: &'a TurnSpawnContext,
    project_root_for_ralph: &'a Path,
    project_root_for_autopilot: &'a Path,
    project_root_for_team: &'a Path,
    buffer_responses: bool,
}

async fn process_initial_assistant_turn(
    input: InitialAssistantTurnInput<'_>,
) -> InitialAssistantTurnOutcome {
    let InitialAssistantTurnInput {
        app,
        client,
        repo,
        sessions,
        session_id,
        round_id,
        message_id,
        request_text,
        pending_tool_calls,
        assistant_text,
        assistant_reasoning,
        was_cancelled,
        constraint_harness,
        constraint_state,
        tool_results_dir,
        ask_user_waiters,
        cancel_flag,
        preflight_skip_turn_summary,
        turn_token_usage,
        provider_name,
        request_image_attachments,
        pending_tools,
        turn_spawn_context,
        project_root_for_ralph,
        project_root_for_autopilot,
        project_root_for_team,
        buffer_responses,
    } = input;

    if was_cancelled {
        handle_stream_cancelled_turn(StreamCancelledTurnInput {
            app,
            message_id,
            round_id,
            session_id,
            sessions,
            repo,
            turn_spawn_context,
            project_root_for_ralph,
            project_root_for_autopilot,
            ralph_phase: crate::domain::ralph_state::RalphPhase::Executing,
            autopilot_phase: crate::domain::autopilot_state::AutopilotPhase::Qa,
        })
        .await;
        return InitialAssistantTurnOutcome::Complete;
    }

    let mut final_reply_for_follow_up = assistant_text.clone();

    if handle_runtime_tool_gate(RuntimeToolGateInput {
        app,
        client,
        repo,
        sessions,
        session_id,
        round_id,
        message_id,
        request_text,
        assistant_text: &assistant_text,
        assistant_reasoning: &assistant_reasoning,
        tool_calls: &pending_tool_calls,
        constraint_harness,
        constraint_state,
        tool_results_dir,
        ask_user_waiters,
        cancel_flag,
        preflight_skip_turn_summary,
        turn_token_usage,
        provider_name: provider_name.clone(),
    })
    .await
    {
        return InitialAssistantTurnOutcome::Complete;
    }

    if pending_tool_calls.is_empty()
        && handle_runtime_post_response_block(RuntimePostResponseBlockInput {
            app,
            client,
            repo,
            sessions,
            session_id,
            round_id,
            message_id,
            request_text,
            assistant_text: &assistant_text,
            assistant_reasoning: &assistant_reasoning,
            tool_calls: &pending_tool_calls,
            constraint_harness,
            constraint_state,
            tool_results_dir,
            ask_user_waiters,
            cancel_flag,
            preflight_skip_turn_summary,
            turn_token_usage,
            provider_name,
        })
        .await
    {
        return InitialAssistantTurnOutcome::Complete;
    }

    if buffer_responses && !pending_tool_calls.is_empty() {
        emit_buffered_assistant_text(app, message_id, &assistant_text);
        emit_runtime_constraint_metadata(
            app,
            repo,
            session_id,
            round_id,
            message_id,
            "runtime_constraints.commit",
            serde_json::json!({
                "mode": "buffered",
                "phase": "pre_tool",
            }),
        )
        .await;
    }

    // First assistant turn: persist with tool_calls JSON for reload
    let assistant_msg_id = persist_assistant_message(PersistAssistantMessageInput {
        repo,
        sessions,
        session_id,
        content: &assistant_text,
        tool_calls: &pending_tool_calls,
        reasoning: &assistant_reasoning,
        warn_context: "Failed to save assistant message",
    })
    .await;

    update_ralph_phase_if_needed(
        mode_lifecycle_context!(
            turn_spawn_context.is_ralph_mode,
            sessions,
            repo,
            project_root_for_ralph,
            session_id,
            turn_spawn_context.ralph_env.clone(),
            Some(round_id),
        ),
        if pending_tool_calls.is_empty() {
            crate::domain::ralph_state::RalphPhase::Verifying
        } else {
            crate::domain::ralph_state::RalphPhase::Executing
        },
    )
    .await;
    update_autopilot_phase_if_needed(
        mode_lifecycle_context!(
            turn_spawn_context.is_autopilot_mode,
            sessions,
            repo,
            project_root_for_autopilot,
            session_id,
            turn_spawn_context.autopilot_env.clone(),
            Some(round_id),
        ),
        if pending_tool_calls.is_empty() {
            crate::domain::autopilot_state::AutopilotPhase::Validation
        } else {
            crate::domain::autopilot_state::AutopilotPhase::Implementation
        },
    )
    .await;

    let mut last_assistant_id = assistant_msg_id.clone();

    if pending_tool_calls.is_empty() {
        if let Some(retry) = run_post_response_retry_if_needed(PostResponseRetryInput {
            app,
            client,
            repo,
            sessions,
            session_id,
            round_id,
            message_id,
            request_text,
            assistant_text: &assistant_text,
            constraint_harness,
            constraint_state,
            request_image_attachments,
            pending_tools,
            cancel_flag,
            turn_token_usage,
        })
        .await
        {
            final_reply_for_follow_up = retry.assistant_text;
            last_assistant_id = retry.assistant_id;
        }

        complete_final_assistant_turn(FinalAssistantTurnInput {
            app,
            client,
            repo,
            sessions,
            session_id,
            round_id,
            message_id,
            assistant_message_id: &last_assistant_id,
            final_reply: &final_reply_for_follow_up,
            user_request: request_text,
            turn_spawn_context,
            project_root_for_ralph,
            project_root_for_autopilot,
            project_root_for_team,
            preflight_skip_turn_summary,
            turn_token_usage,
            turn_had_tool_errors: false,
            buffer_responses,
        })
        .await;
        return InitialAssistantTurnOutcome::Complete;
    }

    InitialAssistantTurnOutcome::Continue(InitialAssistantTurnState {
        pending_tool_calls,
        final_reply_for_follow_up,
        last_assistant_id,
    })
}

struct FollowupAssistantTurnInput<'a> {
    app: &'a AppHandle,
    client: &'a dyn LlmClient,
    repo: &'a Arc<crate::domain::persistence::SessionRepository>,
    sessions: &'a Arc<RwLock<HashMap<String, SessionRuntimeState>>>,
    session_id: &'a str,
    round_id: &'a str,
    message_id: &'a str,
    request_text: &'a str,
    next_tools: Vec<CompletedToolCall>,
    next_text: String,
    next_reasoning: String,
    follow_cancelled: bool,
    tool_results: &'a [ToolResultRow],
    constraint_harness: &'a RuntimeConstraintHarness,
    constraint_state: &'a mut RuntimeConstraintState,
    tool_results_dir: &'a Path,
    ask_user_waiters: &'a Arc<Mutex<HashMap<String, AskUserWaiter>>>,
    cancel_flag: &'a Arc<RwLock<bool>>,
    preflight_skip_turn_summary: bool,
    turn_token_usage: &'a mut Option<crate::llm::TokenUsage>,
    provider_name: String,
    request_image_attachments: &'a [RequestImageAttachment],
    pending_tools: &'a Arc<Mutex<HashMap<String, PendingToolCall>>>,
    turn_spawn_context: &'a TurnSpawnContext,
    project_root_for_ralph: &'a Path,
    project_root_for_autopilot: &'a Path,
    project_root_for_team: &'a Path,
    buffer_responses: bool,
    final_reply_for_follow_up: &'a mut String,
    last_assistant_id: &'a mut String,
    pending_tool_calls: &'a mut Vec<CompletedToolCall>,
}

async fn process_followup_assistant_turn(input: FollowupAssistantTurnInput<'_>) -> bool {
    let FollowupAssistantTurnInput {
        app,
        client,
        repo,
        sessions,
        session_id,
        round_id,
        message_id,
        request_text,
        next_tools,
        next_text,
        next_reasoning,
        follow_cancelled,
        tool_results,
        constraint_harness,
        constraint_state,
        tool_results_dir,
        ask_user_waiters,
        cancel_flag,
        preflight_skip_turn_summary,
        turn_token_usage,
        provider_name,
        request_image_attachments,
        pending_tools,
        turn_spawn_context,
        project_root_for_ralph,
        project_root_for_autopilot,
        project_root_for_team,
        buffer_responses,
        final_reply_for_follow_up,
        last_assistant_id,
        pending_tool_calls,
    } = input;

    if handle_runtime_tool_gate(RuntimeToolGateInput {
        app,
        client,
        repo,
        sessions,
        session_id,
        round_id,
        message_id,
        request_text,
        assistant_text: &next_text,
        assistant_reasoning: &next_reasoning,
        tool_calls: &next_tools,
        constraint_harness,
        constraint_state,
        tool_results_dir,
        ask_user_waiters,
        cancel_flag,
        preflight_skip_turn_summary,
        turn_token_usage,
        provider_name: provider_name.clone(),
    })
    .await
    {
        return true;
    }

    if next_tools.is_empty()
        && handle_runtime_post_response_block(RuntimePostResponseBlockInput {
            app,
            client,
            repo,
            sessions,
            session_id,
            round_id,
            message_id,
            request_text,
            assistant_text: &next_text,
            assistant_reasoning: &next_reasoning,
            tool_calls: &next_tools,
            constraint_harness,
            constraint_state,
            tool_results_dir,
            ask_user_waiters,
            cancel_flag,
            preflight_skip_turn_summary,
            turn_token_usage,
            provider_name,
        })
        .await
    {
        return true;
    }

    let synthesized_empty_final_reply =
        next_tools.is_empty() && next_text.trim().is_empty() && !tool_results.is_empty();
    let assistant_text_for_turn = if synthesized_empty_final_reply {
        tool_no_final_answer_message(tool_results)
    } else {
        next_text.clone()
    };
    *final_reply_for_follow_up = assistant_text_for_turn.clone();

    if synthesized_empty_final_reply && !buffer_responses {
        emit_buffered_assistant_text(app, message_id, &assistant_text_for_turn);
    }

    if buffer_responses && !next_tools.is_empty() {
        emit_buffered_assistant_text(app, message_id, &next_text);
        emit_runtime_constraint_metadata(
            app,
            repo,
            session_id,
            round_id,
            message_id,
            "runtime_constraints.commit",
            serde_json::json!({
                "mode": "buffered",
                "phase": "pre_tool",
            }),
        )
        .await;
    }

    if follow_cancelled {
        handle_stream_cancelled_turn(StreamCancelledTurnInput {
            app,
            message_id,
            round_id,
            session_id,
            sessions,
            repo,
            turn_spawn_context,
            project_root_for_ralph,
            project_root_for_autopilot,
            ralph_phase: crate::domain::ralph_state::RalphPhase::Executing,
            autopilot_phase: crate::domain::autopilot_state::AutopilotPhase::Qa,
        })
        .await;
        return true;
    }

    let next_assistant_id = persist_assistant_message(PersistAssistantMessageInput {
        repo,
        sessions,
        session_id,
        content: &assistant_text_for_turn,
        tool_calls: &next_tools,
        reasoning: &next_reasoning,
        warn_context: "Failed to save follow-up assistant",
    })
    .await;

    *last_assistant_id = next_assistant_id.clone();
    *pending_tool_calls = next_tools;

    if pending_tool_calls.is_empty() {
        if let Some(retry) = run_post_response_retry_if_needed(PostResponseRetryInput {
            app,
            client,
            repo,
            sessions,
            session_id,
            round_id,
            message_id,
            request_text,
            assistant_text: &assistant_text_for_turn,
            constraint_harness,
            constraint_state,
            request_image_attachments,
            pending_tools,
            cancel_flag,
            turn_token_usage,
        })
        .await
        {
            *final_reply_for_follow_up = retry.assistant_text;
            *last_assistant_id = retry.assistant_id;
        }

        let turn_had_tool_errors = tool_results.iter().any(|(_, _, is_error)| *is_error);
        complete_final_assistant_turn(FinalAssistantTurnInput {
            app,
            client,
            repo,
            sessions,
            session_id,
            round_id,
            message_id,
            assistant_message_id: last_assistant_id.as_str(),
            final_reply: final_reply_for_follow_up.as_str(),
            user_request: request_text,
            turn_spawn_context,
            project_root_for_ralph,
            project_root_for_autopilot,
            project_root_for_team,
            preflight_skip_turn_summary,
            turn_token_usage,
            turn_had_tool_errors,
            buffer_responses,
        })
        .await;
        return true;
    }

    false
}

async fn run_turn_spawn(task: TurnSpawnTask) {
    let TurnSpawnTask {
        app_clone,
        message_id_clone,
        round_id_clone,
        session_id_clone,
        pending_tools_clone,
        ask_user_waiters_clone,
        active_rounds_clone,
        active_orchestrations_clone,
        sessions_clone,
        repo_clone,
        client,
        compact_log_for_stream,
        project_root,
        cancel_flag,
        round_cancel_spawn,
        resolved_runtime_constraints,
        llm_messages,
        request_text_for_constraints,
        project_root_for_constraints,
        tools,
        request_image_attachments,
        turn_spawn_context,
        project_root_for_ralph,
        project_root_for_autopilot,
        project_root_for_team,
        skill_task_context,
        web_search_api_keys,
        computer_use_mode,
        browser_use_mode,
        preflight_skip_turn_summary,
        keyword_skill_route,
    } = task;
    // Keep this round cancellable for the entire assistant/tool loop. Previously the
    // active-round entry was removed after the first model response, so cancelling during
    // tool execution only updated SQLite; the background task kept writing follow-up
    // assistant messages and could interleave another round's tool result sequence.
    let _active_round_cleanup =
        ActiveRoundCleanup::new(active_rounds_clone.clone(), message_id_clone.clone());

    let TurnRuntimePreparation {
        tool_results_dir,
        agent_runtime,
        constraint_harness,
        mut constraint_state,
        initial_llm_messages,
    } = prepare_turn_runtime(PrepareTurnRuntimeInput {
        app: &app_clone,
        message_id: &message_id_clone,
        round_id: &round_id_clone,
        session_id: &session_id_clone,
        sessions: &sessions_clone,
        repo: &repo_clone,
        compact_log_for_stream,
        project_root: &project_root,
        cancel_flag: &cancel_flag,
        round_cancel: &round_cancel_spawn,
        pending_tools: &pending_tools_clone,
        resolved_runtime_constraints,
        llm_messages: &llm_messages,
        request_text_for_constraints: &request_text_for_constraints,
        project_root_for_constraints: &project_root_for_constraints,
        tools: &tools,
        turn_spawn_context: &turn_spawn_context,
        project_root_for_ralph: &project_root_for_ralph,
        project_root_for_autopilot: &project_root_for_autopilot,
    })
    .await;
    let mut turn_token_usage: Option<crate::llm::TokenUsage> = None;

    // Stream the response with cancellation support
    let initial_stream = match stream_assistant_turn_with_cancel(StreamAssistantTurnInput {
        client: client.as_ref(),
        app: &app_clone,
        message_id: &message_id_clone,
        round_id: &round_id_clone,
        messages: &initial_llm_messages,
        tools: &tools,
        emit_text_chunks: !agent_runtime.runtime_constraints_config.buffer_responses,
        pending_tools: &pending_tools_clone,
        cancel_flag: &cancel_flag,
        repo: &repo_clone,
        sessions: &sessions_clone,
        session_id: &session_id_clone,
        turn_spawn_context: &turn_spawn_context,
        project_root_for_ralph: &project_root_for_ralph,
        project_root_for_autopilot: &project_root_for_autopilot,
        project_root_for_team: &project_root_for_team,
        ralph_failure_phase: crate::domain::ralph_state::RalphPhase::EnvCheck,
        autopilot_failure_phase: crate::domain::autopilot_state::AutopilotPhase::Design,
    })
    .await
    {
        Some(result) => result,
        None => return,
    };
    let AssistantStreamResult {
        tool_calls: pending_tool_calls,
        text: assistant_text,
        reasoning: assistant_reasoning,
        was_cancelled,
        usage: usage_first,
    } = initial_stream;
    merge_turn_token_usage(&mut turn_token_usage, usage_first);

    let initial_state = match process_initial_assistant_turn(InitialAssistantTurnInput {
        app: &app_clone,
        client: client.as_ref(),
        repo: &repo_clone,
        sessions: &sessions_clone,
        session_id: &session_id_clone,
        round_id: &round_id_clone,
        message_id: &message_id_clone,
        request_text: &request_text_for_constraints,
        pending_tool_calls,
        assistant_text,
        assistant_reasoning,
        was_cancelled,
        constraint_harness: &constraint_harness,
        constraint_state: &mut constraint_state,
        tool_results_dir: &tool_results_dir,
        ask_user_waiters: &ask_user_waiters_clone,
        cancel_flag: &cancel_flag,
        preflight_skip_turn_summary,
        turn_token_usage: &mut turn_token_usage,
        provider_name: turn_spawn_context.llm_config.provider.to_string(),
        request_image_attachments: &request_image_attachments,
        pending_tools: &pending_tools_clone,
        turn_spawn_context: &turn_spawn_context,
        project_root_for_ralph: &project_root_for_ralph,
        project_root_for_autopilot: &project_root_for_autopilot,
        project_root_for_team: &project_root_for_team,
        buffer_responses: agent_runtime.runtime_constraints_config.buffer_responses,
    })
    .await
    {
        InitialAssistantTurnOutcome::Continue(state) => state,
        InitialAssistantTurnOutcome::Complete => return,
    };
    let InitialAssistantTurnState {
        mut pending_tool_calls,
        mut final_reply_for_follow_up,
        mut last_assistant_id,
    } = initial_state;

    for _round_idx in 0..MAX_TOOL_ROUNDS {
        let tool_results = execute_and_persist_tool_round(ToolRoundExecutionInput {
            app: &app_clone,
            message_id: &message_id_clone,
            session_id: &session_id_clone,
            tool_results_dir: &tool_results_dir,
            pending_tool_calls: &pending_tool_calls,
            sessions: &sessions_clone,
            repo: &repo_clone,
            agent_runtime: &agent_runtime,
            constraint_state: &mut constraint_state,
            skill_task_context: skill_task_context.as_str(),
            web_search_api_keys: &web_search_api_keys,
            turn_spawn_context: &turn_spawn_context,
            computer_use_mode: &computer_use_mode,
            browser_use_mode: &browser_use_mode,
        })
        .await;
        if handle_tool_round_cancelled(ToolRoundCancellationInput {
            app: &app_clone,
            message_id: &message_id_clone,
            round_id: &round_id_clone,
            repo: &repo_clone,
            cancel_flag: &cancel_flag,
            round_cancel: &round_cancel_spawn,
        })
        .await
        {
            return;
        }
        update_ralph_phase_if_needed(
            mode_lifecycle_context!(
                turn_spawn_context.is_ralph_mode,
                &sessions_clone,
                &repo_clone,
                &project_root_for_ralph,
                &session_id_clone,
                turn_spawn_context.ralph_env.clone(),
                Some(&round_id_clone),
            ),
            crate::domain::ralph_state::RalphPhase::Executing,
        )
        .await;
        let autopilot_state = update_autopilot_phase_if_needed(
            mode_lifecycle_context!(
                turn_spawn_context.is_autopilot_mode,
                &sessions_clone,
                &repo_clone,
                &project_root_for_autopilot,
                &session_id_clone,
                turn_spawn_context.autopilot_env.clone(),
                Some(&round_id_clone),
            ),
            crate::domain::autopilot_state::AutopilotPhase::Qa,
        )
        .await;

        if let Some(state) = autopilot_state {
            if handle_autopilot_qa_limit_if_needed(AutopilotQaLimitInput {
                app: &app_clone,
                client: client.as_ref(),
                repo: &repo_clone,
                sessions: &sessions_clone,
                session_id: &session_id_clone,
                round_id: &round_id_clone,
                message_id: &message_id_clone,
                request_text: &request_text_for_constraints,
                turn_spawn_context: &turn_spawn_context,
                project_root_for_autopilot: &project_root_for_autopilot,
                preflight_skip_turn_summary,
                turn_token_usage: &turn_token_usage,
                state,
            })
            .await
            {
                return;
            }
        }

        // Shrink history before the next model call when tool rounds push toward context limits.
        compact_tool_loop_history(ToolLoopCompactionInput {
            app: &app_clone,
            sessions: &sessions_clone,
            repo: &repo_clone,
            session_id: &session_id_clone,
            llm_config: &turn_spawn_context.llm_config,
            tools_enabled: !tools.is_empty(),
        })
        .await;

        let constrained_followup_messages =
            build_constrained_followup_messages(FollowupMessagesInput {
                app: &app_clone,
                repo: &repo_clone,
                sessions: &sessions_clone,
                session_id: &session_id_clone,
                round_id: &round_id_clone,
                message_id: &message_id_clone,
                request_image_attachments: &request_image_attachments,
                constraint_harness: &constraint_harness,
                constraint_state: &mut constraint_state,
                request_text: &request_text_for_constraints,
                project_root_for_constraints: &project_root_for_constraints,
                tools_enabled: !tools.is_empty(),
            })
            .await;

        let followup_stream = match stream_assistant_turn_with_cancel(StreamAssistantTurnInput {
            client: client.as_ref(),
            app: &app_clone,
            message_id: &message_id_clone,
            round_id: &round_id_clone,
            messages: &constrained_followup_messages,
            tools: &tools,
            emit_text_chunks: !agent_runtime.runtime_constraints_config.buffer_responses,
            pending_tools: &pending_tools_clone,
            cancel_flag: &cancel_flag,
            repo: &repo_clone,
            sessions: &sessions_clone,
            session_id: &session_id_clone,
            turn_spawn_context: &turn_spawn_context,
            project_root_for_ralph: &project_root_for_ralph,
            project_root_for_autopilot: &project_root_for_autopilot,
            project_root_for_team: &project_root_for_team,
            ralph_failure_phase: crate::domain::ralph_state::RalphPhase::Executing,
            autopilot_failure_phase: crate::domain::autopilot_state::AutopilotPhase::Qa,
        })
        .await
        {
            Some(result) => result,
            None => return,
        };
        let AssistantStreamResult {
            tool_calls: next_tools,
            text: next_text,
            reasoning: next_reasoning,
            was_cancelled: follow_cancelled,
            usage: usage_next,
        } = followup_stream;
        merge_turn_token_usage(&mut turn_token_usage, usage_next);

        if process_followup_assistant_turn(FollowupAssistantTurnInput {
            app: &app_clone,
            client: client.as_ref(),
            repo: &repo_clone,
            sessions: &sessions_clone,
            session_id: &session_id_clone,
            round_id: &round_id_clone,
            message_id: &message_id_clone,
            request_text: &request_text_for_constraints,
            next_tools,
            next_text,
            next_reasoning,
            follow_cancelled,
            tool_results: &tool_results,
            constraint_harness: &constraint_harness,
            constraint_state: &mut constraint_state,
            tool_results_dir: &tool_results_dir,
            ask_user_waiters: &ask_user_waiters_clone,
            cancel_flag: &cancel_flag,
            preflight_skip_turn_summary,
            turn_token_usage: &mut turn_token_usage,
            provider_name: turn_spawn_context.llm_config.provider.to_string(),
            request_image_attachments: &request_image_attachments,
            pending_tools: &pending_tools_clone,
            turn_spawn_context: &turn_spawn_context,
            project_root_for_ralph: &project_root_for_ralph,
            project_root_for_autopilot: &project_root_for_autopilot,
            project_root_for_team: &project_root_for_team,
            buffer_responses: agent_runtime.runtime_constraints_config.buffer_responses,
            final_reply_for_follow_up: &mut final_reply_for_follow_up,
            last_assistant_id: &mut last_assistant_id,
            pending_tool_calls: &mut pending_tool_calls,
        })
        .await
        {
            return;
        }
    }

    complete_tool_round_limit_turn(ToolRoundLimitInput {
        app: &app_clone,
        client: client.as_ref(),
        repo: &repo_clone,
        sessions: &sessions_clone,
        session_id: &session_id_clone,
        round_id: &round_id_clone,
        message_id: &message_id_clone,
        assistant_message_id: &last_assistant_id,
        final_reply: &final_reply_for_follow_up,
        request_text: &request_text_for_constraints,
        turn_spawn_context: &turn_spawn_context,
        project_root_for_ralph: &project_root_for_ralph,
        project_root_for_autopilot: &project_root_for_autopilot,
        project_root_for_team: &project_root_for_team,
        preflight_skip_turn_summary,
        turn_token_usage: &turn_token_usage,
    })
    .await;

    start_scheduled_orchestration_if_needed(ScheduledOrchestrationInput {
        app: &app_clone,
        repo: &repo_clone,
        pending_tools: &pending_tools_clone,
        active_orchestrations: &active_orchestrations_clone,
        session_id: &session_id_clone,
        agent_runtime: &agent_runtime,
        turn_spawn_context,
        keyword_skill_route,
    });
}

struct ParsedSendIntake {
    computer_use_mode: crate::domain::computer_use::ComputerUseMode,
    browser_use_mode: crate::domain::browser_operator::BrowserUseMode,
}

enum SendIntakeOutcome {
    Continue(ParsedSendIntake),
    EarlyReturn(MessageResponse),
}

async fn parse_send_intake(request: &SendMessageRequest) -> CommandResult<SendIntakeOutcome> {
    let input_target = match ChatInputTarget::parse(request.input_target.as_deref()) {
        Ok(t) => t,
        Err(msg) => {
            return Err(OmigaError::Chat(ChatError::StreamError(msg.to_string())));
        }
    };
    let computer_use_mode = crate::domain::computer_use::ComputerUseMode::from_request(
        request.computer_use_mode.as_deref(),
    );
    let browser_use_mode = crate::domain::browser_operator::BrowserUseMode::from_request(
        request.browser_use_mode.as_deref(),
    );
    tracing::debug!(
        target: "omiga::computer_use",
        mode = computer_use_mode.as_str(),
        enabled = computer_use_mode.is_enabled(),
        "computer use request gate resolved"
    );
    tracing::debug!(
        target: "omiga::browser_operator",
        mode = browser_use_mode.as_str(),
        enabled = browser_use_mode.is_enabled(),
        "browser operator request gate resolved"
    );

    if let ChatInputTarget::BackgroundAgentFollowup { task_id } = input_target {
        let session_id = request.session_id.clone().ok_or_else(|| {
            OmigaError::Chat(ChatError::StreamError(
                "session_id is required when using input_target bg:<task_id>".to_string(),
            ))
        })?;
        let manager = crate::domain::agents::background::get_background_agent_manager();
        manager
            .enqueue_followup(&task_id, &session_id, request.content.clone())
            .await
            .map_err(|e| OmigaError::Chat(ChatError::StreamError(e.to_string())))?;
        return Ok(SendIntakeOutcome::EarlyReturn(MessageResponse {
            message_id: uuid::Uuid::new_v4().to_string(),
            session_id,
            round_id: uuid::Uuid::new_v4().to_string(),
            input_kind: Some("background_followup_queued".to_string()),
            scheduler_plan: None,
            initial_todos: None,
            user_message_id: None,
        }));
    }

    Ok(SendIntakeOutcome::Continue(ParsedSendIntake {
        computer_use_mode,
        browser_use_mode,
    }))
}

struct KeywordSkillRouting {
    routing_content: String,
    explicit_workflow_command: Option<String>,
    keyword_skill_route: Option<crate::domain::routing::SkillRoute>,
    trace_mode: Option<String>,
}

fn route_keyword_skill(request: &SendMessageRequest) -> KeywordSkillRouting {
    // ===== Keyword-to-Skill routing =====
    // Detect orchestration keywords (ralph, team, literature-search, etc.) and store the route
    // so the skill body can be injected directly into the system prompt later.
    // The user message is left unchanged — the skill instructions arrive via the system prompt,
    // which means the LLM's very first token is already operating under skill guidance (OMX-style
    // auto-invocation) rather than having to decide whether to call the Skill tool.
    let routing_content = request
        .routing_content
        .as_deref()
        .unwrap_or(&request.content);
    let explicit_workflow_command = request
        .workflow_command
        .as_deref()
        .map(str::trim)
        .filter(|cmd| !cmd.is_empty());
    let direct_skill_route = crate::domain::routing::parse_direct_skill_command(routing_content);
    let keyword_skill_route = if request.use_tools {
        match explicit_workflow_command {
            Some("plan") => Some(crate::domain::routing::SkillRoute {
                skill_name: "plan".to_string(),
                args: routing_content.to_string(),
                priority: 12,
            }),
            Some("team") => Some(crate::domain::routing::SkillRoute {
                skill_name: "team".to_string(),
                args: routing_content.to_string(),
                priority: 12,
            }),
            Some("autopilot") => Some(crate::domain::routing::SkillRoute {
                skill_name: "autopilot".to_string(),
                args: routing_content.to_string(),
                priority: 12,
            }),
            _ => direct_skill_route
                .or_else(|| crate::domain::routing::detect_skill_route(routing_content)),
        }
    } else {
        None
    };
    let trace_mode = explicit_workflow_command.map(str::to_string).or_else(|| {
        keyword_skill_route
            .as_ref()
            .map(|route| route.skill_name.clone())
    });
    if let Some(ref route) = keyword_skill_route {
        tracing::info!(
            target: "omiga::routing",
            skill = %route.skill_name,
            priority = route.priority,
            "Keyword routing: will inject '{}' skill body into system prompt",
            route.skill_name
        );
    }

    KeywordSkillRouting {
        routing_content: routing_content.to_string(),
        explicit_workflow_command: explicit_workflow_command.map(str::to_string),
        keyword_skill_route,
        trace_mode,
    }
}

struct BuildTurnSpawnContextInput<'a> {
    llm_config: crate::llm::LlmConfig,
    skill_cache: std::sync::Arc<std::sync::Mutex<crate::domain::skills::SkillCacheMap>>,
    scheduler_result: &'a Option<crate::domain::agents::scheduler::SchedulingResult>,
    is_plan_mode: bool,
    is_explicit_execution_workflow: bool,
    project_root: &'a std::path::Path,
    is_team_mode: bool,
    is_ralph_mode: bool,
    is_autopilot_mode: bool,
    exec_env: &'a str,
    ssh_server: Option<&'a str>,
    local_venv_type: &'a str,
    local_venv_name: &'a str,
}

fn build_turn_spawn_context(input: BuildTurnSpawnContextInput<'_>) -> TurnSpawnContext {
    // scheduler_result stays on the stack for MessageResponse; clone only when a
    // real multi-agent plan was built.
    let scheduler = if input.is_plan_mode || !input.is_explicit_execution_workflow {
        None
    } else {
        input.scheduler_result.clone()
    };
    let project_root_str = input.project_root.to_string_lossy().to_string();
    let ralph_env = ralph_runtime_env_label(
        input.exec_env,
        input.ssh_server,
        input.local_venv_type,
        input.local_venv_name,
    );
    let autopilot_env = ralph_env.clone();
    // LLM planner recommendation takes priority over Auto default.
    let strategy = input
        .scheduler_result
        .as_ref()
        .map(|r| r.recommended_strategy)
        .unwrap_or(if input.is_team_mode {
            crate::domain::agents::scheduler::SchedulingStrategy::Team
        } else {
            crate::domain::agents::scheduler::SchedulingStrategy::Auto
        });

    TurnSpawnContext {
        llm_config: input.llm_config,
        skill_cache: input.skill_cache,
        scheduler,
        project_root_str,
        is_team_mode: input.is_team_mode,
        is_ralph_mode: input.is_ralph_mode,
        is_autopilot_mode: input.is_autopilot_mode,
        is_explicit_execution_workflow: input.is_explicit_execution_workflow,
        ralph_env,
        autopilot_env,
        strategy,
    }
}
