//! 权限管理器核心实现

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
        }
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
            Some(ComposerPermissionStance::Ask) | None => {
                self.default_decision(context, &risk).await
            }
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
            return self.default_decision(context, risk).await;
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
    async fn default_decision(
        &self,
        context: &PermissionContext,
        risk: &RiskAssessment,
    ) -> PermissionDecision {
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

    /// 风险评估
    async fn assess_risk(&self, context: &PermissionContext) -> RiskAssessment {
        let mut detected_risks = Vec::new();
        let mut categories = Vec::new();

        // 1. 工具级别风险
        let tool_risk = self.assess_tool_risk(&context.tool_name);
        detected_risks.extend(tool_risk.detected_risks);
        categories.extend(tool_risk.categories);

        // 1b. Connector 写操作具有外部服务副作用；只有模型显式声明
        // confirm_write=true 时才进入统一 UI 审批。未声明确认的写操作会先由
        // connector 工具自身拦截并记录为 blocked，避免无意义地弹审批框。
        if Self::approval_cache_key(&context.tool_name) == "connector"
            && connectors::connector_permission_write_confirmed(&context.arguments)
            && connectors::connector_permission_write_operation_from_args(&context.arguments)
                .is_some()
        {
            let (connector_id, operation) =
                connectors::connector_permission_identity_from_args(&context.arguments)
                    .unwrap_or_else(|| ("unknown".to_string(), "unknown".to_string()));
            detected_risks.push(DetectedRisk {
                category: RiskCategory::Network,
                severity: RiskLevel::Critical,
                description: format!("Connector 写操作将修改外部服务: {connector_id}/{operation}"),
                mitigation: Some(
                    "请确认目标、内容和账号无误；批准后仍会写入 connector 审计日志。".to_string(),
                ),
            });
            detected_risks.push(DetectedRisk {
                category: RiskCategory::Privacy,
                severity: RiskLevel::Medium,
                description: "外部服务可能接收当前对话生成的内容".to_string(),
                mitigation: Some("避免发送 secret、token、未确认的私有信息。".to_string()),
            });
            categories.push(RiskCategory::Network);
            categories.push(RiskCategory::Privacy);
        }

        // 2. 参数级别风险（bash/shell 危险命令检测）
        if context.tool_name == "bash" || context.tool_name == "shell" {
            if let Some(cmd) = context.arguments.get("command").and_then(|v| v.as_str()) {
                let pattern_risks = self.patterns.check(cmd);
                for risk in &pattern_risks {
                    categories.push(risk.category.clone());
                }
                detected_risks.extend(pattern_risks);
            }
        }

        // 3. 路径风险
        let home_dir = dirs::home_dir();
        if let Some(ref paths) = context.file_paths {
            for path in paths {
                let path_str = path.to_string_lossy();

                // 3a. 系统路径
                if path_str.starts_with("/etc/")
                    || path_str.starts_with("/boot/")
                    || path_str.starts_with("/sys/")
                {
                    detected_risks.push(DetectedRisk {
                        category: RiskCategory::System,
                        severity: RiskLevel::High,
                        description: format!("访问系统路径: {}", path_str),
                        mitigation: Some("确认是否真的需要修改系统文件".to_string()),
                    });
                    categories.push(RiskCategory::System);
                }

                // 3b. 敏感文件
                if path_str.contains(".env")
                    || path_str.contains("secret")
                    || path_str.contains("credential")
                {
                    detected_risks.push(DetectedRisk {
                        category: RiskCategory::Privacy,
                        severity: RiskLevel::Medium,
                        description: format!("可能涉及敏感文件: {}", path_str),
                        mitigation: Some("确认是否需要修改此文件".to_string()),
                    });
                    categories.push(RiskCategory::Privacy);
                }

                // 3c. 项目根目录之外的写操作
                if let Some(ref root) = context.project_root {
                    let abs_path = if path.is_absolute() {
                        path.clone()
                    } else {
                        root.join(path)
                    };
                    let canonical_root = std::fs::canonicalize(root).unwrap_or(root.clone());
                    let canonical_path =
                        std::fs::canonicalize(&abs_path).unwrap_or(abs_path.clone());

                    if !canonical_path.starts_with(&canonical_root) {
                        // 判断是否直接在 home 目录下（更危险）
                        let is_home_level = home_dir
                            .as_ref()
                            .map(|h| {
                                let canonical_home = std::fs::canonicalize(h).unwrap_or(h.clone());
                                // 直接子目录或文件（depth == home + 1）
                                canonical_path.starts_with(&canonical_home)
                                    && canonical_path
                                        .strip_prefix(&canonical_home)
                                        .map(|rel| rel.components().count() <= 1)
                                        .unwrap_or(false)
                            })
                            .unwrap_or(false);

                        let (severity, desc) = if is_home_level {
                            (
                                RiskLevel::High,
                                format!(
                                    "操作路径在 Home 目录根层级 (~/)，超出项目范围: {}",
                                    path_str
                                ),
                            )
                        } else {
                            (
                                RiskLevel::Medium,
                                format!("操作路径超出项目目录: {}", path_str),
                            )
                        };
                        detected_risks.push(DetectedRisk {
                            category: RiskCategory::FileSystem,
                            severity,
                            description: desc,
                            mitigation: Some(format!(
                                "项目根目录为 {}，请确认是否允许访问外部路径",
                                root.display()
                            )),
                        });
                        categories.push(RiskCategory::FileSystem);
                    }
                }
            }
        }

        // 3d. bash 命令中的路径越界检测（mkdir / touch / cp 等写入外部路径）
        if let (Some(ref root), true) = (
            &context.project_root,
            context.tool_name == "bash" || context.tool_name == "shell",
        ) {
            if let Some(cmd) = context.arguments.get("command").and_then(|v| v.as_str()) {
                for cap in bash_path_regex().captures_iter(cmd) {
                    let raw = cap.get(1).map(|m| m.as_str()).unwrap_or_default();
                    // Expand ~ to home dir
                    let expanded = if let Some(stripped) = raw.strip_prefix("~/") {
                        home_dir
                            .as_ref()
                            .map(|h| h.join(stripped))
                            .unwrap_or_else(|| std::path::PathBuf::from(raw))
                    } else if raw == "~" {
                        home_dir
                            .clone()
                            .unwrap_or_else(|| std::path::PathBuf::from(raw))
                    } else {
                        std::path::PathBuf::from(raw)
                    };

                    if !expanded.is_absolute() {
                        continue;
                    }

                    let canonical_root = std::fs::canonicalize(root).unwrap_or(root.clone());
                    let canonical_path =
                        std::fs::canonicalize(&expanded).unwrap_or(expanded.clone());

                    if !canonical_path.starts_with(&canonical_root) {
                        let is_home_level = home_dir
                            .as_ref()
                            .map(|h| {
                                let ch = std::fs::canonicalize(h).unwrap_or(h.clone());
                                canonical_path.starts_with(&ch)
                                    && canonical_path
                                        .strip_prefix(&ch)
                                        .map(|rel| rel.components().count() <= 1)
                                        .unwrap_or(false)
                            })
                            .unwrap_or(false);

                        if is_home_level {
                            detected_risks.push(DetectedRisk {
                                category: RiskCategory::FileSystem,
                                severity: RiskLevel::High,
                                description: format!(
                                    "命令中包含 Home 根层级路径 (~/)，超出项目范围: {}",
                                    raw
                                ),
                                mitigation: Some(format!(
                                    "项目根目录为 {}，该路径超出允许范围",
                                    root.display()
                                )),
                            });
                            categories.push(RiskCategory::FileSystem);
                        }
                    }
                }
            }
        }

        // 计算总体风险等级
        let max_level = detected_risks
            .iter()
            .map(|r| r.severity)
            .max()
            .unwrap_or(RiskLevel::Safe);

        let description = if detected_risks.is_empty() {
            format!("使用工具: {}", context.tool_name)
        } else {
            format!("检测到 {} 个风险点", detected_risks.len())
        };

        let recommendations: Vec<String> = detected_risks
            .iter()
            .filter_map(|r| r.mitigation.clone())
            .collect();

        categories.sort();
        categories.dedup();

        RiskAssessment {
            level: max_level,
            categories,
            description,
            recommendations,
            detected_risks,
        }
    }

    fn assess_tool_risk(&self, tool_name: &str) -> RiskAssessment {
        match tool_name {
            "bash" | "shell" => RiskAssessment {
                level: RiskLevel::High,
                categories: vec![RiskCategory::System],
                description: "执行系统命令".to_string(),
                recommendations: vec![
                    "仔细检查命令内容".to_string(),
                    "避免使用 rm -rf 等危险命令".to_string(),
                ],
                detected_risks: vec![DetectedRisk {
                    category: RiskCategory::System,
                    severity: RiskLevel::High,
                    description: "允许执行任意系统命令".to_string(),
                    mitigation: Some("使用受限的 file_* 工具替代".to_string()),
                }],
            },
            "file_write" | "file_edit" | "skill_manage" | "skill_config" => RiskAssessment {
                level: RiskLevel::Medium,
                categories: vec![RiskCategory::FileSystem, RiskCategory::DataLoss],
                description: "修改文件内容".to_string(),
                recommendations: vec![
                    "确认文件路径正确".to_string(),
                    "重要文件建议先备份".to_string(),
                ],
                detected_risks: vec![DetectedRisk {
                    category: RiskCategory::FileSystem,
                    severity: RiskLevel::Medium,
                    description: "将修改文件内容".to_string(),
                    mitigation: Some("使用 file_read 先查看当前内容".to_string()),
                }],
            },
            "file_read" | "glob" | "grep" | "read_mcp_resource" => RiskAssessment {
                level: RiskLevel::Safe,
                categories: vec![],
                description: "读取操作（安全）".to_string(),
                recommendations: vec![],
                detected_risks: vec![],
            },
            "connector" | "fetch" | "query" | "search" => RiskAssessment {
                level: RiskLevel::Low,
                categories: vec![RiskCategory::Network],
                description: "网络请求".to_string(),
                recommendations: vec!["确认 URL 可信".to_string()],
                detected_risks: vec![DetectedRisk {
                    category: RiskCategory::Network,
                    severity: RiskLevel::Low,
                    description: "将访问外部网络".to_string(),
                    mitigation: None,
                }],
            },
            // 其他常见安全工具
            "list_skills" | "skills_list" | "skill_view" | "tool_search" | "get_current_time"
            | "get_system_info" => RiskAssessment {
                level: RiskLevel::Safe,
                categories: vec![],
                description: format!("使用工具: {}", tool_name),
                recommendations: vec![],
                detected_risks: vec![],
            },
            // 默认：未知工具视为 Medium 风险（需要确认）
            _ => RiskAssessment {
                level: RiskLevel::Medium,
                categories: vec![],
                description: format!("使用工具: {}", tool_name),
                recommendations: vec!["未知工具，请确认是否允许执行".to_string()],
                detected_risks: vec![DetectedRisk {
                    category: RiskCategory::Security,
                    severity: RiskLevel::Medium,
                    description: format!("未识别的工具: {}", tool_name),
                    mitigation: Some("如果是 Skill 工具，可以在权限设置中添加规则".to_string()),
                }],
            },
        }
    }

    /// 从工具参数中提取文件路径
    fn extract_file_paths(
        &self,
        tool_name: &str,
        arguments: &serde_json::Value,
    ) -> Option<Vec<std::path::PathBuf>> {
        let mut paths = Vec::new();

        match tool_name {
            "file_read" | "file_write" | "file_edit" => {
                if let Some(path) = arguments.get("path").and_then(|v| v.as_str()) {
                    paths.push(std::path::PathBuf::from(path));
                }
            }
            "bash" | "shell" => {
                if let Some(cmd) = arguments.get("command").and_then(|v| v.as_str()) {
                    // 使用预编译的静态正则
                    for cap in bash_path_regex().captures_iter(cmd) {
                        if let Some(m) = cap.get(1) {
                            paths.push(std::path::PathBuf::from(m.as_str()));
                        }
                    }
                }
            }
            _ => {}
        }

        if paths.is_empty() {
            None
        } else {
            Some(paths)
        }
    }
}

impl Default for PermissionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // 基础决策测试
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn test_safe_read_is_allowed() {
        let mgr = PermissionManager::new();
        let dec = mgr
            .check_tool("s1", "file_read", &serde_json::json!({"path": "README.md"}))
            .await;
        assert!(
            matches!(dec, PermissionDecision::Allow),
            "file_read should be Allow"
        );
    }

    #[tokio::test]
    async fn test_dangerous_command_requires_approval() {
        let mgr = PermissionManager::new();
        let dec = mgr
            .check_tool("s1", "bash", &serde_json::json!({"command": "rm -rf /"}))
            .await;
        assert!(
            matches!(dec, PermissionDecision::RequireApproval(_)),
            "rm -rf / should require approval"
        );
    }

    #[tokio::test]
    async fn connector_confirmed_write_requires_ui_approval() {
        let mgr = PermissionManager::new();
        let args = serde_json::json!({
            "connector": "slack",
            "operation": "post_message",
            "channel": "C123",
            "text": "Ship it",
            "confirm_write": true
        });

        let dec = mgr.check_tool("s_connector", "connector", &args).await;
        let req = match dec {
            PermissionDecision::RequireApproval(req) => req,
            other => panic!("expected connector write approval, got {other:?}"),
        };
        assert_eq!(req.risk.level, RiskLevel::Critical);
        assert!(req
            .risk
            .detected_risks
            .iter()
            .any(|risk| risk.description.contains("slack/post_message")));

        mgr.approve_request("s_connector", PermissionMode::AskEveryTime, &req.context)
            .await
            .unwrap();

        let allowed_once = mgr.check_tool("s_connector", "connector", &args).await;
        assert!(matches!(allowed_once, PermissionDecision::Allow));

        let requires_again = mgr.check_tool("s_connector", "connector", &args).await;
        assert!(matches!(
            requires_again,
            PermissionDecision::RequireApproval(_)
        ));
    }

    #[tokio::test]
    async fn connector_write_approval_is_scoped_to_connector_operation() {
        let mgr = PermissionManager::new();
        let slack_post = serde_json::json!({
            "connector": "slack",
            "operation": "post_message",
            "channel": "C123",
            "text": "Ship it",
            "confirmWrite": true
        });
        let req = match mgr
            .check_tool("s_connector_scope", "Connector", &slack_post)
            .await
        {
            PermissionDecision::RequireApproval(req) => req,
            other => panic!("expected slack write approval, got {other:?}"),
        };
        mgr.approve_request("s_connector_scope", PermissionMode::Session, &req.context)
            .await
            .unwrap();
        assert!(matches!(
            mgr.check_tool("s_connector_scope", "connector", &slack_post)
                .await,
            PermissionDecision::Allow
        ));

        let linear_write = serde_json::json!({
            "connector": "linear",
            "operation": "update_issue_status",
            "id": "ENG-1",
            "confirm_write": true
        });
        assert!(matches!(
            mgr.check_tool("s_connector_scope", "connector", &linear_write)
                .await,
            PermissionDecision::RequireApproval(_)
        ));
    }

    #[tokio::test]
    async fn unconfirmed_connector_write_reaches_tool_level_guard_without_ui_prompt() {
        let mgr = PermissionManager::new();
        let dec = mgr
            .check_tool(
                "s_connector_unconfirmed",
                "connector",
                &serde_json::json!({
                    "connector": "slack",
                    "operation": "post_message",
                    "channel": "C123",
                    "text": "Ship it"
                }),
            )
            .await;
        assert!(
            matches!(dec, PermissionDecision::Allow),
            "missing confirm_write should be blocked by connector tool guard, not a UI prompt"
        );
    }

    // -------------------------------------------------------------------------
    // 会话拒绝测试
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn test_deny_blocks_subsequent_calls() {
        let mgr = PermissionManager::new();
        let ctx = PermissionContext {
            tool_name: "bash".to_string(),
            arguments: serde_json::json!({"command": "git status --short"}),
            session_id: "session_deny_test".to_string(),
            file_paths: None,
            timestamp: chrono::Utc::now(),
            project_root: None,
        };

        // 第一次拒绝
        mgr.deny_request(&ctx, "用户拒绝").await.unwrap();

        // 后续同类命令应直接返回 Deny
        let dec = mgr.check_permission(&ctx).await;
        assert!(
            matches!(dec, PermissionDecision::Deny(_)),
            "后续调用应返回 Deny，实际: {:?}",
            dec
        );
        let ctx_same_class = PermissionContext {
            arguments: serde_json::json!({"command": "git status --porcelain"}),
            ..ctx.clone()
        };
        let dec2 = mgr.check_permission(&ctx_same_class).await;
        assert!(
            matches!(dec2, PermissionDecision::Deny(_)),
            "同类 git status 仍应 Deny，实际: {:?}",
            dec2
        );

        let ctx_other_cmd = PermissionContext {
            arguments: serde_json::json!({"command": "git push origin main"}),
            ..ctx.clone()
        };
        let dec3 = mgr.check_permission(&ctx_other_cmd).await;
        assert!(
            !matches!(dec3, PermissionDecision::Deny(_)),
            "不同命令类别不应被 git status 的拒绝覆盖，实际: {:?}",
            dec3
        );
    }

    #[tokio::test]
    async fn test_deny_isolated_to_session() {
        let mgr = PermissionManager::new();
        let ctx_a = PermissionContext {
            tool_name: "bash".to_string(),
            arguments: serde_json::json!({"command": "ls /tmp"}),
            session_id: "session_a".to_string(),
            file_paths: None,
            timestamp: chrono::Utc::now(),
            project_root: None,
        };
        let ctx_b = PermissionContext {
            session_id: "session_b".to_string(),
            ..ctx_a.clone()
        };

        mgr.deny_request(&ctx_a, "用户拒绝").await.unwrap();

        // session_b 不应受影响
        let dec = mgr.check_permission(&ctx_b).await;
        assert!(
            !matches!(dec, PermissionDecision::Deny(_)),
            "不同 session 不应受拒绝影响"
        );
    }

    // -------------------------------------------------------------------------
    // 会话批准测试
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn test_session_approve_bash_is_scoped_to_command_class() {
        let mgr = PermissionManager::new();
        let ctx = PermissionContext {
            tool_name: "bash".to_string(),
            arguments: serde_json::json!({"command": "git status --short"}),
            session_id: "s_approve".to_string(),
            file_paths: None,
            timestamp: chrono::Utc::now(),
            project_root: None,
        };

        mgr.approve_request("s_approve", PermissionMode::Session, &ctx)
            .await
            .unwrap();

        let dec = mgr.check_permission(&ctx).await;
        assert!(
            matches!(dec, PermissionDecision::Allow),
            "批准后应 Allow，实际: {:?}",
            dec
        );
        let ctx_same_class = PermissionContext {
            arguments: serde_json::json!({"command": "git status --porcelain"}),
            ..ctx.clone()
        };
        let dec2 = mgr.check_permission(&ctx_same_class).await;
        assert!(
            matches!(dec2, PermissionDecision::Allow),
            "同类 git status 应共享本会话批准，实际: {:?}",
            dec2
        );

        let ctx_other = PermissionContext {
            arguments: serde_json::json!({"command": "git push origin main"}),
            ..ctx.clone()
        };
        let dec3 = mgr.check_permission(&ctx_other).await;
        assert!(
            matches!(dec3, PermissionDecision::RequireApproval(_)),
            "不同命令类别不应被 git status 的本会话批准覆盖，实际: {:?}",
            dec3
        );
    }

    #[tokio::test]
    async fn test_session_approve_bash_inline_code_does_not_allow_other_code() {
        let mgr = PermissionManager::new();
        let ctx = PermissionContext {
            tool_name: "bash".to_string(),
            arguments: serde_json::json!({
                "command": "python3 - <<'PY'\nprint('safe')\nPY"
            }),
            session_id: "s_inline".to_string(),
            file_paths: None,
            timestamp: chrono::Utc::now(),
            project_root: None,
        };

        mgr.approve_request("s_inline", PermissionMode::Session, &ctx)
            .await
            .unwrap();

        assert!(matches!(
            mgr.check_permission(&ctx).await,
            PermissionDecision::Allow
        ));

        let destructive = PermissionContext {
            arguments: serde_json::json!({"command": "rm -rf /tmp/omiga-inline-test"}),
            ..ctx.clone()
        };
        let dec = mgr.check_permission(&destructive).await;
        assert!(
            matches!(dec, PermissionDecision::RequireApproval(_)),
            "批准一段 inline Python 不应放行其它 bash 代码，实际: {:?}",
            dec
        );
    }

    #[tokio::test]
    async fn test_session_approve_install_command_is_single_use() {
        let mgr = PermissionManager::new();
        let ctx = PermissionContext {
            tool_name: "bash".to_string(),
            arguments: serde_json::json!({"command": "npm install left-pad"}),
            session_id: "s_install_once".to_string(),
            file_paths: None,
            timestamp: chrono::Utc::now(),
            project_root: None,
        };

        mgr.approve_request("s_install_once", PermissionMode::Session, &ctx)
            .await
            .unwrap();

        assert!(matches!(
            mgr.check_permission(&ctx).await,
            PermissionDecision::Allow
        ));
        let second = mgr.check_permission(&ctx).await;
        assert!(
            matches!(second, PermissionDecision::RequireApproval(_)),
            "软件安装即使选择本会话也必须下次重新询问，实际: {:?}",
            second
        );
    }

    #[tokio::test]
    async fn test_session_approve_file_deletion_is_single_use() {
        let mgr = PermissionManager::new();
        let ctx = PermissionContext {
            tool_name: "bash".to_string(),
            arguments: serde_json::json!({"command": "rm -rf /tmp/omiga-delete-test"}),
            session_id: "s_delete_once".to_string(),
            file_paths: None,
            timestamp: chrono::Utc::now(),
            project_root: None,
        };

        mgr.approve_request("s_delete_once", PermissionMode::Session, &ctx)
            .await
            .unwrap();

        assert!(matches!(
            mgr.check_permission(&ctx).await,
            PermissionDecision::Allow
        ));
        let second = mgr.check_permission(&ctx).await;
        assert!(
            matches!(second, PermissionDecision::RequireApproval(_)),
            "文件删除即使选择本会话也必须下次重新询问，实际: {:?}",
            second
        );
    }

    #[tokio::test]
    async fn test_session_approve_wire_name_aliases_merge() {
        let mgr = PermissionManager::new();
        let ctx_read = PermissionContext {
            tool_name: "Read".to_string(),
            arguments: serde_json::json!({"path": "/a"}),
            session_id: "s_alias".to_string(),
            file_paths: Some(vec![std::path::PathBuf::from("/a")]),
            timestamp: chrono::Utc::now(),
            project_root: None,
        };
        mgr.approve_request("s_alias", PermissionMode::Session, &ctx_read)
            .await
            .unwrap();
        let ctx_file_read = PermissionContext {
            tool_name: "file_read".to_string(),
            arguments: serde_json::json!({"path": "/b"}),
            file_paths: Some(vec![std::path::PathBuf::from("/b")]),
            ..ctx_read.clone()
        };
        let dec = mgr.check_permission(&ctx_file_read).await;
        assert!(
            matches!(dec, PermissionDecision::Allow),
            "Read 与 file_read 应共享会话批准，实际: {:?}",
            dec
        );
    }

    #[tokio::test]
    async fn test_session_approve_file_write_different_paths() {
        let mgr = PermissionManager::new();
        let ctx_a = PermissionContext {
            tool_name: "file_write".to_string(),
            arguments: serde_json::json!({"path": "/tmp/a.txt", "content": "x"}),
            session_id: "s_fw".to_string(),
            file_paths: Some(vec![std::path::PathBuf::from("/tmp/a.txt")]),
            timestamp: chrono::Utc::now(),
            project_root: None,
        };
        mgr.approve_request("s_fw", PermissionMode::Session, &ctx_a)
            .await
            .unwrap();
        let ctx_b = PermissionContext {
            arguments: serde_json::json!({"path": "/tmp/b.txt", "content": "y"}),
            file_paths: Some(vec![std::path::PathBuf::from("/tmp/b.txt")]),
            ..ctx_a.clone()
        };
        let dec = mgr.check_permission(&ctx_b).await;
        assert!(
            matches!(dec, PermissionDecision::Allow),
            "file_write 不同路径应共享会话批准，实际: {:?}",
            dec
        );
    }

    /// Critical 风险（patterns 中如直接写磁盘设备）在未批准前走 Critical 分支；
    /// 本会话批准后仅相同命令类别命中缓存，不应覆盖其它 bash 命令。
    #[tokio::test]
    async fn test_session_approve_allows_critical_bash_same_args() {
        let mgr = PermissionManager::new();
        let ctx = PermissionContext {
            tool_name: "bash".to_string(),
            arguments: serde_json::json!({"command": "echo x > /dev/sda"}),
            session_id: "s_crit".to_string(),
            file_paths: None,
            timestamp: chrono::Utc::now(),
            project_root: None,
        };

        let before = mgr.check_permission(&ctx).await;
        assert!(
            matches!(before, PermissionDecision::RequireApproval(_)),
            "未批准时应 RequireApproval"
        );

        mgr.approve_request("s_crit", PermissionMode::Session, &ctx)
            .await
            .unwrap();

        let after = mgr.check_permission(&ctx).await;
        assert!(
            matches!(after, PermissionDecision::Allow),
            "本会话记住后应 Allow，实际: {:?}",
            after
        );
        let ctx_other = PermissionContext {
            arguments: serde_json::json!({"command": "dd if=/dev/zero of=/dev/sda"}),
            ..ctx.clone()
        };
        let after_other = mgr.check_permission(&ctx_other).await;
        assert!(
            matches!(after_other, PermissionDecision::RequireApproval(_)),
            "Critical bash 批准不应覆盖其它命令类别，实际: {:?}",
            after_other
        );
    }

    #[tokio::test]
    async fn test_time_window_approve() {
        let mgr = PermissionManager::new();
        let ctx = PermissionContext {
            tool_name: "bash".to_string(),
            arguments: serde_json::json!({"command": "echo hello"}),
            session_id: "s_tw".to_string(),
            file_paths: None,
            timestamp: chrono::Utc::now(),
            project_root: None,
        };

        mgr.approve_request("s_tw", PermissionMode::TimeWindow { minutes: 60 }, &ctx)
            .await
            .unwrap();

        let dec = mgr.check_permission(&ctx).await;
        assert!(matches!(dec, PermissionDecision::Allow));
        let ctx2 = PermissionContext {
            arguments: serde_json::json!({"command": "echo different"}),
            ..ctx.clone()
        };
        let dec2 = mgr.check_permission(&ctx2).await;
        assert!(
            matches!(dec2, PermissionDecision::Allow),
            "时间窗口内同类命令应 Allow，实际: {:?}",
            dec2
        );
        let ctx3 = PermissionContext {
            arguments: serde_json::json!({"command": "rm -rf /tmp/omiga-time-window-test"}),
            ..ctx.clone()
        };
        let dec3 = mgr.check_permission(&ctx3).await;
        assert!(
            matches!(dec3, PermissionDecision::RequireApproval(_)),
            "时间窗口批准不应覆盖不同 bash 命令类别，实际: {:?}",
            dec3
        );
        let ctx4 = PermissionContext {
            arguments: serde_json::json!({"command": "echo $(rm -rf /tmp/omiga-substitution-test)"}),
            ..ctx.clone()
        };
        let dec4 = mgr.check_permission(&ctx4).await;
        assert!(
            matches!(dec4, PermissionDecision::RequireApproval(_)),
            "普通 echo 批准不应覆盖带命令替换的 shell 代码，实际: {:?}",
            dec4
        );
    }

    #[tokio::test]
    async fn test_approve_bypass_mode_rejected() {
        let mgr = PermissionManager::new();
        let ctx = PermissionContext {
            tool_name: "bash".to_string(),
            arguments: serde_json::json!({}),
            session_id: "s".to_string(),
            file_paths: None,
            timestamp: chrono::Utc::now(),
            project_root: None,
        };
        let result = mgr.approve_request("s", PermissionMode::Bypass, &ctx).await;
        assert!(result.is_err(), "Bypass 模式应被拒绝");
    }

    #[tokio::test]
    async fn test_time_window_overflow_rejected() {
        let mode = PermissionMode::TimeWindow { minutes: u32::MAX };
        assert!(
            mode.validate_user_mode().is_err(),
            "超大 TimeWindow 应被拒绝"
        );
    }

    // -------------------------------------------------------------------------
    // 规则有效期测试
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn test_use_limit_rule_expires() {
        let mgr = PermissionManager::new();

        // 添加一条 UseLimit(1) 的规则：允许 file_write，但只能用1次
        let rule = PermissionRule {
            id: "rule_ul".to_string(),
            name: "限制使用次数".to_string(),
            description: None,
            tool_matcher: ToolMatcher::Exact("file_write".to_string()),
            path_matcher: None,
            argument_conditions: vec![],
            mode: PermissionMode::Auto,
            validity: RuleValidity::UseLimit(1),
            priority: 0,
            created_at: chrono::Utc::now(),
            last_used_at: None,
            use_count: 0,
        };
        mgr.add_rule(rule).await.unwrap();

        let ctx = PermissionContext {
            tool_name: "file_write".to_string(),
            arguments: serde_json::json!({"path": "/tmp/test.txt"}),
            session_id: "s_ul".to_string(),
            file_paths: Some(vec![std::path::PathBuf::from("/tmp/test.txt")]),
            timestamp: chrono::Utc::now(),
            project_root: None,
        };

        // 第一次：规则有效（use_count=0 < limit=1），Auto + Medium risk → Allow
        let dec1 = mgr.check_permission(&ctx).await;
        assert!(matches!(dec1, PermissionDecision::Allow), "第一次应 Allow");

        // 第二次：规则已失效（use_count=1 >= limit=1），走 default_decision
        let dec2 = mgr.check_permission(&ctx).await;
        // file_write 在 assess_tool_risk 中为 Medium，default_decision 对 Medium 为 RequireApproval
        assert!(
            matches!(dec2, PermissionDecision::RequireApproval(_)),
            "规则失效后应按默认策略：Medium 风险 RequireApproval，实际: {:?}",
            dec2
        );
    }

    #[tokio::test]
    async fn test_current_session_rule_isolated() {
        let mgr = PermissionManager::new();

        let rule = PermissionRule {
            id: "rule_cs".to_string(),
            name: "当前会话规则".to_string(),
            description: None,
            tool_matcher: ToolMatcher::Exact("file_write".to_string()),
            path_matcher: None,
            argument_conditions: vec![],
            mode: PermissionMode::Auto,
            validity: RuleValidity::CurrentSession {
                session_id: "session_owner".to_string(),
            },
            priority: 0,
            created_at: chrono::Utc::now(),
            last_used_at: None,
            use_count: 0,
        };
        mgr.add_rule(rule).await.unwrap();

        let write_args = serde_json::json!({"path": "/tmp/current-session-test.txt"});

        // 规则创建者的会话：file_write 是 Medium 风险，Auto 规则 → Allow（Low/Medium 允许）
        let ctx_owner = PermissionContext {
            tool_name: "file_write".to_string(),
            arguments: write_args.clone(),
            session_id: "session_owner".to_string(),
            file_paths: Some(vec![std::path::PathBuf::from(
                "/tmp/current-session-test.txt",
            )]),
            timestamp: chrono::Utc::now(),
            project_root: None,
        };
        let dec_owner = mgr.check_permission(&ctx_owner).await;
        assert!(
            matches!(dec_owner, PermissionDecision::Allow),
            "规则所有者 session 应 Allow"
        );

        // 其他会话：规则无效，走 default_decision → RequireApproval
        let ctx_other = PermissionContext {
            session_id: "session_other".to_string(),
            ..ctx_owner.clone()
        };
        let dec_other = mgr.check_permission(&ctx_other).await;
        assert!(
            matches!(dec_other, PermissionDecision::RequireApproval(_)),
            "其他 session 不应受 CurrentSession 规则影响"
        );
    }

    // -------------------------------------------------------------------------
    // 规则匹配测试
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn test_wildcard_matcher() {
        let mgr = PermissionManager::new();
        assert!(mgr.matches_tool_matcher(&ToolMatcher::Wildcard("file_*".to_string()), "file_read"));
        assert!(
            mgr.matches_tool_matcher(&ToolMatcher::Wildcard("file_*".to_string()), "file_write")
        );
        assert!(!mgr.matches_tool_matcher(&ToolMatcher::Wildcard("file_*".to_string()), "bash"));
    }

    #[tokio::test]
    async fn test_path_matcher_prefix() {
        let mgr = PermissionManager::new();
        let path = std::path::Path::new("/tmp/test.txt");
        assert!(mgr.matches_path_matcher(&PathMatcher::Prefix("/tmp/".to_string()), path));
        assert!(!mgr.matches_path_matcher(&PathMatcher::Prefix("/etc/".to_string()), path));
    }

    #[tokio::test]
    async fn test_path_matcher_extension() {
        let mgr = PermissionManager::new();
        let path = std::path::Path::new("/tmp/test.rs");
        assert!(mgr.matches_path_matcher(
            &PathMatcher::FileExtension(vec!["rs".to_string(), "toml".to_string()]),
            path
        ));
        assert!(
            !mgr.matches_path_matcher(&PathMatcher::FileExtension(vec!["py".to_string()]), path)
        );
    }

    #[tokio::test]
    async fn test_condition_ne() {
        let mgr = PermissionManager::new();
        let cond = ArgumentCondition {
            key: "cmd".to_string(),
            operator: ConditionOperator::Ne,
            value: serde_json::json!("rm -rf /"),
        };
        assert!(mgr.matches_condition(&cond, &serde_json::json!({"cmd": "ls"})));
        assert!(!mgr.matches_condition(&cond, &serde_json::json!({"cmd": "rm -rf /"})));
    }

    #[tokio::test]
    async fn test_condition_contains() {
        let mgr = PermissionManager::new();
        let cond = ArgumentCondition {
            key: "command".to_string(),
            operator: ConditionOperator::Contains,
            value: serde_json::json!("--force"),
        };
        assert!(mgr.matches_condition(&cond, &serde_json::json!({"command": "git push --force"})));
        assert!(!mgr.matches_condition(&cond, &serde_json::json!({"command": "git push"})));
    }

    #[tokio::test]
    async fn test_hash_no_prefix_collision() {
        // "a" + "bc" 与 "ab" + "c" 应产生不同 hash
        let h1 = PermissionManager::compute_tool_hash("a", &serde_json::json!("bc"));
        let h2 = PermissionManager::compute_tool_hash("ab", &serde_json::json!("c"));
        assert_ne!(h1, h2);
    }

    #[tokio::test]
    async fn test_hash_canonical_json_key_order() {
        // 键顺序不同但内容相同的 JSON 应产生相同 hash
        let args1 = serde_json::json!({"a": 1, "b": 2, "c": 3});
        let args2 = serde_json::json!({"c": 3, "a": 1, "b": 2});
        let h1 = PermissionManager::compute_tool_hash("test", &args1);
        let h2 = PermissionManager::compute_tool_hash("test", &args2);
        assert_eq!(
            h1, h2,
            "Canonical JSON should produce same hash regardless of key order"
        );
    }

    #[tokio::test]
    async fn test_hash_canonical_json_nested() {
        // 嵌套对象也应正确处理
        let args1 = serde_json::json!({"outer": {"a": 1, "b": 2}});
        let args2 = serde_json::json!({"outer": {"b": 2, "a": 1}});
        let h1 = PermissionManager::compute_tool_hash("test", &args1);
        let h2 = PermissionManager::compute_tool_hash("test", &args2);
        assert_eq!(h1, h2, "Nested objects should be canonicalized");
    }

    // -------------------------------------------------------------------------
    // 审计日志测试
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn test_denial_audit_log() {
        let mgr = PermissionManager::new();
        let ctx = PermissionContext {
            tool_name: "bash".to_string(),
            arguments: serde_json::json!({"command": "rm -rf /"}),
            session_id: "s_audit".to_string(),
            file_paths: None,
            timestamp: chrono::Utc::now(),
            project_root: None,
        };

        mgr.deny_request(&ctx, "用户明确拒绝").await.unwrap();

        let denials = mgr.get_recent_denials(10).await;
        assert_eq!(denials.len(), 1);
        assert_eq!(denials[0].tool_name, "bash");
        assert_eq!(denials[0].reason, "用户明确拒绝");
    }
}
