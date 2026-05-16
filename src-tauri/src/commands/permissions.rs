use crate::app_state::OmigaAppState;
use crate::commands::CommandResult;
use crate::domain::permissions::manager::PermissionManager;
use crate::domain::permissions::types::{
    DetectedRisk, PermissionContext, PermissionDecision, PermissionModeInput, PermissionRule,
    RiskLevel,
};
use crate::domain::persistence::NewPermissionAuditEventRecord;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;

// === 响应类型 ===

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct PermissionCheckResponse {
    /// 与 `check_tool` / 批准回传一致，便于前端不依赖 `permission-request` 事件也能带上会话 id
    pub session_id: String,
    pub allowed: bool,
    pub requires_approval: bool,
    pub request_id: Option<String>,
    pub tool_name: String,
    pub risk_level: String,
    pub risk_description: String,
    pub detected_risks: Vec<RiskInfoDto>,
    pub recommendations: Vec<String>,
    pub arguments: Option<Value>, // 返回原始参数，用于批准时的哈希匹配
    pub project_root: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RiskInfoDto {
    pub category: String,
    pub severity: String,
    pub description: String,
    pub mitigation: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RuleListResponse {
    pub rules: Vec<PermissionRule>,
    pub default_mode: String,
}

#[derive(Debug, Serialize)]
pub struct ApprovalStatusResponse {
    pub session_id: String,
    pub approved_tools: Vec<String>,
    pub approved_until: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
pub struct DenialRecordDto {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub tool_name: String,
    pub reason: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionAuditEventDto {
    pub id: String,
    pub session_id: String,
    pub request_id: Option<String>,
    pub project_root: Option<String>,
    pub decision: String,
    pub tool_name: String,
    pub mode: Option<String>,
    pub reason: Option<String>,
    pub arguments: Value,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionAuditEventsResponse {
    pub events: Vec<PermissionAuditEventDto>,
    pub total_count: usize,
    pub approved_count: usize,
    pub denied_count: usize,
}

// === 请求类型 ===

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApproveRequest {
    pub session_id: String,
    pub mode: PermissionModeInput,
    pub tool_name: String,
    #[serde(default)]
    pub arguments: Value,
    /// Wakes the blocked `execute_one_tool` wait (same id as `permission-request`).
    #[serde(default)]
    pub request_id: Option<String>,
    #[serde(default)]
    pub project_root: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DenyRequest {
    pub session_id: String,
    pub tool_name: String,
    #[serde(default)]
    pub arguments: Value,
    #[serde(default = "default_deny_reason")]
    pub reason: String,
    #[serde(default)]
    pub request_id: Option<String>,
    #[serde(default)]
    pub project_root: Option<String>,
}

fn default_deny_reason() -> String {
    "用户拒绝".to_string()
}

#[derive(Debug, Deserialize)]
pub struct AddRuleRequest {
    pub rule: PermissionRule,
}

#[derive(Debug, Deserialize)]
pub struct UpdateRuleRequest {
    pub rule: PermissionRule,
}

// === 转换函数 ===

fn risk_level_to_string(level: RiskLevel) -> String {
    match level {
        RiskLevel::Safe => "safe",
        RiskLevel::Low => "low",
        RiskLevel::Medium => "medium",
        RiskLevel::High => "high",
        RiskLevel::Critical => "critical",
    }
    .to_string()
}

fn risk_category_to_string(category: &crate::domain::permissions::RiskCategory) -> String {
    match category {
        crate::domain::permissions::RiskCategory::FileSystem => "filesystem",
        crate::domain::permissions::RiskCategory::System => "system",
        crate::domain::permissions::RiskCategory::Network => "network",
        crate::domain::permissions::RiskCategory::DataLoss => "data_loss",
        crate::domain::permissions::RiskCategory::Security => "security",
        crate::domain::permissions::RiskCategory::Privacy => "privacy",
    }
    .to_string()
}

fn convert_risk_info(risk: &DetectedRisk) -> RiskInfoDto {
    RiskInfoDto {
        category: risk_category_to_string(&risk.category),
        severity: risk_level_to_string(risk.severity),
        description: risk.description.clone(),
        mitigation: risk.mitigation.clone(),
    }
}

fn audit_timestamp(raw: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(raw)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

fn permission_mode_to_audit_label(
    mode: &crate::domain::permissions::types::PermissionMode,
) -> String {
    match mode {
        crate::domain::permissions::types::PermissionMode::AskEveryTime => {
            "ask_every_time".to_string()
        }
        crate::domain::permissions::types::PermissionMode::Session => "session".to_string(),
        crate::domain::permissions::types::PermissionMode::TimeWindow { minutes } => {
            format!("time_window:{minutes}")
        }
        crate::domain::permissions::types::PermissionMode::Plan => "plan".to_string(),
        crate::domain::permissions::types::PermissionMode::Auto => "auto".to_string(),
        crate::domain::permissions::types::PermissionMode::Bypass => "bypass".to_string(),
    }
}

fn arguments_to_audit_json(arguments: &Value) -> String {
    serde_json::to_string(arguments).unwrap_or_else(|_| "{}".to_string())
}

fn normalize_project_root(project_root: Option<&str>) -> Option<PathBuf> {
    project_root
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn build_permission_context(
    manager: &PermissionManager,
    session_id: String,
    tool_name: String,
    arguments: Value,
    project_root: Option<&str>,
) -> PermissionContext {
    PermissionContext {
        file_paths: manager.extract_file_paths(&tool_name, &arguments),
        project_root: normalize_project_root(project_root),
        session_id,
        tool_name,
        arguments,
        timestamp: chrono::Utc::now(),
    }
}

async fn waiter_context_for_request(
    app_state: &OmigaAppState,
    request_id: Option<&str>,
) -> Option<PermissionContext> {
    let request_id = request_id?;
    let map = app_state.chat.permission_tool_waiters.lock().await;
    map.get(request_id).map(|waiter| waiter.context.clone())
}

fn context_project_root_label(context: &PermissionContext) -> Option<String> {
    context
        .project_root
        .as_ref()
        .map(|root| root.to_string_lossy().to_string())
}

pub(crate) async fn append_permission_audit_event(
    app_state: &OmigaAppState,
    session_id: &str,
    request_id: Option<&str>,
    project_root: Option<&str>,
    decision: &str,
    tool_name: &str,
    mode: Option<&str>,
    reason: Option<&str>,
    arguments_json: &str,
) {
    if let Err(err) = app_state
        .repo
        .append_permission_audit_event(NewPermissionAuditEventRecord {
            session_id,
            request_id,
            project_root,
            decision,
            tool_name,
            mode,
            reason,
            arguments_json,
        })
        .await
    {
        tracing::warn!(
            target: "omiga::permissions",
            error = %err,
            "failed to append durable permission audit event"
        );
    }
}

fn convert_decision_to_response(
    decision: PermissionDecision,
    session_id: &str,
    tool_name: &str,
    arguments: &Value,
    project_root: Option<&str>,
) -> PermissionCheckResponse {
    match decision {
        PermissionDecision::Allow => PermissionCheckResponse {
            session_id: session_id.to_string(),
            allowed: true,
            requires_approval: false,
            request_id: None,
            tool_name: tool_name.to_string(),
            risk_level: "safe".to_string(),
            risk_description: "已允许".to_string(),
            detected_risks: vec![],
            recommendations: vec![],
            arguments: Some(arguments.clone()),
            project_root: project_root.map(str::to_string),
        },
        PermissionDecision::Deny(reason) => PermissionCheckResponse {
            session_id: session_id.to_string(),
            allowed: false,
            requires_approval: false,
            request_id: None,
            tool_name: tool_name.to_string(),
            risk_level: "high".to_string(),
            risk_description: reason,
            detected_risks: vec![],
            recommendations: vec!["如需允许此操作，请手动批准".to_string()],
            arguments: Some(arguments.clone()),
            project_root: project_root.map(str::to_string),
        },
        PermissionDecision::RequireApproval(req) => PermissionCheckResponse {
            session_id: session_id.to_string(),
            allowed: false,
            requires_approval: true,
            request_id: Some(req.request_id.clone()),
            tool_name: tool_name.to_string(),
            risk_level: risk_level_to_string(req.risk.level),
            risk_description: req.risk.description.clone(),
            detected_risks: req
                .risk
                .detected_risks
                .iter()
                .map(convert_risk_info)
                .collect(),
            recommendations: req.risk.recommendations.clone(),
            arguments: Some(arguments.clone()),
            project_root: project_root.map(str::to_string),
        },
    }
}

// === 命令 ===

/// 检查权限
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionCheckRequest {
    pub session_id: String,
    pub tool_name: String,
    #[serde(default)]
    pub arguments: Value,
    #[serde(default)]
    pub project_root: Option<String>,
}

#[tauri::command]
pub async fn permission_check(
    request: PermissionCheckRequest,
    manager: State<'_, Arc<PermissionManager>>,
    app_state: State<'_, OmigaAppState>,
) -> CommandResult<PermissionCheckResponse> {
    let session_id = request.session_id;
    let tool_name = request.tool_name;
    let arguments = request.arguments;
    let project_root = request.project_root;
    let context = build_permission_context(
        &manager,
        session_id,
        tool_name.clone(),
        arguments.clone(),
        project_root.as_deref(),
    );

    let decision = manager.check_permission(&context).await;
    if let PermissionDecision::Deny(reason) = &decision {
        let arguments_json = arguments_to_audit_json(&arguments);
        append_permission_audit_event(
            &app_state,
            &context.session_id,
            None,
            project_root.as_deref(),
            "denied",
            &tool_name,
            None,
            Some(reason),
            &arguments_json,
        )
        .await;
    }
    let response = convert_decision_to_response(
        decision,
        &context.session_id,
        &tool_name,
        &arguments,
        project_root.as_deref(),
    );

    Ok(response)
}

/// 批准权限请求
#[tauri::command]
pub async fn permission_approve(
    request: ApproveRequest,
    manager: State<'_, Arc<PermissionManager>>,
    app_state: State<'_, OmigaAppState>,
) -> CommandResult<()> {
    tracing::info!(
        session_id = %request.session_id,
        tool_name = %request.tool_name,
        mode = ?request.mode,
        "Approving permission"
    );

    // 构建 context 用于批准。聊天工具审批使用后端保存的原始 context；
    // renderer 只需要脱敏参数用于展示，避免把 secrets 往返传给前端。
    let context = waiter_context_for_request(&app_state, request.request_id.as_deref())
        .await
        .unwrap_or_else(|| {
            build_permission_context(
                &manager,
                request.session_id.clone(),
                request.tool_name.clone(),
                request.arguments.clone(),
                request.project_root.as_deref(),
            )
        });

    let mode: crate::domain::permissions::types::PermissionMode = request.mode.clone().into();
    let mode_label = permission_mode_to_audit_label(&mode);
    let arguments_json = arguments_to_audit_json(&context.arguments);
    let context_project_root = context_project_root_label(&context);

    manager
        .approve_request(&context.session_id, mode, &context)
        .await
        .map_err(crate::errors::AppError::Unknown)?;

    append_permission_audit_event(
        &app_state,
        &context.session_id,
        request.request_id.as_deref(),
        context_project_root.as_deref(),
        "approved",
        &context.tool_name,
        Some(&mode_label),
        None,
        &arguments_json,
    )
    .await;

    if let Some(rid) = request.request_id.as_ref() {
        let mut map = app_state.chat.permission_tool_waiters.lock().await;
        if let Some(waiter) = map.remove(rid) {
            let _ = waiter.tx.send(Ok(()));
        }
    }

    tracing::info!("Permission approved successfully");
    Ok(())
}

/// 拒绝权限请求
#[tauri::command]
pub async fn permission_deny(
    request: DenyRequest,
    manager: State<'_, Arc<PermissionManager>>,
    app_state: State<'_, OmigaAppState>,
) -> CommandResult<()> {
    tracing::info!(
        session_id = %request.session_id,
        tool_name = %request.tool_name,
        reason = %request.reason,
        "Denying permission"
    );

    // 构建 context 用于拒绝。聊天工具审批优先使用后端保存的原始 context，
    // 避免前端持有或回传未脱敏参数。
    let context = waiter_context_for_request(&app_state, request.request_id.as_deref())
        .await
        .unwrap_or_else(|| {
            build_permission_context(
                &manager,
                request.session_id.clone(),
                request.tool_name.clone(),
                request.arguments.clone(),
                request.project_root.as_deref(),
            )
        });

    manager
        .deny_request(&context, &request.reason)
        .await
        .map_err(crate::errors::AppError::Unknown)?;

    let arguments_json = arguments_to_audit_json(&context.arguments);
    let context_project_root = context_project_root_label(&context);
    append_permission_audit_event(
        &app_state,
        &context.session_id,
        request.request_id.as_deref(),
        context_project_root.as_deref(),
        "denied",
        &context.tool_name,
        None,
        Some(&request.reason),
        &arguments_json,
    )
    .await;

    if let Err(err) = crate::domain::connectors::append_connector_permission_denial_audit_event(
        &context.tool_name,
        &context.arguments,
        Some(&context.session_id),
        None,
        &request.reason,
    ) {
        tracing::warn!(
            target: "omiga::connectors",
            error = %err,
            "failed to append connector permission-denial audit event"
        );
    }

    if let Some(rid) = request.request_id.as_ref() {
        let mut map = app_state.chat.permission_tool_waiters.lock().await;
        if let Some(waiter) = map.remove(rid) {
            let _ = waiter.tx.send(Err(request.reason.clone()));
        }
    }

    tracing::info!("Permission denied successfully");
    Ok(())
}

/// 获取所有规则
#[tauri::command]
pub async fn permission_list_rules(
    manager: State<'_, Arc<PermissionManager>>,
) -> CommandResult<Vec<PermissionRule>> {
    let rules = manager.get_rules().await;
    Ok(rules)
}

/// 添加规则
#[tauri::command]
pub async fn permission_add_rule(
    request: AddRuleRequest,
    manager: State<'_, Arc<PermissionManager>>,
) -> CommandResult<()> {
    manager
        .add_rule(request.rule)
        .await
        .map_err(crate::errors::AppError::Unknown)?;
    Ok(())
}

/// 删除规则
#[tauri::command]
pub async fn permission_delete_rule(
    id: String,
    manager: State<'_, Arc<PermissionManager>>,
) -> CommandResult<()> {
    manager
        .delete_rule(&id)
        .await
        .map_err(crate::errors::AppError::Unknown)?;
    Ok(())
}

/// 更新规则
#[tauri::command]
pub async fn permission_update_rule(
    request: UpdateRuleRequest,
    manager: State<'_, Arc<PermissionManager>>,
) -> CommandResult<()> {
    manager
        .update_rule(request.rule)
        .await
        .map_err(crate::errors::AppError::Unknown)?;
    Ok(())
}

/// 获取最近拒绝记录
#[tauri::command]
pub async fn permission_get_recent_denials(
    limit: Option<usize>,
    project_root: Option<String>,
    app_state: State<'_, OmigaAppState>,
    manager: State<'_, Arc<PermissionManager>>,
) -> CommandResult<Vec<DenialRecordDto>> {
    let limit = limit.unwrap_or(50);
    match app_state
        .repo
        .list_recent_permission_denials(limit as i64, project_root.as_deref())
        .await
    {
        Ok(denials) => {
            let dtos = denials
                .into_iter()
                .map(|d| DenialRecordDto {
                    id: d.id,
                    timestamp: audit_timestamp(&d.created_at),
                    tool_name: d.tool_name,
                    reason: d.reason.unwrap_or_else(|| "denied".to_string()),
                })
                .collect();
            return Ok(dtos);
        }
        Err(err) => {
            tracing::warn!(
                target: "omiga::permissions",
                error = %err,
                "failed to load durable permission denials; falling back to in-memory denials"
            );
        }
    }

    let denials = manager.get_recent_denials(limit).await;

    let dtos: Vec<DenialRecordDto> = denials
        .into_iter()
        .map(|d| DenialRecordDto {
            id: d.id,
            timestamp: d.timestamp,
            tool_name: d.tool_name,
            reason: d.reason,
        })
        .collect();

    Ok(dtos)
}

/// 获取持久化权限审计事件
#[tauri::command]
pub async fn permission_get_audit_events(
    limit: Option<usize>,
    project_root: Option<String>,
    offset: Option<usize>,
    decision: Option<String>,
    tool_query: Option<String>,
    app_state: State<'_, OmigaAppState>,
) -> CommandResult<PermissionAuditEventsResponse> {
    let limit = limit.unwrap_or(100);
    let offset = offset.unwrap_or(0);
    let page = app_state
        .repo
        .list_recent_permission_audit_events_page(
            limit as i64,
            offset as i64,
            project_root.as_deref(),
            decision.as_deref(),
            tool_query.as_deref(),
        )
        .await
        .map_err(|err| crate::errors::AppError::Persistence(err.to_string()))?;

    let events = page
        .events
        .into_iter()
        .map(|event| PermissionAuditEventDto {
            id: event.id,
            session_id: event.session_id,
            request_id: event.request_id,
            project_root: event.project_root,
            decision: event.decision,
            tool_name: event.tool_name,
            mode: event.mode,
            reason: event.reason,
            arguments: serde_json::from_str(&event.arguments_json).unwrap_or(Value::Null),
            timestamp: audit_timestamp(&event.created_at),
        })
        .collect();

    Ok(PermissionAuditEventsResponse {
        events,
        total_count: page.total_count.max(0) as usize,
        approved_count: page.facets.approved_count.max(0) as usize,
        denied_count: page.facets.denied_count.max(0) as usize,
    })
}

/// 设置默认模式
#[tauri::command]
pub async fn permission_set_default_mode(
    mode: PermissionModeInput,
    manager: State<'_, Arc<PermissionManager>>,
) -> CommandResult<()> {
    manager.set_default_mode(mode).await;
    Ok(())
}

/// 获取批准状态
#[tauri::command]
pub async fn permission_get_approval_status(
    session_id: String,
    manager: State<'_, Arc<PermissionManager>>,
) -> CommandResult<ApprovalStatusResponse> {
    let (approved_tools, approved_until) = manager.get_session_approvals(&session_id).await;

    Ok(ApprovalStatusResponse {
        session_id,
        approved_tools: approved_tools.into_iter().collect(),
        approved_until,
    })
}

/// 清除会话批准
#[tauri::command]
pub async fn permission_clear_session_approvals(
    session_id: String,
    manager: State<'_, Arc<PermissionManager>>,
) -> CommandResult<()> {
    manager.clear_session_approvals(&session_id).await;
    Ok(())
}
