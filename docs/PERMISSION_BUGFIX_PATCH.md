# 权限系统 Bug 修复补丁

## 必须修复的 Bug

### 1. 修复前端参数传递 (critical)

**文件**: `src/state/permissionStore.ts`

```typescript
// 修改 PermissionCheckResult 接口，添加 arguments
export interface PermissionCheckResult {
  allowed: boolean;
  requires_approval: boolean;
  request_id?: string;
  tool_name: string;
  risk_level: RiskLevel;
  risk_description: string;
  detected_risks: RiskInfo[];
  recommendations: string[];
  arguments?: Record<string, unknown>; // 添加此行
}

// 修改 approveRequest
approveRequest: async (mode) => {
  const { pendingRequest } = get();
  if (!pendingRequest) return;

  await invoke("permission_approve", {
    sessionId: "current",
    mode,
    toolName: pendingRequest.tool_name,
    arguments: pendingRequest.arguments || {}, // 使用原始参数
  });

  set({ pendingRequest: null });
},

// 修改 denyRequest
denyRequest: async (reason = "用户拒绝") => {
  const { pendingRequest } = get();
  if (!pendingRequest) return;

  await invoke("permission_deny", {
    sessionId: "current",
    toolName: pendingRequest.tool_name,
    arguments: pendingRequest.arguments || {}, // 使用原始参数
    reason,
  });

  set({ pendingRequest: null });
},
```

**文件**: `src-tauri/src/commands/permissions.rs`

```rust
// 修改 PermissionCheckResult 结构体
#[derive(Debug, Clone, serde::Serialize)]
pub struct PermissionCheckResult {
    pub allowed: bool,
    pub requires_approval: bool,
    pub request_id: Option<String>,
    pub tool_name: String,
    pub risk_level: String,
    pub risk_description: String,
    pub detected_risks: Vec<RiskInfo>,
    pub recommendations: Vec<String>,
    pub arguments: Option<serde_json::Value>, // 添加此行
}

// 修改 From 实现
impl From<PermissionDecision> for PermissionCheckResult {
    fn from(decision: PermissionDecision) -> Self {
        match decision {
            PermissionDecision::Allow => Self { ... },
            PermissionDecision::Deny(reason) => Self { ... },
            PermissionDecision::RequireApproval(request) => Self {
                ...
                arguments: Some(request.context.arguments), // 添加此行
            },
        }
    }
}
```

### 2. 修复 ConditionOperator::In

**文件**: `src/domain/permissions/manager.rs` (第496-502行)

```rust
ConditionOperator::In => {
    match (arg_value, condition.value.as_array()) {
        (Some(val), Some(arr)) => arr.contains(val),
        _ => false,
    }
}
```

### 3. 修复时间窗口序列化

**文件**: `src-tauri/src/commands/permissions.rs`

添加一个前端友好的 PermissionMode 类型：

```rust
#[derive(Debug, Clone, serde::Deserialize)]
pub enum PermissionModeInput {
    AskEveryTime,
    Session,
    TimeWindow { minutes: u32 },
    Plan,
    Auto,
}

impl From<PermissionModeInput> for PermissionMode {
    fn from(input: PermissionModeInput) -> Self {
        match input {
            PermissionModeInput::AskEveryTime => PermissionMode::AskEveryTime,
            PermissionModeInput::Session => PermissionMode::Session,
            PermissionModeInput::TimeWindow { minutes } => PermissionMode::TimeWindow { minutes },
            PermissionModeInput::Plan => PermissionMode::Plan,
            PermissionModeInput::Auto => PermissionMode::Auto,
        }
    }
}
```

### 4. 添加时间窗口清理

**文件**: `src/domain/permissions/manager.rs`

在 `is_approved_by_hash` 方法中添加清理：

```rust
async fn is_approved_by_hash(&self, session_id: &str, tool_hash: &str) -> bool {
    // 检查会话批准
    {
        let approvals = self.session_approvals.read().await;
        if let Some(session_approved) = approvals.get(session_id) {
            if session_approved.contains(tool_hash) {
                return true;
            }
        }
    }

    // 检查时间窗口批准（同时清理过期条目）
    {
        let mut windows = self.window_approvals.write().await;
        let now = chrono::Utc::now();
        // 惰性清理过期条目
        windows.retain(|_, expire_at| now < *expire_at);
        
        if let Some(expire_at) = windows.get(tool_hash) {
            if now < *expire_at {
                return true;
            }
        }
    }

    false
}
```

### 5. 修复哈希计算稳定性

**文件**: `src/domain/permissions/manager.rs`

使用规范化的 JSON 表示：

```rust
fn compute_tool_hash(tool_name: &str, arguments: &serde_json::Value) -> String {
    use std::collections::BTreeMap;
    
    // 将 JSON 转换为规范形式（排序键）
    let canonical = match arguments {
        serde_json::Value::Object(map) => {
            let sorted: BTreeMap<_, _> = map.iter().collect();
            serde_json::to_string(&sorted).unwrap_or_default()
        }
        _ => arguments.to_string(),
    };
    
    let mut hasher = Sha256::new();
    hasher.update(tool_name.as_bytes());
    hasher.update(b"\x00");
    hasher.update(canonical.as_bytes());
    format!("{:x}", hasher.finalize())
}
```

## 测试建议

添加以下测试用例：

```rust
#[tokio::test]
async fn test_arguments_hash_consistency() {
    let mgr = PermissionManager::new();
    
    // 相同参数不同顺序应产生相同哈希
    let args1 = serde_json::json!({"a": 1, "b": 2});
    let args2 = serde_json::json!({"b": 2, "a": 1});
    
    let h1 = PermissionManager::compute_tool_hash("test", &args1);
    let h2 = PermissionManager::compute_tool_hash("test", &args2);
    
    assert_eq!(h1, h2);
}

#[tokio::test]
async fn test_time_window_expiration_cleanup() {
    let mgr = PermissionManager::new();
    let ctx = create_test_context();
    
    // 批准一个极短的时间窗口
    mgr.approve_request("s", PermissionMode::TimeWindow { minutes: 0 }, &ctx)
        .await
        .unwrap();
    
    // 立即检查应该过期
    let dec = mgr.check_permission(&ctx).await;
    assert!(matches!(dec, PermissionDecision::RequireApproval(_)));
    
    // 验证过期条目已被清理
    let windows = mgr.window_approvals.read().await;
    assert!(windows.is_empty());
}
```

## 验证步骤

1. 运行后端测试：`cargo test permission_manager::tests`
2. 运行前端测试：`bun test`（如果有）
3. 手动测试：
   - 执行危险命令（如 `rm -rf /`）
   - 点击"允许本次会话"
   - 再次执行相同命令，应该直接通过
