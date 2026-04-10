# Omiga 权限系统优化设计

> 生产级细粒度权限控制方案

---

## 1. 系统架构概览

```
┌─────────────────────────────────────────────────────────────────┐
│                     权限系统架构                                 │
├─────────────────────────────────────────────────────────────────┤
│  Frontend (React + MUI)                                         │
│  ├── PermissionDialog        - 权限确认对话框                   │
│  ├── PermissionRulesPanel    - 规则管理面板                     │
│  ├── RecentDenialsList       - 拒绝历史                         │
│  └── DangerousCommandAlert   - 危险命令警告                     │
├─────────────────────────────────────────────────────────────────┤
│  Tauri Bridge                                                   │
├─────────────────────────────────────────────────────────────────┤
│  Backend (Rust)                                                 │
│  ├── PermissionManager       - 权限管理器核心                   │
│  ├── RuleEngine              - 规则匹配引擎                     │
│  ├── RiskAssessor            - 风险评估器                       │
│  ├── PatternMatcher          - 模式匹配器                       │
│  ├── AuditLogger             - 审计日志                         │
│  └── PresetManager           - 预设配置管理                     │
└─────────────────────────────────────────────────────────────────┘
```

---

## 2. 核心类型设计

### 2.1 权限模式

```rust
// src-tauri/src/domain/permissions/types.rs

/// 权限模式 - 借鉴 Claude Code 设计并扩展
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum PermissionMode {
    /// 每次调用都询问（最安全）
    AskEveryTime,
    
    /// 同一会话内相同操作只问一次
    Session,
    
    /// 时间窗口内自动批准（如 1 小时）
    TimeWindow { minutes: u32 },
    
    /// Plan 模式 - 批量确认（类似 Claude Code Plan 模式）
    Plan,
    
    /// 自动批准安全操作
    Auto,
    
    /// 完全绕过（危险！仅用于测试）
    Bypass,
}

impl PermissionMode {
    /// 获取模式显示名称
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::AskEveryTime => "每次询问",
            Self::Session => "本次会话",
            Self::TimeWindow { minutes } => match minutes {
                60 => "1小时内",
                240 => "4小时内",
                1440 => "24小时内",
                _ => "自定义",
            },
            Self::Plan => "Plan模式",
            Self::Auto => "自动批准",
            Self::Bypass => "完全绕过",
        }
    }
    
    /// 是否需要持久化存储
    pub fn needs_persistence(&self) -> bool {
        matches!(self, Self::TimeWindow { .. } | Self::Plan | Self::Auto)
    }
}
```

### 2.2 权限规则

```rust
/// 权限规则 - 支持复杂匹配条件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRule {
    /// 规则唯一ID
    pub id: String,
    
    /// 规则名称（用户可识别）
    pub name: String,
    
    /// 规则描述
    pub description: Option<String>,
    
    /// 工具匹配模式
    pub tool_matcher: ToolMatcher,
    
    /// 路径匹配模式（可选）
    pub path_matcher: Option<PathMatcher>,
    
    /// 参数匹配条件（可选）
    pub argument_conditions: Vec<ArgumentCondition>,
    
    /// 权限模式
    pub mode: PermissionMode,
    
    /// 规则有效期
    pub validity: RuleValidity,
    
    /// 优先级（数字越小优先级越高）
    pub priority: i32,
    
    /// 创建时间
    pub created_at: DateTime<Utc>,
    
    /// 最后使用时间
    pub last_used_at: Option<DateTime<Utc>>,
    
    /// 使用次数统计
    pub use_count: u64,
}

/// 工具匹配器
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "pattern")]
pub enum ToolMatcher {
    /// 精确匹配
    Exact(String),
    /// 通配符匹配（如 "file_*"）
    Wildcard(String),
    /// 正则匹配
    Regex(String),
    /// 任意工具
    Any,
}

/// 路径匹配器
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "pattern")]
pub enum PathMatcher {
    /// 精确路径
    Exact(String),
    /// 前缀匹配（目录）
    Prefix(String),
    /// Glob 模式
    Glob(String),
    /// 正则匹配
    Regex(String),
    /// 项目内任意路径
    WithinProject,
    /// 特定文件类型
    FileExtension(Vec<String>),
}

/// 参数条件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArgumentCondition {
    /// 参数名
    pub key: String,
    /// 条件操作
    pub operator: ConditionOperator,
    /// 期望值
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op")]
pub enum ConditionOperator {
    Eq,      // 等于
    Ne,      // 不等于
    Contains,// 包含
    StartsWith, // 以...开头
    Matches, // 正则匹配
    In,      // 在列表中
}

/// 规则有效期
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RuleValidity {
    /// 永久有效
    Permanent,
    /// 指定过期时间
    Until(DateTime<Utc>),
    /// 使用次数限制
    UseLimit(u64),
    /// 会话期间有效
    CurrentSession,
}
```

### 2.3 风险评估

```rust
/// 风险等级
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RiskLevel {
    /// 安全 - 纯读取操作
    Safe = 0,
    /// 低风险 - 项目内修改
    Low = 1,
    /// 中等风险 - 文件系统修改
    Medium = 2,
    /// 高风险 - 删除/系统操作
    High = 3,
    /// 严重风险 - 可能导致系统损坏
    Critical = 4,
}

impl RiskLevel {
    /// 获取颜色（用于 UI）
    pub fn color(&self) -> &'static str {
        match self {
            Self::Safe => "#4caf50",      // 绿色
            Self::Low => "#8bc34a",       // 浅绿
            Self::Medium => "#ff9800",    // 橙色
            Self::High => "#f44336",      // 红色
            Self::Critical => "#b71c1c",  // 深红
        }
    }
    
    /// 获取图标
    pub fn icon(&self) -> &'static str {
        match self {
            Self::Safe => "✓",
            Self::Low => "ℹ",
            Self::Medium => "⚠",
            Self::High => "⚠",
            Self::Critical => "☠",
        }
    }
    
    /// 是否需要显式确认
    pub fn needs_confirmation(&self) -> bool {
        *self >= Self::Medium
    }
}

/// 风险详情
#[derive(Debug, Clone)]
pub struct RiskAssessment {
    /// 风险等级
    pub level: RiskLevel,
    /// 风险类别
    pub categories: Vec<RiskCategory>,
    /// 风险描述
    pub description: String,
    /// 建议操作
    pub recommendations: Vec<String>,
    /// 检测到的具体风险点
    pub detected_risks: Vec<DetectedRisk>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RiskCategory {
    FileSystem,      // 文件系统操作
    System,          // 系统命令
    Network,         // 网络操作
    DataLoss,        // 数据丢失风险
    Security,        // 安全风险
    Privacy,         // 隐私风险
}

#[derive(Debug, Clone)]
pub struct DetectedRisk {
    pub category: RiskCategory,
    pub severity: RiskLevel,
    pub description: String,
    pub mitigation: Option<String>,
}
```

---

## 3. 权限管理器实现

```rust
// src-tauri/src/domain/permissions/manager.rs

pub struct PermissionManager {
    /// 权限规则存储
    rules: Arc<RwLock<Vec<PermissionRule>>>,
    
    /// 会话级批准缓存（session_id -> tool_hash）
    session_approvals: Arc<RwLock<HashMap<String, HashSet<String>>>>,
    
    /// 时间窗口批准缓存（tool_hash -> expire_time）
    window_approvals: Arc<RwLock<HashMap<String, DateTime<Utc>>>>,
    
    /// 拒绝历史记录
    recent_denials: Arc<RwLock<VecDeque<DenialRecord>>>,
    
    /// 审计日志
    audit_log: Arc<AuditLogger>,
    
    /// 预设配置
    presets: Arc<PresetManager>,
    
    /// 危险模式数据库
    dangerous_patterns: Arc<DangerousPatternDB>,
}

impl PermissionManager {
    /// 检查权限 - 核心方法
    pub async fn check_permission(
        &self,
        context: &PermissionContext,
    ) -> PermissionDecision {
        let start_time = Instant::now();
        
        // 1. 执行风险评估
        let risk = self.assess_risk(context).await;
        
        // 2. 严重风险立即要求确认
        if risk.level == RiskLevel::Critical {
            self.log_audit(AuditEvent::CriticalRiskDetected {
                context: context.clone(),
                risk: risk.clone(),
            }).await;
            
            return PermissionDecision::RequireApproval(PermissionRequest {
                context: context.clone(),
                risk,
                suggested_mode: PermissionMode::AskEveryTime,
            });
        }
        
        // 3. 规则匹配（按优先级排序）
        let rules = self.rules.read().await;
        let mut matching_rules: Vec<_> = rules
            .iter()
            .filter(|r| self.rule_matches(r, context))
            .collect();
        
        matching_rules.sort_by_key(|r| r.priority);
        
        // 4. 应用最高优先级的规则
        if let Some(rule) = matching_rules.first() {
            let decision = self.apply_rule(rule, context, &risk).await;
            
            // 更新规则统计
            self.update_rule_usage(rule.id.clone()).await;
            
            self.log_audit(AuditEvent::RuleApplied {
                rule_id: rule.id.clone(),
                context: context.clone(),
                decision: decision.clone(),
                duration_ms: start_time.elapsed().as_millis() as u64,
            }).await;
            
            return decision;
        }
        
        // 5. 检查会话级批准
        if self.is_session_approved(context).await {
            return PermissionDecision::Allow;
        }
        
        // 6. 检查时间窗口批准
        if self.is_window_approved(context).await {
            return PermissionDecision::Allow;
        }
        
        // 7. 根据风险等级默认行为
        let decision = self.default_decision(context, &risk).await;
        
        self.log_audit(AuditEvent::DefaultDecision {
            context: context.clone(),
            risk,
            decision: decision.clone(),
            duration_ms: start_time.elapsed().as_millis() as u64,
        }).await;
        
        decision
    }
    
    /// 风险评估
    async fn assess_risk(&self, context: &PermissionContext) -> RiskAssessment {
        let mut categories = Vec::new();
        let mut detected_risks = Vec::new();
        
        // 1. 工具级别风险评估
        let tool_risk = self.assess_tool_risk(context.tool_name);
        categories.extend(tool_risk.categories.clone());
        detected_risks.extend(tool_risk.detected_risks);
        
        // 2. 参数级别风险评估
        let arg_risk = self.assess_argument_risk(context);
        categories.extend(arg_risk.categories);
        detected_risks.extend(arg_risk.detected_risks);
        
        // 3. 路径级别风险评估（针对文件操作）
        if let Some(paths) = &context.file_paths {
            let path_risk = self.assess_path_risk(paths).await;
            categories.extend(path_risk.categories);
            detected_risks.extend(path_risk.detected_risks);
        }
        
        // 4. 计算总体风险等级
        let max_risk = detected_risks
            .iter()
            .map(|r| r.severity)
            .max()
            .unwrap_or(RiskLevel::Safe);
        
        // 5. 生成描述和建议
        let (description, recommendations) = self.generate_risk_info(&detected_risks);
        
        // 去重类别
        categories.sort();
        categories.dedup();
        
        RiskAssessment {
            level: max_risk,
            categories,
            description,
            recommendations,
            detected_risks,
        }
    }
    
    /// 评估工具风险
    fn assess_tool_risk(&self, tool_name: &str) -> RiskAssessment {
        match tool_name {
            "bash" | "shell" | "powershell" => RiskAssessment {
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
            "file_write" | "file_edit" | "sed_edit" => RiskAssessment {
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
            "file_read" | "glob" | "grep" => RiskAssessment {
                level: RiskLevel::Safe,
                categories: vec![],
                description: "读取文件（安全操作）".to_string(),
                recommendations: vec![],
                detected_risks: vec![],
            },
            "web_fetch" | "web_search" => RiskAssessment {
                level: RiskLevel::Low,
                categories: vec![RiskCategory::Network],
                description: "网络请求".to_string(),
                recommendations: vec![
                    "确认 URL 可信".to_string(),
                ],
                detected_risks: vec![DetectedRisk {
                    category: RiskCategory::Network,
                    severity: RiskLevel::Low,
                    description: "将访问外部网络".to_string(),
                    mitigation: None,
                }],
            },
            _ => RiskAssessment {
                level: RiskLevel::Medium,
                categories: vec![],
                description: format!("使用工具: {}", tool_name),
                recommendations: vec![],
                detected_risks: vec![],
            },
        }
    }
    
    /// 评估参数风险（危险命令检测）
    fn assess_argument_risk(&self, context: &PermissionContext) -> RiskAssessment {
        let mut detected_risks = Vec::new();
        
        if context.tool_name == "bash" || context.tool_name == "shell" {
            if let Some(cmd) = context.arguments.get("command").and_then(|v| v.as_str()) {
                // 使用 DangerousPatternDB 检测
                let patterns = self.dangerous_patterns.get_patterns();
                for pattern in patterns {
                    if pattern.matches(cmd) {
                        detected_risks.push(DetectedRisk {
                            category: pattern.category,
                            severity: pattern.severity,
                            description: pattern.description.clone(),
                            mitigation: pattern.mitigation.clone(),
                        });
                    }
                }
            }
        }
        
        RiskAssessment {
            level: detected_risks.iter().map(|r| r.severity).max().unwrap_or(RiskLevel::Safe),
            categories: detected_risks.iter().map(|r| r.category).collect(),
            description: "参数风险分析".to_string(),
            recommendations: detected_risks.iter()
                .filter_map(|r| r.mitigation.clone())
                .collect(),
            detected_risks,
        }
    }
}
```

---

## 4. 危险命令检测数据库

```rust
// src-tauri/src/domain/permissions/dangerous_patterns.rs

pub struct DangerousPatternDB {
    patterns: Vec<DangerousPattern>,
}

pub struct DangerousPattern {
    /// 匹配正则
    pub regex: Regex,
    /// 风险等级
    pub severity: RiskLevel,
    /// 风险类别
    pub category: RiskCategory,
    /// 描述
    pub description: String,
    /// 缓解建议
    pub mitigation: Option<String>,
    /// 是否区分大小写
    pub case_sensitive: bool,
}

impl DangerousPatternDB {
    pub fn new() -> Self {
        let mut db = Self { patterns: Vec::new() };
        db.load_default_patterns();
        db
    }
    
    fn load_default_patterns(&mut self) {
        // ==================== CRITICAL 级别 ====================
        self.add(PatternBuilder::new(r"rm\s+-rf\s+(/|~|\.\./|/\s*)")
            .critical()
            .category(RiskCategory::DataLoss)
            .description("递归强制删除根目录或家目录")
            .mitigation("确认目标路径是否正确，避免使用 -rf 参数")
            .build());
        
        self.add(PatternBuilder::new(r":\(\)\s*\{\s*:\s*\|\s*:\s*&\s*\}\s*;\s*:")
            .critical()
            .category(RiskCategory::System)
            .description("Fork bomb - 会导致系统资源耗尽")
            .mitigation("这是一个恶意命令，永远不要执行")
            .build());
        
        self.add(PatternBuilder::new(r">\s*/dev/sd[a-z]")
            .critical()
            .category(RiskCategory::DataLoss)
            .description("直接写入磁盘设备（会覆盖分区表）")
            .mitigation("使用 dd 时注意 of= 参数")
            .build());
        
        self.add(PatternBuilder::new(r"mkfs\.\w+\s+/dev/")
            .critical()
            .category(RiskCategory::DataLoss)
            .description("格式化磁盘分区")
            .mitigation("确认目标设备是否正确")
            .build());
        
        // ==================== HIGH 级别 ====================
        self.add(PatternBuilder::new(r"chmod\s+-R\s+777\s+/")
            .high()
            .category(RiskCategory::Security)
            .description("递归修改根目录权限为 777")
            .mitigation("使用更精确的权限设置")
            .build());
        
        self.add(PatternBuilder::new(r"chown\s+-R\s+\w+:\w+\s+/")
            .high()
            .category(RiskCategory::System)
            .description("递归修改根目录所有者")
            .mitigation("确认目标路径")
            .build());
        
        self.add(PatternBuilder::new(r"dd\s+if=.+of=/dev/")
            .high()
            .category(RiskCategory::DataLoss)
            .description("dd 命令写入设备")
            .mitigation("仔细检查 of= 参数")
            .build());
        
        self.add(PatternBuilder::new(r"shutdown|reboot|halt|poweroff")
            .high()
            .category(RiskCategory::System)
            .description("系统关机/重启命令")
            .mitigation("确认是否真的需要重启")
            .build());
        
        // ==================== MEDIUM 级别 ====================
        self.add(PatternBuilder::new(r"rm\s+-rf")
            .medium()
            .category(RiskCategory::DataLoss)
            .description("递归强制删除")
            .mitigation("确认目标路径，考虑使用 -i 交互模式")
            .build());
        
        self.add(PatternBuilder::new(r"curl.+\|\s*sh|wget.+\|\s*sh")
            .medium()
            .category(RiskCategory::Security)
            .description("管道执行远程脚本（可能有恶意代码）")
            .mitigation("先下载查看脚本内容，确认安全后再执行")
            .build());
        
        self.add(PatternBuilder::new(r"sudo|su\s+-")
            .medium()
            .category(RiskCategory::Security)
            .description("提权操作")
            .mitigation("确认命令来源可信")
            .build());
        
        // 文件操作风险
        self.add(PatternBuilder::new(r"/etc/(passwd|shadow|sudoers)")
            .high()
            .category(RiskCategory::System)
            .description("修改系统关键文件")
            .mitigation("这些文件修改可能导致系统无法登录")
            .build());
        
        self.add(PatternBuilder::new(r"\.(env|secret|key|pem|p12)$")
            .medium()
            .category(RiskCategory::Privacy)
            .description("可能涉及敏感凭证文件")
            .mitigation("确认是否需要修改这些文件")
            .file_only()
            .build());
    }
    
    pub fn get_patterns(&self) -> &[DangerousPattern] {
        &self.patterns
    }
    
    pub fn add(&mut self, pattern: DangerousPattern) {
        self.patterns.push(pattern);
    }
}
```

---

## 5. 预设配置管理

```rust
// src-tauri/src/domain/permissions/presets.rs

pub struct PresetManager;

impl PresetManager {
    /// 获取开发模式预设
    pub fn development_preset() -> Vec<PermissionRule> {
        vec![
            // 项目内文件操作自动批准
            PermissionRule {
                id: uuid::Uuid::new_v4().to_string(),
                name: "项目文件自动批准".to_string(),
                description: Some("项目目录内的文件读写操作".to_string()),
                tool_matcher: ToolMatcher::Wildcard("file_*".to_string()),
                path_matcher: Some(PathMatcher::WithinProject),
                argument_conditions: vec![],
                mode: PermissionMode::Auto,
                validity: RuleValidity::Permanent,
                priority: 100,
                created_at: Utc::now(),
                last_used_at: None,
                use_count: 0,
            },
            // 安全的读取操作
            PermissionRule {
                id: uuid::Uuid::new_v4().to_string(),
                name: "读取操作自动批准".to_string(),
                tool_matcher: ToolMatcher::Exact("file_read".to_string()),
                path_matcher: None,
                argument_conditions: vec![],
                mode: PermissionMode::Auto,
                validity: RuleValidity::Permanent,
                priority: 90,
                created_at: Utc::now(),
                last_used_at: None,
                use_count: 0,
            },
        ]
    }
    
    /// 获取安全模式预设
    pub fn secure_preset() -> Vec<PermissionRule> {
        vec![
            PermissionRule {
                id: uuid::Uuid::new_v4().to_string(),
                name: "所有操作询问".to_string(),
                tool_matcher: ToolMatcher::Any,
                path_matcher: None,
                argument_conditions: vec![],
                mode: PermissionMode::AskEveryTime,
                validity: RuleValidity::Permanent,
                priority: 0, // 最低优先级，作为默认规则
                created_at: Utc::now(),
                last_used_at: None,
                use_count: 0,
            },
        ]
    }
    
    /// 获取 CI/CD 模式预设
    pub fn cicd_preset() -> Vec<PermissionRule> {
        vec![
            PermissionRule {
                id: uuid::Uuid::new_v4().to_string(),
                name: "CI 环境批量批准".to_string(),
                tool_matcher: ToolMatcher::Any,
                path_matcher: None,
                argument_conditions: vec![],
                mode: PermissionMode::Plan,
                validity: RuleValidity::CurrentSession,
                priority: 0,
                created_at: Utc::now(),
                last_used_at: None,
                use_count: 0,
            },
        ]
    }
}
```

---

## 6. 前端 UI 设计

### 6.1 权限确认对话框

```tsx
// src/components/permissions/PermissionDialog.tsx

import React, { useState } from 'react';
import {
  Dialog,
  DialogTitle,
  DialogContent,
  DialogActions,
  Button,
  Typography,
  Alert,
  AlertTitle,
  Box,
  Chip,
  Divider,
  FormControl,
  InputLabel,
  Select,
  MenuItem,
  Stack,
  Accordion,
  AccordionSummary,
  AccordionDetails,
} from '@mui/material';
import {
  Warning as WarningIcon,
  Error as ErrorIcon,
  CheckCircle as CheckIcon,
  ExpandMore as ExpandMoreIcon,
} from '@mui/icons-material';

interface PermissionDialogProps {
  open: boolean;
  request: PermissionRequest;
  onApprove: (mode: PermissionMode) => void;
  onDeny: () => void;
}

export const PermissionDialog: React.FC<PermissionDialogProps> = ({
  open,
  request,
  onApprove,
  onDeny,
}) => {
  const [mode, setMode] = useState<PermissionMode>(PermissionMode.Session);
  const [showDetails, setShowDetails] = useState(false);
  
  const { risk, context } = request;
  const isDangerous = risk.level >= RiskLevel.High;
  const isCritical = risk.level === RiskLevel.Critical;
  
  const getRiskColor = (level: RiskLevel) => {
    switch (level) {
      case RiskLevel.Safe: return 'success';
      case RiskLevel.Low: return 'info';
      case RiskLevel.Medium: return 'warning';
      case RiskLevel.High: return 'error';
      case RiskLevel.Critical: return 'error';
    }
  };
  
  return (
    <Dialog 
      open={open} 
      maxWidth="md" 
      fullWidth
      disableEscapeKeyDown={isCritical}
    >
      <DialogTitle sx={{ display: 'flex', alignItems: 'center', gap: 1 }}>
        {isCritical ? (
          <ErrorIcon color="error" fontSize="large" />
        ) : isDangerous ? (
          <WarningIcon color="error" />
        ) : (
          <CheckIcon color="primary" />
        )}
        权限确认
        <Chip 
          label={risk.level} 
          color={getRiskColor(risk.level) as any}
          size="small"
          sx={{ ml: 'auto' }}
        />
      </DialogTitle>
      
      <DialogContent>
        {/* 风险警告 */}
        {isCritical && (
          <Alert severity="error" sx={{ mb: 2 }}>
            <AlertTitle>严重风险操作</AlertTitle>
            此操作可能导致系统损坏或数据丢失，请格外谨慎！
          </Alert>
        )}
        
        {isDangerous && !isCritical && (
          <Alert severity="warning" sx={{ mb: 2 }}>
            <AlertTitle>高风险操作</AlertTitle>
            此操作可能影响系统稳定性，请确认您了解其后果。
          </Alert>
        )}
        
        {/* 操作信息 */}
        <Box sx={{ mb: 2 }}>
          <Typography variant="subtitle2" color="text.secondary">
            工具
          </Typography>
          <Typography variant="h6" component="code">
            {context.toolName}
          </Typography>
        </Box>
        
        <Box sx={{ mb: 2 }}>
          <Typography variant="subtitle2" color="text.secondary">
            参数
          </Typography>
          <Box 
            component="pre" 
            sx={{ 
              bgcolor: 'grey.100', 
              p: 1, 
              borderRadius: 1,
              overflow: 'auto',
              maxHeight: 200,
            }}
          >
            {JSON.stringify(context.arguments, null, 2)}
          </Box>
        </Box>
        
        {/* 风险详情 */}
        <Accordion 
          expanded={showDetails} 
          onChange={() => setShowDetails(!showDetails)}
        >
          <AccordionSummary expandIcon={<ExpandMoreIcon />}>
            <Typography>风险详情</Typography>
          </AccordionSummary>
          <AccordionDetails>
            <Stack spacing={1}>
              {risk.detectedRisks.map((risk, idx) => (
                <Alert 
                  key={idx} 
                  severity={getRiskColor(risk.severity) as any}
                  variant="outlined"
                >
                  <Typography variant="body2">
                    {risk.description}
                  </Typography>
                  {risk.mitigation && (
                    <Typography variant="caption" color="text.secondary">
                      建议: {risk.mitigation}
                    </Typography>
                  )}
                </Alert>
              ))}
            </Stack>
          </AccordionDetails>
        </Accordion>
        
        <Divider sx={{ my: 2 }} />
        
        {/* 记住选择 */}
        <FormControl fullWidth>
          <InputLabel>记住我的选择</InputLabel>
          <Select
            value={mode}
            onChange={(e) => setMode(e.target.value as PermissionMode)}
          >
            <MenuItem value={PermissionMode.AskEveryTime}>
              仅这次允许
            </MenuItem>
            <MenuItem value={PermissionMode.Session}>
              本次会话内允许
            </MenuItem>
            <MenuItem value={PermissionMode.TimeWindow60}>
              1小时内允许
            </MenuItem>
            <MenuItem value={PermissionMode.Plan}>
              Plan 模式（批量确认）
            </MenuItem>
          </Select>
        </FormControl>
      </DialogContent>
      
      <DialogActions sx={{ px: 3, pb: 2 }}>
        <Button 
          onClick={onDeny} 
          color="inherit"
          variant="outlined"
        >
          拒绝
        </Button>
        <Button 
          onClick={() => onApprove(mode)} 
          color={isDangerous ? 'error' : 'primary'}
          variant="contained"
          autoFocus
        >
          {isCritical ? '我已了解风险，确认允许' : '允许'}
        </Button>
      </DialogActions>
    </Dialog>
  );
};
```

### 6.2 规则管理面板

```tsx
// src/components/permissions/PermissionRulesPanel.tsx

import React from 'react';
import {
  Box,
  List,
  ListItem,
  ListItemText,
  ListItemSecondaryAction,
  IconButton,
  Chip,
  Typography,
  Button,
  Dialog,
} from '@mui/material';
import {
  Delete as DeleteIcon,
  Edit as EditIcon,
  Add as AddIcon,
} from '@mui/icons-material';

export const PermissionRulesPanel: React.FC = () => {
  const [rules, setRules] = useState<PermissionRule[]>([]);
  const [editingRule, setEditingRule] = useState<PermissionRule | null>(null);
  
  const loadRules = async () => {
    const loaded = await invoke<PermissionRule[]>('permission_list_rules');
    setRules(loaded);
  };
  
  useEffect(() => {
    loadRules();
  }, []);
  
  const handleDelete = async (id: string) => {
    await invoke('permission_delete_rule', { id });
    loadRules();
  };
  
  return (
    <Box>
      <Box sx={{ display: 'flex', justifyContent: 'space-between', mb: 2 }}>
        <Typography variant="h6">权限规则</Typography>
        <Button
          variant="contained"
          startIcon={<AddIcon />}
          onClick={() => setEditingRule({} as PermissionRule)}
        >
          添加规则
        </Button>
      </Box>
      
      <List>
        {rules.map((rule) => (
          <ListItem key={rule.id} divider>
            <ListItemText
              primary={
                <Box sx={{ display: 'flex', alignItems: 'center', gap: 1 }}>
                  {rule.name}
                  <Chip 
                    label={rule.mode.displayName} 
                    size="small" 
                    color="primary"
                  />
                </Box>
              }
              secondary={rule.description}
            />
            <ListItemSecondaryAction>
              <IconButton onClick={() => setEditingRule(rule)}>
                <EditIcon />
              </IconButton>
              <IconButton onClick={() => handleDelete(rule.id)}>
                <DeleteIcon />
              </IconButton>
            </ListItemSecondaryAction>
          </ListItem>
        ))}
      </List>
      
      {/* 规则编辑对话框 */}
      <RuleEditDialog
        open={!!editingRule}
        rule={editingRule}
        onClose={() => setEditingRule(null)}
        onSave={loadRules}
      />
    </Box>
  );
};
```

---

## 7. Tauri 命令接口

```rust
// src-tauri/src/commands/permissions.rs

#[tauri::command]
pub async fn permission_check(
    tool_name: String,
    arguments: serde_json::Value,
    session_id: String,
    state: tauri::State<'_, OmigaAppState>,
) -> Result<PermissionCheckResult, String> {
    let context = PermissionContext {
        tool_name,
        arguments,
        session_id,
        file_paths: None,
        timestamp: Utc::now(),
    };
    
    let manager = &state.permission_manager;
    let decision = manager.check_permission(&context).await;
    
    Ok(PermissionCheckResult::from(decision))
}

#[tauri::command]
pub async fn permission_approve(
    request_id: String,
    mode: PermissionMode,
    session_id: String,
    state: tauri::State<'_, OmigaAppState>,
) -> Result<(), String> {
    let manager = &state.permission_manager;
    manager.approve_request(&request_id, &session_id, mode).await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn permission_list_rules(
    state: tauri::State<'_, OmigaAppState>,
) -> Result<Vec<PermissionRule>, String> {
    let manager = &state.permission_manager;
    manager.list_rules().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn permission_add_rule(
    rule: PermissionRule,
    state: tauri::State<'_, OmigaAppState>,
) -> Result<(), String> {
    let manager = &state.permission_manager;
    manager.add_rule(rule).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn permission_delete_rule(
    id: String,
    state: tauri::State<'_, OmigaAppState>,
) -> Result<(), String> {
    let manager = &state.permission_manager;
    manager.delete_rule(&id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn permission_apply_preset(
    preset: String,
    state: tauri::State<'_, OmigaAppState>,
) -> Result<(), String> {
    let manager = &state.permission_manager;
    
    let rules = match preset.as_str() {
        "development" => PresetManager::development_preset(),
        "secure" => PresetManager::secure_preset(),
        "cicd" => PresetManager::cicd_preset(),
        _ => return Err("Unknown preset".to_string()),
    };
    
    for rule in rules {
        manager.add_rule(rule).await.map_err(|e| e.to_string())?;
    }
    
    Ok(())
}

#[tauri::command]
pub async fn permission_get_recent_denials(
    limit: usize,
    state: tauri::State<'_, OmigaAppState>,
) -> Result<Vec<DenialRecord>, String> {
    let manager = &state.permission_manager;
    manager.get_recent_denials(limit).await.map_err(|e| e.to_string())
}
```

---

## 8. 性能优化

### 8.1 规则缓存

```rust
pub struct RuleCache {
    /// 工具名 -> 匹配规则缓存
    tool_cache: RwLock<HashMap<String, Vec<PermissionRule>>>,
    /// 缓存过期时间
    expires_at: RwLock<DateTime<Utc>>,
}

impl RuleCache {
    pub async fn get_rules_for_tool(&self, tool: &str) -> Vec<PermissionRule> {
        // 检查缓存是否过期
        if self.is_expired().await {
            self.invalidate().await;
        }
        
        // 返回缓存或重新计算
        let cache = self.tool_cache.read().await;
        cache.get(tool).cloned().unwrap_or_default()
    }
}
```

### 8.2 异步风险评估

```rust
/// 并行风险评估
pub async fn assess_risk_parallel(&self, context: &PermissionContext) -> RiskAssessment {
    let futures = vec![
        self.assess_tool_risk(context.tool_name),
        self.assess_argument_risk(context),
        self.assess_path_risk(context),
    ];
    
    let results = futures::future::join_all(futures).await;
    
    // 合并结果
    self.merge_risk_assessments(results)
}
```

---

*文档版本: 1.0*  
*最后更新: 2026-04-07*
