//! 权限管理器核心实现
//!
//! 子模块组织：
//! - `risk_assessment`：工具风险分类、路径越界检测、文件路径提取
//! - `workspace_guard`：工作区安全守卫（免弹窗自动放行逻辑）

mod risk_assessment;
mod workspace_guard;

use super::patterns::DangerousPatternDB;
use super::tool_rules::canonical_permission_tool_name;
use super::types::*;
use crate::domain::connectors;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, OnceLock};
use tokio::sync::RwLock;

/// 从 bash/shell 命令中提取路径的静态正则（只编译一次）
static BASH_PATH_REGEX: OnceLock<regex::Regex> = OnceLock::new();

fn bash_path_regex() -> &'static regex::Regex {
    BASH_PATH_REGEX.get_or_init(|| {
        regex::Regex::new(r"(?:^|[\s;|&])([/~][\w\-./~]+)").expect("bash path regex is valid")
    })
}

/// 权限管理器
pub struct PermissionManager {
    /// 权限规则列表（按优先级排序）
    rules: Arc<RwLock<Vec<PermissionRule>>>,
    /// 危险模式数据库
    patterns: DangerousPatternDB,
    /// 会话级批准缓存: session_id → 批准单元键。
    /// 大多数工具按规范化工具名缓存；bash/shell 额外带上命令类别，避免一次批准放行全部命令。
    session_approvals: Arc<RwLock<HashMap<String, HashSet<String>>>>,
    /// 时间窗口批准: "session_id:tool_key" → expire_time
    window_approvals: Arc<RwLock<HashMap<String, chrono::DateTime<chrono::Utc>>>>,
    /// 单次批准: "session_id:tool_key" → expire_time（30 秒内有效）
    once_approvals: Arc<RwLock<HashMap<String, chrono::DateTime<chrono::Utc>>>>,
    /// 会话级拒绝: session_id → 拒绝单元键（与批准单元一致）。
    session_denials: Arc<RwLock<HashMap<String, HashSet<String>>>>,
    /// 最近拒绝记录（审计日志）
    recent_denials: Arc<RwLock<VecDeque<DenialRecord>>>,
    /// 会话级 composer「权限模式」：`session_id` → 最近一次 `send_message` 带入的默认拦截立场（无规则匹配时使用）
    session_composer_stance: Arc<RwLock<HashMap<String, ComposerPermissionStance>>>,
    /// 工作区排除路径：即使在工作区智能放行模式下也需要确认的路径前缀（相对于 project_root）。
    /// 使用 std::sync::RwLock 而非 tokio::sync::RwLock，因为 is_workspace_safe 是同步方法。
    workspace_exclusions: Arc<std::sync::RwLock<Vec<String>>>,
}

impl PermissionManager {
    /// 创建新的权限管理器
    pub fn new() -> Self {
        Self {
            rules: Arc::new(RwLock::new(Vec::new())),
            patterns: DangerousPatternDB::new(),
            session_approvals: Arc::new(RwLock::new(HashMap::new())),
            window_approvals: Arc::new(RwLock::new(HashMap::new())),
            once_approvals: Arc::new(RwLock::new(HashMap::new())),
            session_denials: Arc::new(RwLock::new(HashMap::new())),
            recent_denials: Arc::new(RwLock::new(VecDeque::with_capacity(100))),
            session_composer_stance: Arc::new(RwLock::new(HashMap::new())),
            workspace_exclusions: Arc::new(std::sync::RwLock::new(Vec::new())),
        }
    }

    // =========================================================================
    // 工作区排除路径 API
    // =========================================================================

    /// 替换工作区排除路径列表。路径应为相对于 project_root 的前缀，例如 "dist/"、".env"。
    pub fn set_workspace_exclusions(&self, patterns: Vec<String>) {
        if let Ok(mut g) = self.workspace_exclusions.write() {
            *g = patterns;
        }
    }

    /// 返回当前工作区排除路径列表的副本。
    pub fn get_workspace_exclusions(&self) -> Vec<String> {
        self.workspace_exclusions
            .read()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    // =========================================================================
    // 公共 API
    // =========================================================================

    /// 核心权限检查方法
    pub async fn check_permission(&self, context: &PermissionContext) -> PermissionDecision {
        let tool_key = Self::approval_cache_key_for_context(context);

        // 0. 检查本会话是否已主动拒绝此批准单元（bash/shell 精确到命令类别）
        {
            let denials = self.session_denials.read().await;
            if let Some(session_denied) = denials.get(&context.session_id) {
                if session_denied.contains(&tool_key) {
                    return PermissionDecision::Deny(format!(
                        "工具 '{}' 已在本会话中被拒绝",
                        context.tool_name
                    ));
                }
            }
        }

        // 1. 用户已选择「本会话 / 时间窗口 / 单次」记住的批准优先于风险等级与规则
        //    （否则 Critical 短路、AskEveryTime 规则等会跳过缓存，导致每次都弹窗）
        if self
            .is_remembered_tool_allowed(&context.session_id, &tool_key)
            .await
        {
            return PermissionDecision::Allow;
        }

        // 2. 风险评估
        let risk = self.assess_risk(context).await;

        // 3. Critical 风险立即要求确认（未命中上述批准缓存时）
        if risk.level == RiskLevel::Critical {
            return PermissionDecision::RequireApproval(Box::new(PermissionRequest {
                request_id: uuid::Uuid::new_v4().to_string(),
                context: context.clone(),
                risk,
                suggested_mode: PermissionMode::AskEveryTime,
            }));
        }

        // 4. 规则匹配（克隆匹配到的规则，释放读锁）
        let maybe_rule = {
            let rules = self.rules.read().await;
            rules
                .iter()
                .filter(|r| self.rule_matches(r, context))
                .min_by_key(|r| r.priority)
                .cloned()
        };

        if let Some(rule) = maybe_rule {
            let decision = self.apply_rule(&rule, context, &risk).await;
            // 规则被成功应用后递增使用计数
            if !matches!(decision, PermissionDecision::Deny(_)) {
                self.increment_use_count(&rule.id).await;
            }
            return decision;
        }

        // 5. 无规则匹配时：按会话 composer 权限立场或系统默认分级
        let stance = self.get_session_composer_stance(&context.session_id).await;
        match stance {
            Some(ComposerPermissionStance::Auto) => self.composer_auto_fallback(context, &risk),
            Some(ComposerPermissionStance::Bypass) => self.composer_bypass_fallback(),
            // Ask 模式：用户明确要求每次询问，工作区智能放行不应绕过该意图
            Some(ComposerPermissionStance::Ask) => {
                self.default_decision(context, &risk, false).await
            }
            // 无明确立场时使用默认分级（允许工作区智能放行）
            None => self.default_decision(context, &risk, true).await,
        }
    }

    /// 简化的检查接口（用于工具调用，自动构建 context）
    pub async fn check_tool(
        &self,
        session_id: &str,
        tool_name: &str,
        arguments: &serde_json::Value,
    ) -> PermissionDecision {
        self.check_tool_with_root(session_id, tool_name, arguments, None)
            .await
    }

    pub async fn check_tool_with_root(
        &self,
        session_id: &str,
        tool_name: &str,
        arguments: &serde_json::Value,
        project_root: Option<&std::path::Path>,
    ) -> PermissionDecision {
        let context = PermissionContext {
            tool_name: tool_name.to_string(),
            arguments: arguments.clone(),
            session_id: session_id.to_string(),
            file_paths: self.extract_file_paths(tool_name, arguments),
            timestamp: chrono::Utc::now(),
            project_root: project_root.map(|p| p.to_path_buf()),
        };
        self.check_permission(&context).await
    }

    /// 批准请求
    pub async fn approve_request(
        &self,
        session_id: &str,
        mode: PermissionMode,
        context: &PermissionContext,
    ) -> Result<(), String> {
        // 校验模式合法性（前端不能传 Bypass）
        mode.validate_user_mode()?;

        let tool_key = Self::approval_cache_key_for_context(context);

        // 批准时移除本会话内对该工具的拒绝记录（用户改变了主意）
        {
            let mut denials = self.session_denials.write().await;
            if let Some(session_denied) = denials.get_mut(session_id) {
                session_denied.remove(&tool_key);
            }
        }

        if Self::approval_must_be_single_use(context) {
            self.remember_once_approval(session_id, &tool_key).await;
            return Ok(());
        }

        match mode {
            PermissionMode::Session | PermissionMode::Plan => {
                let mut approvals = self.session_approvals.write().await;
                approvals
                    .entry(session_id.to_string())
                    .or_default()
                    .insert(tool_key);
            }
            PermissionMode::TimeWindow { minutes } => {
                // 已经在 validate_user_mode 中校验了最大值和非零
                let expire_at = chrono::Utc::now() + chrono::Duration::minutes(minutes as i64);
                let mut windows = self.window_approvals.write().await;
                let session_key = format!("{}:{}", session_id, tool_key);
                windows.insert(session_key, expire_at);
            }
            PermissionMode::AskEveryTime => {
                // 单次批准：存储在临时缓存中，30秒内有效
                // 这给用户时间重新触发操作，但不会长期保留
                self.remember_once_approval(session_id, &tool_key).await;
            }
            PermissionMode::Auto => {
                // Auto 模式由规则控制，单次批准无需持久化
            }
            PermissionMode::Bypass => {
                // validate_user_mode 已经返回 Err，此处不可达
                unreachable!("Bypass mode rejected by validate_user_mode")
            }
        }

        Ok(())
    }

    /// 拒绝请求（记录审计日志，并在本会话中不再询问该工具——与参数无关）
    pub async fn deny_request(
        &self,
        context: &PermissionContext,
        reason: &str,
    ) -> Result<(), String> {
        let tool_key = Self::approval_cache_key_for_context(context);

        {
            let mut denials = self.session_denials.write().await;
            denials
                .entry(context.session_id.clone())
                .or_default()
                .insert(tool_key);
        }

        // 写入审计日志
        let denial = DenialRecord {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now(),
            tool_name: context.tool_name.clone(),
            arguments: context.arguments.clone(),
            reason: reason.to_string(),
            session_id: context.session_id.clone(),
        };

        const MAX_DENIAL_RECORDS: usize = 100;
        let mut denials_log = self.recent_denials.write().await;
        if denials_log.len() >= MAX_DENIAL_RECORDS {
            denials_log.pop_back();
        }
        denials_log.push_front(denial);

        Ok(())
    }

    /// 添加权限规则
    pub async fn add_rule(&self, rule: PermissionRule) -> Result<(), String> {
        let mut rules = self.rules.write().await;
        rules.push(rule);
        rules.sort_by_key(|r| r.priority);
        Ok(())
    }

    /// 删除权限规则
    pub async fn delete_rule(&self, rule_id: &str) -> Result<(), String> {
        let mut rules = self.rules.write().await;
        rules.retain(|r| r.id != rule_id);
        Ok(())
    }

    /// 列出所有规则
    pub async fn list_rules(&self) -> Vec<PermissionRule> {
        self.rules.read().await.clone()
    }

    /// 获取最近拒绝记录
    pub async fn get_recent_denials(&self, limit: usize) -> Vec<DenialRecord> {
        let denials = self.recent_denials.read().await;
        denials.iter().take(limit).cloned().collect()
    }

    /// 更新规则
    pub async fn update_rule(&self, rule: PermissionRule) -> Result<(), String> {
        let mut rules = self.rules.write().await;
        if let Some(index) = rules.iter().position(|r| r.id == rule.id) {
            rules[index] = rule;
            rules.sort_by_key(|r| r.priority);
            Ok(())
        } else {
            Err("规则不存在".to_string())
        }
    }

    /// 获取所有规则
    pub async fn get_rules(&self) -> Vec<PermissionRule> {
        self.rules.read().await.clone()
    }

    /// 会话级别批准（bash/shell 按命令类别，其它工具按规范化工具名）
    pub async fn approve_session(
        &self,
        session_id: String,
        tool_name: String,
        arguments: &serde_json::Value,
    ) {
        let context = PermissionContext {
            tool_name,
            arguments: arguments.clone(),
            session_id: session_id.clone(),
            file_paths: None,
            timestamp: chrono::Utc::now(),
            project_root: None,
        };
        let tool_key = Self::approval_cache_key_for_context(&context);
        if Self::approval_must_be_single_use(&context) {
            self.remember_once_approval(&session_id, &tool_key).await;
            return;
        }
        let mut approvals = self.session_approvals.write().await;
        approvals.entry(session_id).or_default().insert(tool_key);
    }

    /// 时间窗口批准（bash/shell 按命令类别，绑定到 session）
    pub async fn approve_time_window(
        &self,
        session_id: String,
        tool_name: String,
        arguments: &serde_json::Value,
        minutes: i64,
    ) {
        let context = PermissionContext {
            tool_name,
            arguments: arguments.clone(),
            session_id: session_id.clone(),
            file_paths: None,
            timestamp: chrono::Utc::now(),
            project_root: None,
        };
        let tool_key = Self::approval_cache_key_for_context(&context);
        if Self::approval_must_be_single_use(&context) {
            self.remember_once_approval(&session_id, &tool_key).await;
            return;
        }
        let expire_at = chrono::Utc::now() + chrono::Duration::minutes(minutes);
        let mut windows = self.window_approvals.write().await;
        let session_key = format!("{}:{}", session_id, tool_key);
        windows.insert(session_key, expire_at);
    }

    /// 按 `tool_name` 记入本会话批准（与 `approve_session` 相同语义）。
    /// `_legacy_hash` 已废弃：旧调用方没有参数上下文，因此仅保留非 bash 的宽松兼容行为。
    pub async fn approve_with_hash(
        &self,
        session_id: String,
        tool_name: String,
        _legacy_hash: String,
    ) {
        let tool_key = Self::approval_cache_key(&tool_name);
        let mut approvals = self.session_approvals.write().await;
        approvals.entry(session_id).or_default().insert(tool_key);
    }

    /// 单次批准（仅这次允许，不持久化）
    pub async fn approve_once(&self, _session_id: String, _tool_name: String, _hash: String) {
        // 单次批准不需要持久化，直接通过 check 已经返回 Allow
        // 这里可以添加临时缓存如果需要
    }

    /// 旧接口：拒绝工具（无参数上下文，按工具名记入本会话拒绝列表，`hash` 参数已忽略）
    pub async fn deny_tool(
        &self,
        session_id: String,
        tool_name: String,
        _hash: String,
        reason: String,
    ) {
        let tool_key = Self::approval_cache_key(&tool_name);
        {
            let mut denials = self.session_denials.write().await;
            denials
                .entry(session_id.clone())
                .or_default()
                .insert(tool_key);
        }

        // 写入审计日志
        let denial = DenialRecord {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now(),
            tool_name: tool_name.clone(),
            arguments: serde_json::Value::Null,
            reason,
            session_id,
        };

        const MAX_DENIAL_RECORDS: usize = 100;
        let mut denials_log = self.recent_denials.write().await;
        if denials_log.len() >= MAX_DENIAL_RECORDS {
            denials_log.pop_back();
        }
        denials_log.push_front(denial);
    }

    /// 设置默认模式
    pub async fn set_default_mode(
        &self,
        mode: crate::domain::permissions::types::PermissionModeInput,
    ) {
        // 实际项目中应存储在配置中
        tracing::info!(?mode, "Setting default permission mode");
    }

    /// 同步聊天 composer 的 `permissionMode`（`ask` \| `auto` \| `bypass`），作为本会话在**无用户规则命中时**的工具拦截默认策略。
    /// `raw` 为 `None` 或空字符串时不修改已有立场（便于仅更新其它字段的请求）。
    pub async fn set_session_composer_stance(&self, session_id: &str, raw: Option<&str>) {
        let Some(s) = raw.map(str::trim).filter(|x| !x.is_empty()) else {
            return;
        };
        let mut map = self.session_composer_stance.write().await;
        match s {
            "ask" => {
                map.insert(session_id.to_string(), ComposerPermissionStance::Ask);
            }
            "auto" => {
                map.insert(session_id.to_string(), ComposerPermissionStance::Auto);
            }
            "bypass" => {
                map.insert(session_id.to_string(), ComposerPermissionStance::Bypass);
            }
            _ => {
                map.remove(session_id);
            }
        }
    }

    pub async fn remove_session_composer_stance(&self, session_id: &str) {
        let mut map = self.session_composer_stance.write().await;
        map.remove(session_id);
    }

    async fn get_session_composer_stance(
        &self,
        session_id: &str,
    ) -> Option<ComposerPermissionStance> {
        let map = self.session_composer_stance.read().await;
        map.get(session_id).copied()
    }

    /// 获取会话批准状态
    pub async fn get_session_approvals(
        &self,
        session_id: &str,
    ) -> (
        std::collections::HashSet<String>,
        Option<chrono::DateTime<chrono::Utc>>,
    ) {
        let approvals = self.session_approvals.read().await;
        let approved_tools = approvals.get(session_id).cloned().unwrap_or_default();

        // 获取该 session 的时间窗口批准中最早的过期时间
        let windows = self.window_approvals.read().await;
        let session_prefix = format!("{}:", session_id);
        let approved_until = windows
            .iter()
            .filter(|(key, _)| key.starts_with(&session_prefix))
            .map(|(_, expire_at)| *expire_at)
            .min();

        (approved_tools, approved_until)
    }

    /// 清除会话批准
    pub async fn clear_session_approvals(&self, session_id: &str) {
        let mut approvals = self.session_approvals.write().await;
        approvals.remove(session_id);

        let mut denials = self.session_denials.write().await;
        denials.remove(session_id);

        // 清除该 session 的时间窗口和单次批准
        let prefix = format!("{}:", session_id);

        let mut windows = self.window_approvals.write().await;
        windows.retain(|key, _| !key.starts_with(&prefix));

        let mut once = self.once_approvals.write().await;
        once.retain(|key, _| !key.starts_with(&prefix));
    }

    // =========================================================================
    // 内部辅助方法
    // =========================================================================

    /// 用户「记住」批准 / 本会话拒绝 使用的基础键：与 `tool_rules` 一致，Read/Search 等与内置名合并
    fn approval_cache_key(tool_name: &str) -> String {
        let c = canonical_permission_tool_name(tool_name.trim());
        if c.starts_with("mcp__") {
            c
        } else {
            c.to_ascii_lowercase()
        }
    }

    fn approval_cache_key_for_context(context: &PermissionContext) -> String {
        let base = Self::approval_cache_key(&context.tool_name);
        match base.as_str() {
            "connector" => {
                let Some((connector_id, operation)) =
                    connectors::connector_permission_identity_from_args(&context.arguments)
                else {
                    return base;
                };
                format!("connector:{connector_id}:{operation}")
            }
            "bash" | "shell" => Self::bash_approval_cache_key(&base, &context.arguments),
            _ => base,
        }
    }

    fn bash_approval_cache_key(base: &str, arguments: &serde_json::Value) -> String {
        let Some(command) = arguments
            .get("command")
            .or_else(|| arguments.get("cmd"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            return base.to_string();
        };

        format!("{base}:{}", Self::bash_command_class(command))
    }

    fn approval_must_be_single_use(context: &PermissionContext) -> bool {
        let base = Self::approval_cache_key(&context.tool_name);
        if base == "computer_type" {
            return true;
        }
        if base != "bash" && base != "shell" {
            return false;
        }
        let Some(command) = context
            .arguments
            .get("command")
            .or_else(|| context.arguments.get("cmd"))
            .and_then(|v| v.as_str())
        else {
            return false;
        };
        Self::shell_command_requires_fresh_approval(command)
    }

    fn shell_command_requires_fresh_approval(command: &str) -> bool {
        let lower = command.to_ascii_lowercase();
        if lower.contains("curl") && lower.contains('|') && lower.contains("sh") {
            return true;
        }
        if lower.contains("wget") && lower.contains('|') && lower.contains("sh") {
            return true;
        }
        if lower.contains("find ") && (lower.contains(" -delete") || lower.contains(" -exec rm")) {
            return true;
        }

        let words = Self::shell_words(command);
        let Some((exe, rest)) = Self::command_word_and_rest(&words) else {
            return false;
        };
        Self::is_file_deletion_command(&exe, &rest)
            || Self::is_software_install_command(&exe, &rest)
            || Self::is_destructive_git_command(&exe, &rest)
    }

    fn is_file_deletion_command(exe: &str, rest: &[String]) -> bool {
        matches!(
            exe,
            "rm" | "rmdir" | "unlink" | "shred" | "trash" | "trash-put"
        ) || (exe == "find"
            && (rest.iter().any(|t| t == "-delete")
                || rest
                    .windows(2)
                    .any(|w| w[0] == "-exec" && matches!(w[1].as_str(), "rm" | "/bin/rm"))))
    }

    fn is_destructive_git_command(exe: &str, rest: &[String]) -> bool {
        if exe != "git" {
            return false;
        }
        match Self::git_subcommand(rest).as_deref() {
            Some("clean") => true,
            Some("reset") => rest.iter().any(|t| t == "--hard"),
            Some("push") => rest
                .iter()
                .any(|t| matches!(t.as_str(), "--force" | "--force-with-lease" | "-f")),
            _ => false,
        }
    }

    fn is_software_install_command(exe: &str, rest: &[String]) -> bool {
        let first = Self::first_non_option_token(rest);
        match exe {
            "npm" => matches!(first, Some("install" | "i" | "add" | "ci")),
            "yarn" | "pnpm" | "bun" => matches!(first, Some("add" | "install")),
            "pip" | "pip3" => matches!(first, Some("install")),
            "python" | "python2" | "python3" => {
                rest.first().map(String::as_str) == Some("-m")
                    && rest.get(1).map(String::as_str) == Some("pip")
                    && rest.iter().any(|t| t == "install")
            }
            "uv" => match first {
                Some("add" | "sync") => true,
                Some("pip") => Self::token_after(rest, "pip") == Some("install"),
                Some("tool") => Self::token_after(rest, "tool") == Some("install"),
                _ => false,
            },
            "cargo" | "gem" | "brew" | "port" => matches!(first, Some("install")),
            "go" => matches!(first, Some("install" | "get")),
            "apt" | "apt-get" | "yum" | "dnf" => matches!(first, Some("install")),
            "apk" => matches!(first, Some("add")),
            "pacman" => rest.iter().any(|t| t == "-S" || t.starts_with("-S")),
            "conda" | "mamba" | "micromamba" => matches!(first, Some("install" | "create")),
            _ => false,
        }
    }

    fn bash_command_class(command: &str) -> String {
        let normalized = Self::normalize_command_for_cache(command);
        if Self::has_shell_control_or_redirection(command) {
            return format!("hash:{}", Self::short_hash(&normalized));
        }

        let words = Self::shell_words(command);
        let Some((exe, rest)) = Self::command_word_and_rest(&words) else {
            return format!("hash:{}", Self::short_hash(&normalized));
        };

        if Self::hash_scoped_shell_command(&exe) {
            return format!("cmd:{exe}:{}", Self::short_hash(&normalized));
        }

        let mut parts = vec![format!("cmd:{exe}")];
        match exe.as_str() {
            "git" | "cargo" | "docker" | "docker-compose" | "kubectl" | "go" | "pip" | "pip3"
            | "poetry" | "brew" | "apt" | "apt-get" | "yum" | "dnf" | "pacman" | "apk" | "make"
            | "just" => {
                if let Some(sub) = Self::first_non_option_token(&rest) {
                    parts.push(sub.to_string());
                }
            }
            "npm" | "pnpm" | "yarn" | "bun" | "uv" => {
                if let Some(sub) = Self::first_non_option_token(&rest) {
                    parts.push(sub.to_string());
                    if matches!(sub, "run" | "exec" | "dlx" | "tool") {
                        if let Some(next) = Self::token_after(&rest, sub) {
                            parts.push(next.to_string());
                        }
                    }
                }
            }
            "python" | "python2" | "python3" | "node" | "ruby" | "perl" | "rscript" => {
                if rest.iter().any(|t| matches!(t.as_str(), "-c" | "-e" | "-"))
                    || command.contains("<<")
                {
                    parts.push(Self::short_hash(&normalized));
                } else if rest.first().map(|s| s.as_str()) == Some("-m") {
                    parts.push("-m".to_string());
                    if let Some(module) = rest.get(1) {
                        parts.push(module.clone());
                    }
                } else if let Some(script) = Self::first_non_option_token(&rest) {
                    parts.push(script.to_string());
                }
            }
            _ => {}
        }
        parts.join(":")
    }

    fn command_word_and_rest(words: &[String]) -> Option<(String, Vec<String>)> {
        let mut i = 0usize;
        while i < words.len() && Self::looks_like_env_assignment(&words[i]) {
            i += 1;
        }
        if words.get(i).map(String::as_str) == Some("env") {
            i += 1;
            while i < words.len()
                && (Self::looks_like_env_assignment(&words[i]) || words[i].starts_with('-'))
            {
                i += 1;
            }
        }
        if words.get(i).map(String::as_str) == Some("sudo") {
            i += 1;
            while i < words.len() && words[i].starts_with('-') {
                i += 1;
            }
        }
        let exe = words.get(i)?;
        let exe = std::path::Path::new(exe)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(exe)
            .to_ascii_lowercase();
        let rest = words[i + 1..]
            .iter()
            .map(|s| Self::normalize_cache_token(s))
            .collect();
        Some((exe, rest))
    }

    fn git_subcommand(rest: &[String]) -> Option<String> {
        let mut i = 0usize;
        while i < rest.len() {
            let token = rest[i].as_str();
            if token == "-C" || token == "-c" {
                i += 2;
                continue;
            }
            if token.starts_with('-') {
                i += 1;
                continue;
            }
            return Some(token.to_string());
        }
        None
    }

    fn hash_scoped_shell_command(exe: &str) -> bool {
        matches!(
            exe,
            "rm" | "rmdir"
                | "mv"
                | "cp"
                | "chmod"
                | "chown"
                | "dd"
                | "find"
                | "rsync"
                | "scp"
                | "curl"
                | "wget"
                | "ssh"
                | "nc"
                | "ftp"
                | "open"
                | "osascript"
        )
    }

    fn first_non_option_token(tokens: &[String]) -> Option<&str> {
        tokens
            .iter()
            .map(String::as_str)
            .find(|token| !token.starts_with('-') && !Self::looks_like_env_assignment(token))
    }

    fn token_after<'a>(tokens: &'a [String], needle: &str) -> Option<&'a str> {
        let pos = tokens.iter().position(|token| token == needle)?;
        tokens
            .iter()
            .skip(pos + 1)
            .map(String::as_str)
            .find(|token| !token.starts_with('-') && !Self::looks_like_env_assignment(token))
    }

    fn normalize_cache_token(token: &str) -> String {
        token.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    fn normalize_command_for_cache(command: &str) -> String {
        command
            .trim()
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn looks_like_env_assignment(token: &str) -> bool {
        let Some((name, _)) = token.split_once('=') else {
            return false;
        };
        !name.is_empty()
            && name
                .chars()
                .all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
            && name
                .chars()
                .next()
                .is_some_and(|ch| ch == '_' || ch.is_ascii_alphabetic())
    }

    fn short_hash(value: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(value.as_bytes());
        let full = format!("{:x}", hasher.finalize());
        full.chars().take(16).collect()
    }

    fn has_shell_control_or_redirection(command: &str) -> bool {
        let mut in_single = false;
        let mut in_double = false;
        let mut escaped = false;
        let bytes = command.as_bytes();

        for (i, ch) in command.char_indices() {
            if escaped {
                escaped = false;
                continue;
            }
            if in_double && ch == '\\' {
                escaped = true;
                continue;
            }
            if !in_double && ch == '\'' {
                in_single = !in_single;
                continue;
            }
            if !in_single && ch == '"' {
                in_double = !in_double;
                continue;
            }
            if in_single || in_double {
                continue;
            }
            match ch {
                ';' | '\n' | '|' | '&' | '>' | '`' => return true,
                '$' if bytes.get(i + 1) == Some(&b'(') => return true,
                '<' => return true,
                _ => {}
            }
        }
        false
    }

    fn shell_words(command: &str) -> Vec<String> {
        let mut words = Vec::new();
        let mut current = String::new();
        let mut in_single = false;
        let mut in_double = false;
        let mut escaped = false;

        for ch in command.chars() {
            if escaped {
                current.push(ch);
                escaped = false;
                continue;
            }
            if in_double && ch == '\\' {
                escaped = true;
                continue;
            }
            if !in_double && ch == '\'' {
                in_single = !in_single;
                continue;
            }
            if !in_single && ch == '"' {
                in_double = !in_double;
                continue;
            }
            if !in_single && !in_double && ch.is_whitespace() {
                if !current.is_empty() {
                    words.push(std::mem::take(&mut current));
                }
                continue;
            }
            current.push(ch);
        }
        if !current.is_empty() {
            words.push(current);
        }
        words
    }

    /// 使用 SHA-256 计算工具+参数哈希（仅单元测试校验 canonical 行为）
    #[cfg(test)]
    fn compute_tool_hash(tool_name: &str, arguments: &serde_json::Value) -> String {
        let mut hasher = Sha256::new();
        hasher.update(tool_name.as_bytes());
        hasher.update(b"\x00"); // 分隔符防止 "a"+"bc" == "ab"+"c"

        // 使用 canonical JSON 序列化，确保键按字母顺序排序
        let canonical = Self::canonicalize_json(arguments);
        hasher.update(canonical.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    #[cfg(test)]
    fn canonicalize_json(value: &serde_json::Value) -> String {
        match value {
            serde_json::Value::Object(map) => {
                let mut sorted: Vec<(&String, &serde_json::Value)> = map.iter().collect();
                sorted.sort_by(|a, b| a.0.cmp(b.0));
                let obj: serde_json::Map<String, serde_json::Value> = sorted
                    .into_iter()
                    .map(|(k, v)| (k.clone(), Self::canonicalize_value(v)))
                    .collect();
                serde_json::Value::Object(obj).to_string()
            }
            _ => value.to_string(),
        }
    }

    #[cfg(test)]
    fn canonicalize_value(value: &serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::Object(map) => {
                let mut sorted: Vec<(&String, &serde_json::Value)> = map.iter().collect();
                sorted.sort_by(|a, b| a.0.cmp(b.0));
                let obj: serde_json::Map<String, serde_json::Value> = sorted
                    .into_iter()
                    .map(|(k, v)| (k.clone(), Self::canonicalize_value(v)))
                    .collect();
                serde_json::Value::Object(obj)
            }
            serde_json::Value::Array(arr) => {
                serde_json::Value::Array(arr.iter().map(Self::canonicalize_value).collect())
            }
            other => other.clone(),
        }
    }

    /// 本会话 / 时间窗口 / 单次「记住」是否已覆盖该批准单元
    async fn is_remembered_tool_allowed(&self, session_id: &str, tool_key: &str) -> bool {
        let session_key = format!("{}:{}", session_id, tool_key);
        let now = chrono::Utc::now();

        {
            let approvals = self.session_approvals.read().await;
            if let Some(session_approved) = approvals.get(session_id) {
                if session_approved.contains(tool_key) {
                    return true;
                }
            }
        }

        // 检查时间窗口批准（绑定到特定 session）
        {
            let mut windows = self.window_approvals.write().await;

            // 惰性清理：移除所有已过期的条目（retain 为 true 表示保留）
            windows.retain(|_key, expire_at| now < *expire_at);

            if let Some(expire_at) = windows.get(&session_key) {
                if now < *expire_at {
                    return true;
                }
            }
        }

        // 检查单次批准（AskEveryTime）
        {
            let mut once = self.once_approvals.write().await;

            once.retain(|_key, expire_at| now < *expire_at);

            if let Some(expire_at) = once.get(&session_key) {
                if now < *expire_at {
                    // 使用后立即删除（真正的单次使用）
                    once.remove(&session_key);
                    return true;
                }
            }
        }

        false
    }

    async fn remember_once_approval(&self, session_id: &str, tool_key: &str) {
        let expire_at = chrono::Utc::now() + chrono::Duration::seconds(30);
        let mut once = self.once_approvals.write().await;
        let session_key = format!("{}:{}", session_id, tool_key);
        once.insert(session_key, expire_at);
    }

    /// 递增规则使用计数（同时更新 last_used_at）
    async fn increment_use_count(&self, rule_id: &str) {
        let mut rules = self.rules.write().await;
        if let Some(rule) = rules.iter_mut().find(|r| r.id == rule_id) {
            rule.use_count = rule.use_count.saturating_add(1);
            rule.last_used_at = Some(chrono::Utc::now());
        }
    }

    /// 应用规则，返回权限决定
    async fn apply_rule(
        &self,
        rule: &PermissionRule,
        context: &PermissionContext,
        risk: &RiskAssessment,
    ) -> PermissionDecision {
        // 检查规则有效期
        let is_valid = match &rule.validity {
            RuleValidity::Permanent => true,
            RuleValidity::Until(time) => chrono::Utc::now() < *time,
            RuleValidity::UseLimit(limit) => rule.use_count < *limit,
            RuleValidity::CurrentSession { session_id } => {
                // 仅对创建规则时所在会话有效
                &context.session_id == session_id
            }
        };

        if !is_valid {
            // 规则已过期 → 回落到默认决定；规则过期时允许工作区智能放行
            return self.default_decision(context, risk, true).await;
        }

        let tool_key = Self::approval_cache_key_for_context(context);

        match rule.mode {
            PermissionMode::AskEveryTime => {
                PermissionDecision::RequireApproval(Box::new(PermissionRequest {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    context: context.clone(),
                    risk: risk.clone(),
                    suggested_mode: PermissionMode::Session,
                }))
            }
            PermissionMode::Session | PermissionMode::Plan => {
                if self
                    .is_remembered_tool_allowed(&context.session_id, &tool_key)
                    .await
                {
                    PermissionDecision::Allow
                } else {
                    PermissionDecision::RequireApproval(Box::new(PermissionRequest {
                        request_id: uuid::Uuid::new_v4().to_string(),
                        context: context.clone(),
                        risk: risk.clone(),
                        suggested_mode: rule.mode,
                    }))
                }
            }
            PermissionMode::TimeWindow { .. } => {
                if self
                    .is_remembered_tool_allowed(&context.session_id, &tool_key)
                    .await
                {
                    PermissionDecision::Allow
                } else {
                    PermissionDecision::RequireApproval(Box::new(PermissionRequest {
                        request_id: uuid::Uuid::new_v4().to_string(),
                        context: context.clone(),
                        risk: risk.clone(),
                        suggested_mode: rule.mode,
                    }))
                }
            }
            PermissionMode::Auto => {
                if risk.level >= RiskLevel::High {
                    PermissionDecision::RequireApproval(Box::new(PermissionRequest {
                        request_id: uuid::Uuid::new_v4().to_string(),
                        context: context.clone(),
                        risk: risk.clone(),
                        suggested_mode: PermissionMode::AskEveryTime,
                    }))
                } else {
                    PermissionDecision::Allow
                }
            }
            PermissionMode::Bypass => PermissionDecision::Allow,
        }
    }

    /// 与规则引擎 `PermissionMode::Auto` 一致：仅 High 及以上需确认（Critical 已在 `check_permission` 前段单独处理）。
    fn composer_auto_fallback(
        &self,
        context: &PermissionContext,
        risk: &RiskAssessment,
    ) -> PermissionDecision {
        // 工作区内的非破坏性操作也在 Auto 模式下放行
        if self.is_workspace_safe(context) {
            return PermissionDecision::Allow;
        }

        if risk.level >= RiskLevel::High {
            PermissionDecision::RequireApproval(Box::new(PermissionRequest {
                request_id: uuid::Uuid::new_v4().to_string(),
                context: context.clone(),
                risk: risk.clone(),
                suggested_mode: PermissionMode::AskEveryTime,
            }))
        } else {
            PermissionDecision::Allow
        }
    }

    /// 尽可能放行；Critical 已在 `check_permission` 步骤 3 强制确认，此处不再出现 Critical。
    fn composer_bypass_fallback(&self) -> PermissionDecision {
        PermissionDecision::Allow
    }

    /// 默认决定（无规则匹配时）
    ///
    /// `allow_workspace_bypass`：是否允许工作区智能放行。
    /// - `true`（无明确立场）：工作区内非破坏性写操作自动放行
    /// - `false`（用户设置了 Ask 模式）：尊重用户意图，跳过工作区放行逻辑，回落到风险等级判断
    async fn default_decision(
        &self,
        context: &PermissionContext,
        risk: &RiskAssessment,
        allow_workspace_bypass: bool,
    ) -> PermissionDecision {
        // 工作区智能放行：仅在用户未明确要求"每次询问"时启用
        if allow_workspace_bypass && self.is_workspace_safe(context) {
            return PermissionDecision::Allow;
        }

        match risk.level {
            // Safe 和 Low 风险自动允许（读取文件、网络搜索等）
            RiskLevel::Safe | RiskLevel::Low => PermissionDecision::Allow,
            // Medium 风险（文件写入、编辑）需要确认
            RiskLevel::Medium => PermissionDecision::RequireApproval(Box::new(PermissionRequest {
                request_id: uuid::Uuid::new_v4().to_string(),
                context: context.clone(),
                risk: risk.clone(),
                suggested_mode: PermissionMode::Session,
            })),
            // High 和 Critical 风险（删除、系统命令）需要确认，建议使用更严格的模式
            RiskLevel::High | RiskLevel::Critical => {
                PermissionDecision::RequireApproval(Box::new(PermissionRequest {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    context: context.clone(),
                    risk: risk.clone(),
                    suggested_mode: PermissionMode::AskEveryTime,
                }))
            }
        }
    }

    /// 规则匹配
    fn rule_matches(&self, rule: &PermissionRule, context: &PermissionContext) -> bool {
        // 1. 工具名称匹配
        if !self.matches_tool_matcher(&rule.tool_matcher, &context.tool_name) {
            return false;
        }

        // 2. 路径匹配（如果有）
        if let Some(ref path_matcher) = rule.path_matcher {
            let file_paths = context.file_paths.as_deref().unwrap_or(&[]);
            if !file_paths.is_empty() {
                let any_path_matches = file_paths
                    .iter()
                    .any(|p| self.matches_path_matcher(path_matcher, p));
                if !any_path_matches {
                    return false;
                }
            }
            // 如果没有文件路径信息，WithinProject 视为通过，其他路径匹配器视为不通过
            else if !matches!(path_matcher, PathMatcher::WithinProject) {
                return false;
            }
        }

        // 3. 参数条件匹配
        for condition in &rule.argument_conditions {
            if !self.matches_condition(condition, &context.arguments) {
                return false;
            }
        }

        true
    }

    fn matches_tool_matcher(&self, matcher: &ToolMatcher, tool_name: &str) -> bool {
        match matcher {
            ToolMatcher::Exact(name) => tool_name == name,
            ToolMatcher::Wildcard(pattern) => {
                // 将通配符转为有限大小正则（防止 ReDoS）
                let escaped = regex::escape(pattern);
                let regex_str = format!("^{}$", escaped.replace(r"\*", ".*").replace(r"\?", "."));
                regex::RegexBuilder::new(&regex_str)
                    .size_limit(1_000_000)
                    .dfa_size_limit(1_000_000)
                    .build()
                    .map(|re| re.is_match(tool_name))
                    .unwrap_or(false)
            }
            ToolMatcher::Regex(pattern) => regex::RegexBuilder::new(pattern)
                .size_limit(1_000_000)
                .dfa_size_limit(1_000_000)
                .build()
                .map(|re| re.is_match(tool_name))
                .unwrap_or(false),
            ToolMatcher::Any => true,
        }
    }

    fn matches_path_matcher(&self, matcher: &PathMatcher, path: &std::path::Path) -> bool {
        let path_str = path.to_string_lossy();
        match matcher {
            PathMatcher::WithinProject => true,
            PathMatcher::Exact(expected) => path_str == expected.as_str(),
            PathMatcher::Prefix(prefix) => path_str.starts_with(prefix.as_str()),
            PathMatcher::Glob(pattern) => globset::GlobBuilder::new(pattern)
                .case_insensitive(false)
                .build()
                .and_then(|g| {
                    let mut builder = globset::GlobSetBuilder::new();
                    builder.add(g);
                    builder.build()
                })
                .map(|gs| gs.is_match(path))
                .unwrap_or(false),
            PathMatcher::Regex(pattern) => regex::RegexBuilder::new(pattern)
                .size_limit(1_000_000)
                .dfa_size_limit(1_000_000)
                .build()
                .map(|re| re.is_match(&path_str))
                .unwrap_or(false),
            PathMatcher::FileExtension(exts) => {
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    exts.iter().any(|e| e == ext)
                } else {
                    false
                }
            }
        }
    }

    fn matches_condition(
        &self,
        condition: &ArgumentCondition,
        arguments: &serde_json::Value,
    ) -> bool {
        let arg_value = arguments.get(&condition.key);
        match condition.operator {
            ConditionOperator::Eq => arg_value == Some(&condition.value),
            ConditionOperator::Ne => arg_value != Some(&condition.value),
            ConditionOperator::Contains => match (arg_value, condition.value.as_str()) {
                (Some(serde_json::Value::String(s)), Some(pattern)) => s.contains(pattern),
                (Some(serde_json::Value::Array(arr)), _) => arr.contains(&condition.value),
                _ => false,
            },
            ConditionOperator::StartsWith => match (arg_value, condition.value.as_str()) {
                (Some(serde_json::Value::String(s)), Some(prefix)) => s.starts_with(prefix),
                _ => false,
            },
            ConditionOperator::Matches => match (arg_value, condition.value.as_str()) {
                (Some(serde_json::Value::String(s)), Some(pattern)) => {
                    regex::RegexBuilder::new(pattern)
                        .size_limit(1_000_000)
                        .dfa_size_limit(1_000_000)
                        .build()
                        .map(|re| re.is_match(s))
                        .unwrap_or(false)
                }
                _ => false,
            },
            ConditionOperator::In => match (arg_value, condition.value.as_array()) {
                (Some(val), Some(arr)) => arr.contains(val),
                _ => false,
            },
        }
    }

    // assess_risk, assess_tool_risk, extract_file_paths → risk_assessment.rs
    // canonicalize_best_effort, is_workspace_safe     → workspace_guard.rs
}

impl Default for PermissionManager {
    fn default() -> Self {
        Self::new()
    }
}


#[cfg(test)]
mod tests;
