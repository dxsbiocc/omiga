//! 权限管理器核心实现

use super::patterns::DangerousPatternDB;
use super::types::*;
use super::tool_rules::canonical_permission_tool_name;
#[cfg(test)]
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
    /// 会话级批准缓存: session_id → 规范化工具名（忽略参数，同一会话内同一工具只问一次）
    session_approvals: Arc<RwLock<HashMap<String, HashSet<String>>>>,
    /// 时间窗口批准: "session_id:tool_key" → expire_time
    window_approvals: Arc<RwLock<HashMap<String, chrono::DateTime<chrono::Utc>>>>,
    /// 单次批准: "session_id:tool_key" → expire_time（30 秒内有效）
    once_approvals: Arc<RwLock<HashMap<String, chrono::DateTime<chrono::Utc>>>>,
    /// 会话级拒绝: session_id → 规范化工具名（拒绝后本会话内该工具一律不再询问）
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
        let tool_key = Self::approval_cache_key(&context.tool_name);

        // 0. 检查本会话是否已主动拒绝此工具（按工具名，不区分参数）
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
        if self.is_remembered_tool_allowed(&context.session_id, &tool_key).await {
            return PermissionDecision::Allow;
        }

        // 2. 风险评估
        let risk = self.assess_risk(context).await;

        // 3. Critical 风险立即要求确认（未命中上述批准缓存时）
        if risk.level == RiskLevel::Critical {
            return PermissionDecision::RequireApproval(PermissionRequest {
                request_id: uuid::Uuid::new_v4().to_string(),
                context: context.clone(),
                risk,
                suggested_mode: PermissionMode::AskEveryTime,
            });
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
        let context = PermissionContext {
            tool_name: tool_name.to_string(),
            arguments: arguments.clone(),
            session_id: session_id.to_string(),
            file_paths: self.extract_file_paths(tool_name, arguments),
            timestamp: chrono::Utc::now(),
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

        let tool_key = Self::approval_cache_key(&context.tool_name);

        // 批准时移除本会话内对该工具的拒绝记录（用户改变了主意）
        {
            let mut denials = self.session_denials.write().await;
            if let Some(session_denied) = denials.get_mut(session_id) {
                session_denied.remove(&tool_key);
            }
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
                let expire_at = chrono::Utc::now()
                    + chrono::Duration::minutes(minutes as i64);
                let mut windows = self.window_approvals.write().await;
                let session_key = format!("{}:{}", session_id, tool_key);
                windows.insert(session_key, expire_at);
            }
            PermissionMode::AskEveryTime => {
                // 单次批准：存储在临时缓存中，30秒内有效
                // 这给用户时间重新触发操作，但不会长期保留
                let expire_at = chrono::Utc::now() + chrono::Duration::seconds(30);
                let mut once = self.once_approvals.write().await;
                let session_key = format!("{}:{}", session_id, tool_key);
                once.insert(session_key, expire_at);
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
    pub async fn deny_request(&self, context: &PermissionContext, reason: &str) -> Result<(), String> {
        let tool_key = Self::approval_cache_key(&context.tool_name);

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

    /// 会话级别批准（按工具名，忽略参数）
    pub async fn approve_session(&self, session_id: String, tool_name: String, _arguments: &serde_json::Value) {
        let tool_key = Self::approval_cache_key(&tool_name);
        let mut approvals = self.session_approvals.write().await;
        approvals
            .entry(session_id)
            .or_default()
            .insert(tool_key);
    }

    /// 时间窗口批准（按工具名，绑定到 session）
    pub async fn approve_time_window(
        &self,
        session_id: String,
        tool_name: String,
        _arguments: &serde_json::Value,
        minutes: i64,
    ) {
        let tool_key = Self::approval_cache_key(&tool_name);
        let expire_at = chrono::Utc::now() + chrono::Duration::minutes(minutes);
        let mut windows = self.window_approvals.write().await;
        let session_key = format!("{}:{}", session_id, tool_key);
        windows.insert(session_key, expire_at);
    }

    /// 按 `tool_name` 记入本会话批准（与 `approve_session` 相同语义）。
    /// `_legacy_hash` 已废弃：旧调用方传入 SHA-256(tool+args)，现在只按工具名缓存，不再区分参数。
    pub async fn approve_with_hash(&self, session_id: String, tool_name: String, _legacy_hash: String) {
        let tool_key = Self::approval_cache_key(&tool_name);
        let mut approvals = self.session_approvals.write().await;
        approvals
            .entry(session_id)
            .or_default()
            .insert(tool_key);
    }

    /// 单次批准（仅这次允许，不持久化）
    pub async fn approve_once(&self, _session_id: String, _tool_name: String, _hash: String) {
        // 单次批准不需要持久化，直接通过 check 已经返回 Allow
        // 这里可以添加临时缓存如果需要
    }

    /// 拒绝工具（按工具名记入本会话拒绝列表，`hash` 参数已忽略）
    pub async fn deny_tool(&self, session_id: String, tool_name: String, _hash: String, reason: String) {
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
    pub async fn set_default_mode(&self, mode: crate::domain::permissions::types::PermissionModeInput) {
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
    pub async fn get_session_approvals(&self, session_id: &str) -> (std::collections::HashSet<String>, Option<chrono::DateTime<chrono::Utc>>) {
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

    /// 用户「记住」批准 / 本会话拒绝 使用的键：与 `tool_rules` 一致，Read/WebSearch 等与内置名合并
    fn approval_cache_key(tool_name: &str) -> String {
        let c = canonical_permission_tool_name(tool_name.trim());
        if c.starts_with("mcp__") {
            c
        } else {
            c.to_ascii_lowercase()
        }
    }

    /// 使用 SHA-256 计算工具+参数哈希（仅单元测试校验 canonical 行为；批准缓存按工具名）
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

    /// 本会话 / 时间窗口 / 单次「记住」是否已覆盖该工具（`tool_key` = `approval_cache_key`）
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

        let tool_key = Self::approval_cache_key(&context.tool_name);

        match rule.mode {
            PermissionMode::AskEveryTime => PermissionDecision::RequireApproval(PermissionRequest {
                request_id: uuid::Uuid::new_v4().to_string(),
                context: context.clone(),
                risk: risk.clone(),
                suggested_mode: PermissionMode::Session,
            }),
            PermissionMode::Session | PermissionMode::Plan => {
                if self
                    .is_remembered_tool_allowed(&context.session_id, &tool_key)
                    .await
                {
                    PermissionDecision::Allow
                } else {
                    PermissionDecision::RequireApproval(PermissionRequest {
                        request_id: uuid::Uuid::new_v4().to_string(),
                        context: context.clone(),
                        risk: risk.clone(),
                        suggested_mode: rule.mode,
                    })
                }
            }
            PermissionMode::TimeWindow { .. } => {
                if self
                    .is_remembered_tool_allowed(&context.session_id, &tool_key)
                    .await
                {
                    PermissionDecision::Allow
                } else {
                    PermissionDecision::RequireApproval(PermissionRequest {
                        request_id: uuid::Uuid::new_v4().to_string(),
                        context: context.clone(),
                        risk: risk.clone(),
                        suggested_mode: rule.mode,
                    })
                }
            }
            PermissionMode::Auto => {
                if risk.level >= RiskLevel::High {
                    PermissionDecision::RequireApproval(PermissionRequest {
                        request_id: uuid::Uuid::new_v4().to_string(),
                        context: context.clone(),
                        risk: risk.clone(),
                        suggested_mode: PermissionMode::AskEveryTime,
                    })
                } else {
                    PermissionDecision::Allow
                }
            }
            PermissionMode::Bypass => PermissionDecision::Allow,
        }
    }

    /// 与规则引擎 `PermissionMode::Auto` 一致：仅 High 及以上需确认（Critical 已在 `check_permission` 前段单独处理）。
    fn composer_auto_fallback(&self, context: &PermissionContext, risk: &RiskAssessment) -> PermissionDecision {
        if risk.level >= RiskLevel::High {
            PermissionDecision::RequireApproval(PermissionRequest {
                request_id: uuid::Uuid::new_v4().to_string(),
                context: context.clone(),
                risk: risk.clone(),
                suggested_mode: PermissionMode::AskEveryTime,
            })
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
            RiskLevel::Medium => PermissionDecision::RequireApproval(PermissionRequest {
                request_id: uuid::Uuid::new_v4().to_string(),
                context: context.clone(),
                risk: risk.clone(),
                suggested_mode: PermissionMode::Session,
            }),
            // High 和 Critical 风险（删除、系统命令）需要确认，建议使用更严格的模式
            RiskLevel::High | RiskLevel::Critical => PermissionDecision::RequireApproval(PermissionRequest {
                request_id: uuid::Uuid::new_v4().to_string(),
                context: context.clone(),
                risk: risk.clone(),
                suggested_mode: PermissionMode::AskEveryTime,
            }),
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
            else {
                if !matches!(path_matcher, PathMatcher::WithinProject) {
                    return false;
                }
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
            PathMatcher::Glob(pattern) => {
                globset::GlobBuilder::new(pattern)
                    .case_insensitive(false)
                    .build()
                    .and_then(|g| {
                        let mut builder = globset::GlobSetBuilder::new();
                        builder.add(g);
                        builder.build()
                    })
                    .map(|gs| gs.is_match(path))
                    .unwrap_or(false)
            }
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

    fn matches_condition(&self, condition: &ArgumentCondition, arguments: &serde_json::Value) -> bool {
        let arg_value = arguments.get(&condition.key);
        match condition.operator {
            ConditionOperator::Eq => arg_value == Some(&condition.value),
            ConditionOperator::Ne => arg_value != Some(&condition.value),
            ConditionOperator::Contains => {
                match (arg_value, condition.value.as_str()) {
                    (Some(serde_json::Value::String(s)), Some(pattern)) => s.contains(pattern),
                    (Some(serde_json::Value::Array(arr)), _) => arr.contains(&condition.value),
                    _ => false,
                }
            }
            ConditionOperator::StartsWith => {
                match (arg_value, condition.value.as_str()) {
                    (Some(serde_json::Value::String(s)), Some(prefix)) => s.starts_with(prefix),
                    _ => false,
                }
            }
            ConditionOperator::Matches => {
                match (arg_value, condition.value.as_str()) {
                    (Some(serde_json::Value::String(s)), Some(pattern)) => {
                        regex::RegexBuilder::new(pattern)
                            .size_limit(1_000_000)
                            .dfa_size_limit(1_000_000)
                            .build()
                            .map(|re| re.is_match(s))
                            .unwrap_or(false)
                    }
                    _ => false,
                }
            }
            ConditionOperator::In => {
                match (arg_value, condition.value.as_array()) {
                    (Some(val), Some(arr)) => arr.contains(val),
                    _ => false,
                }
            }
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
        if let Some(ref paths) = context.file_paths {
            for path in paths {
                let path_str = path.to_string_lossy();
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
                if path_str.contains(".env") || path_str.contains("secret") || path_str.contains("credential") {
                    detected_risks.push(DetectedRisk {
                        category: RiskCategory::Privacy,
                        severity: RiskLevel::Medium,
                        description: format!("可能涉及敏感文件: {}", path_str),
                        mitigation: Some("确认是否需要修改此文件".to_string()),
                    });
                    categories.push(RiskCategory::Privacy);
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
                level: RiskLevel::Medium,
                categories: vec![RiskCategory::System],
                description: "执行系统命令".to_string(),
                recommendations: vec![
                    "仔细检查命令内容".to_string(),
                    "避免使用 rm -rf 等危险命令".to_string(),
                ],
                detected_risks: vec![DetectedRisk {
                    category: RiskCategory::System,
                    severity: RiskLevel::Medium,
                    description: "允许执行任意系统命令".to_string(),
                    mitigation: Some("使用受限的 file_* 工具替代".to_string()),
                }],
            },
            "file_write" | "file_edit" | "skill_manage" | "skill_config" => RiskAssessment {
                level: RiskLevel::Low,
                categories: vec![RiskCategory::FileSystem, RiskCategory::DataLoss],
                description: "修改文件内容".to_string(),
                recommendations: vec![
                    "确认文件路径正确".to_string(),
                    "重要文件建议先备份".to_string(),
                ],
                detected_risks: vec![DetectedRisk {
                    category: RiskCategory::FileSystem,
                    severity: RiskLevel::Low,
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
            "web_fetch" | "web_search" => RiskAssessment {
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
            "list_skills"
            | "skills_list"
            | "skill_view"
            | "search"
            | "tool_search"
            | "get_current_time"
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

        if paths.is_empty() { None } else { Some(paths) }
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
        assert!(matches!(dec, PermissionDecision::Allow), "file_read should be Allow");
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

    // -------------------------------------------------------------------------
    // 会话拒绝测试
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn test_deny_blocks_subsequent_calls() {
        let mgr = PermissionManager::new();
        let ctx = PermissionContext {
            tool_name: "bash".to_string(),
            arguments: serde_json::json!({"command": "ls /tmp"}),
            session_id: "session_deny_test".to_string(),
            file_paths: None,
            timestamp: chrono::Utc::now(),
        };

        // 第一次拒绝
        mgr.deny_request(&ctx, "用户拒绝").await.unwrap();

        // 后续调用应直接返回 Deny（同工具不同参数也拒绝）
        let dec = mgr.check_permission(&ctx).await;
        assert!(
            matches!(dec, PermissionDecision::Deny(_)),
            "后续调用应返回 Deny，实际: {:?}", dec
        );
        let ctx_other_cmd = PermissionContext {
            arguments: serde_json::json!({"command": "echo other"}),
            ..ctx.clone()
        };
        let dec2 = mgr.check_permission(&ctx_other_cmd).await;
        assert!(
            matches!(dec2, PermissionDecision::Deny(_)),
            "同工具不同参数仍应 Deny，实际: {:?}", dec2
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
    async fn test_session_approve_allows_subsequent() {
        let mgr = PermissionManager::new();
        let ctx = PermissionContext {
            tool_name: "bash".to_string(),
            arguments: serde_json::json!({"command": "ls /tmp"}),
            session_id: "s_approve".to_string(),
            file_paths: None,
            timestamp: chrono::Utc::now(),
        };

        mgr.approve_request("s_approve", PermissionMode::Session, &ctx)
            .await
            .unwrap();

        let dec = mgr.check_permission(&ctx).await;
        assert!(
            matches!(dec, PermissionDecision::Allow),
            "批准后应 Allow，实际: {:?}", dec
        );
        let ctx_other = PermissionContext {
            arguments: serde_json::json!({"command": "cp a b"}),
            ..ctx.clone()
        };
        let dec2 = mgr.check_permission(&ctx_other).await;
        assert!(
            matches!(dec2, PermissionDecision::Allow),
            "同工具不同参数也应 Allow，实际: {:?}", dec2
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
            "file_write 不同路径应共享会话批准，实际: {:?}", dec
        );
    }

    /// Critical 风险（patterns 中如直接写磁盘设备）在未批准前走 Critical 分支；
    /// 本会话批准后应命中缓存而不再弹窗
    #[tokio::test]
    async fn test_session_approve_allows_critical_bash_same_args() {
        let mgr = PermissionManager::new();
        let ctx = PermissionContext {
            tool_name: "bash".to_string(),
            arguments: serde_json::json!({"command": "echo x > /dev/sda"}),
            session_id: "s_crit".to_string(),
            file_paths: None,
            timestamp: chrono::Utc::now(),
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
            "本会话记住后应 Allow，实际: {:?}", after
        );
        let ctx_other = PermissionContext {
            arguments: serde_json::json!({"command": "dd if=/dev/zero of=/dev/sda"}),
            ..ctx.clone()
        };
        let after_other = mgr.check_permission(&ctx_other).await;
        assert!(
            matches!(after_other, PermissionDecision::Allow),
            "Critical 工具本会话记住后，其它 bash 参数也应 Allow，实际: {:?}",
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
        };

        mgr.approve_request("s_tw", PermissionMode::TimeWindow { minutes: 60 }, &ctx)
            .await
            .unwrap();

        let dec = mgr.check_permission(&ctx).await;
        assert!(matches!(dec, PermissionDecision::Allow));
        let ctx2 = PermissionContext {
            arguments: serde_json::json!({"command": "different"}),
            ..ctx.clone()
        };
        let dec2 = mgr.check_permission(&ctx2).await;
        assert!(
            matches!(dec2, PermissionDecision::Allow),
            "时间窗口内同工具不同参数应 Allow，实际: {:?}",
            dec2
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
        };
        let result = mgr.approve_request("s", PermissionMode::Bypass, &ctx).await;
        assert!(result.is_err(), "Bypass 模式应被拒绝");
    }

    #[tokio::test]
    async fn test_time_window_overflow_rejected() {
        let mode = PermissionMode::TimeWindow { minutes: u32::MAX };
        assert!(mode.validate_user_mode().is_err(), "超大 TimeWindow 应被拒绝");
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
        };

        // 第一次：规则有效（use_count=0 < limit=1），Auto + Low risk → Allow
        let dec1 = mgr.check_permission(&ctx).await;
        assert!(matches!(dec1, PermissionDecision::Allow), "第一次应 Allow");

        // 第二次：规则已失效（use_count=1 >= limit=1），走 default_decision
        let dec2 = mgr.check_permission(&ctx).await;
        // file_write 在 assess_tool_risk 中为 Low，default_decision 对 Low 为 Allow
        assert!(
            matches!(dec2, PermissionDecision::Allow),
            "规则失效后应按默认策略：Low 风险 Allow，实际: {:?}",
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
            tool_matcher: ToolMatcher::Exact("bash".to_string()),
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

        let safe_cmd = serde_json::json!({"command": "ls"});

        // 规则创建者的会话：bash 是 Medium 风险，Auto 规则 → Allow（Low/Medium 允许）
        let ctx_owner = PermissionContext {
            tool_name: "bash".to_string(),
            arguments: safe_cmd.clone(),
            session_id: "session_owner".to_string(),
            file_paths: None,
            timestamp: chrono::Utc::now(),
        };
        let dec_owner = mgr.check_permission(&ctx_owner).await;
        assert!(matches!(dec_owner, PermissionDecision::Allow), "规则所有者 session 应 Allow");

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
        assert!(mgr.matches_tool_matcher(&ToolMatcher::Wildcard("file_*".to_string()), "file_write"));
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
        assert!(!mgr.matches_path_matcher(
            &PathMatcher::FileExtension(vec!["py".to_string()]),
            path
        ));
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
        assert_eq!(h1, h2, "Canonical JSON should produce same hash regardless of key order");
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
        };

        mgr.deny_request(&ctx, "用户明确拒绝").await.unwrap();

        let denials = mgr.get_recent_denials(10).await;
        assert_eq!(denials.len(), 1);
        assert_eq!(denials[0].tool_name, "bash");
        assert_eq!(denials[0].reason, "用户明确拒绝");
    }
}
