use crate::app_state::OmigaAppState;
use crate::commands::CommandResult;
use crate::domain::permissions::manager::PermissionManager;
use crate::domain::permissions::types::{
    PermissionRule, PermissionContext, PermissionDecision, RiskLevel, PermissionModeInput,
    DetectedRisk,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
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
    }.to_string()
}

fn risk_category_to_string(category: &crate::domain::permissions::RiskCategory) -> String {
    match category {
        crate::domain::permissions::RiskCategory::FileSystem => "filesystem",
        crate::domain::permissions::RiskCategory::System => "system",
        crate::domain::permissions::RiskCategory::Network => "network",
        crate::domain::permissions::RiskCategory::DataLoss => "data_loss",
        crate::domain::permissions::RiskCategory::Security => "security",
        crate::domain::permissions::RiskCategory::Privacy => "privacy",
    }.to_string()
}

fn convert_risk_info(risk: &DetectedRisk) -> RiskInfoDto {
    RiskInfoDto {
        category: risk_category_to_string(&risk.category),
        severity: risk_level_to_string(risk.severity),
        description: risk.description.clone(),
        mitigation: risk.mitigation.clone(),
    }
}

fn convert_decision_to_response(
    decision: PermissionDecision,
    session_id: &str,
    tool_name: &str,
    arguments: &Value,
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
        },
        PermissionDecision::RequireApproval(req) => PermissionCheckResponse {
            session_id: session_id.to_string(),
            allowed: false,
            requires_approval: true,
            request_id: Some(req.request_id.clone()),
            tool_name: tool_name.to_string(),
            risk_level: risk_level_to_string(req.risk.level),
            risk_description: req.risk.description.clone(),
            detected_risks: req.risk.detected_risks.iter().map(convert_risk_info).collect(),
            recommendations: req.risk.recommendations.clone(),
            arguments: Some(arguments.clone()),
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
}

#[tauri::command]
pub async fn permission_check(
    request: PermissionCheckRequest,
    manager: State<'_, Arc<PermissionManager>>,
) -> CommandResult<PermissionCheckResponse> {
    let session_id = request.session_id;
    let tool_name = request.tool_name;
    let arguments = request.arguments;
    let context = PermissionContext {
        session_id,
        tool_name: tool_name.clone(),
        arguments: arguments.clone(),
        file_paths: None,
        timestamp: chrono::Utc::now(),
    };

    let decision = manager.check_permission(&context).await;
    let response = convert_decision_to_response(
        decision,
        &context.session_id,
        &tool_name,
        &arguments,
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

    // 构建 context 用于批准
    let context = PermissionContext {
        session_id: request.session_id.clone(),
        tool_name: request.tool_name.clone(),
        arguments: request.arguments.clone(),
        file_paths: None,
        timestamp: chrono::Utc::now(),
    };

    // 转换 PermissionModeInput -> PermissionMode
    let mode = match request.mode {
        PermissionModeInput::AskEveryTime => crate::domain::permissions::types::PermissionMode::AskEveryTime,
        PermissionModeInput::Session => crate::domain::permissions::types::PermissionMode::Session,
        PermissionModeInput::TimeWindow { minutes } => {
            crate::domain::permissions::types::PermissionMode::TimeWindow { minutes }
        }
        PermissionModeInput::Plan => crate::domain::permissions::types::PermissionMode::Plan,
        PermissionModeInput::Auto => crate::domain::permissions::types::PermissionMode::Auto,
    };

    manager.approve_request(&request.session_id, mode, &context).await
        .map_err(|e| crate::errors::AppError::Unknown(e))?;

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

    // 构建 context 用于拒绝
    let context = PermissionContext {
        session_id: request.session_id,
        tool_name: request.tool_name,
        arguments: request.arguments,
        file_paths: None,
        timestamp: chrono::Utc::now(),
    };

    manager.deny_request(&context, &request.reason).await
        .map_err(|e| crate::errors::AppError::Unknown(e))?;

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
    manager.add_rule(request.rule).await
        .map_err(|e| crate::errors::AppError::Unknown(e))?;
    Ok(())
}

/// 删除规则
#[tauri::command]
pub async fn permission_delete_rule(
    id: String,
    manager: State<'_, Arc<PermissionManager>>,
) -> CommandResult<()> {
    manager.delete_rule(&id).await
        .map_err(|e| crate::errors::AppError::Unknown(e))?;
    Ok(())
}

/// 更新规则
#[tauri::command]
pub async fn permission_update_rule(
    request: UpdateRuleRequest,
    manager: State<'_, Arc<PermissionManager>>,
) -> CommandResult<()> {
    manager.update_rule(request.rule).await
        .map_err(|e| crate::errors::AppError::Unknown(e))?;
    Ok(())
}

/// 获取最近拒绝记录
#[tauri::command]
pub async fn permission_get_recent_denials(
    limit: Option<usize>,
    manager: State<'_, Arc<PermissionManager>>,
) -> CommandResult<Vec<DenialRecordDto>> {
    let limit = limit.unwrap_or(50);
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
