# Omiga 优化优先级调整

> 针对 Omiga 桌面 IDE 特性的重新规划

---

## 🎯 关键认知

### Omiga 的架构特点

```
┌─────────────────────────────────────────────────────────────┐
│                     Omiga (桌面 IDE)                         │
├─────────────────────────────────────────────────────────────┤
│  Frontend (React + MUI + Monaco)                            │
│  ├── 代码编辑器 (Monaco) - 无需外部 IDE                      │
│  ├── 文件浏览器 (自带)                                      │
│  ├── 终端模拟器 (xterm.js)                                  │
│  └── 对话界面 (Chat UI)                                     │
├─────────────────────────────────────────────────────────────┤
│  Tauri (Rust 后端)                                          │
│  ├── 文件系统操作 (本地)                                    │
│  ├── Shell 执行 (本地)                                      │
│  └── LLM API 调用                                           │
└─────────────────────────────────────────────────────────────┘

vs Claude Code (终端工具):
┌─────────────────────────────────────────────────────────────┐
│  Terminal UI (Ink + React)                                  │
│  └── 需要 Bridge 连接外部 IDE (VS Code/JetBrains)           │
└─────────────────────────────────────────────────────────────┘
```

### ❌ 不需要的功能

| 功能 | 原因 |
|------|------|
| IDE Bridge | Omiga 本身就是 IDE，无需连接外部 IDE |
| VS Code 扩展 | 不适用，Omiga 是独立应用 |

### ✅ 需要的功能

| 功能 | 优先级 | 说明 |
|------|--------|------|
| **权限系统** | P0 | 所有工具调用的安全保障 |
| **Gateway 消息平台** | P1 | 让手机/IM 也能使用 Omiga |
| **多后端执行** | P1 | Docker/SSH 安全沙箱 |
| **上下文压缩** | P2 | 长会话 Token 优化 |

---

## 🔥 Phase 1: 权限系统 (立即开始)

### 为什么权限系统是 P0？

Omiga 作为 IDE，会执行：
- `file_write` - 修改代码文件
- `bash` - 运行命令
- `web_fetch` - 网络请求

**没有权限控制 = 安全风险**

### 实施步骤

#### Week 1: 基础架构

```rust
// 1. 完善类型定义 (已提供)
src-tauri/src/domain/permissions/
├── mod.rs          ✅ 已创建
├── types.rs        ✅ 已创建
├── patterns.rs     ✅ 已创建
├── manager.rs      ⬜ 需要实现
├── engine.rs       ⬜ 规则匹配
└── audit.rs        ⬜ 审计日志
```

#### Week 2: 核心实现

```rust
// manager.rs 核心方法
impl PermissionManager {
    pub async fn check_permission(&self, context: &PermissionContext) -> PermissionDecision;
    pub async fn add_rule(&self, rule: PermissionRule) -> Result<()>;
    pub async fn assess_risk(&self, context: &PermissionContext) -> RiskAssessment;
}
```

#### Week 3: 前端 UI

```tsx
// src/components/permissions/
├── PermissionDialog.tsx        # 权限确认弹窗
├── PermissionSettings.tsx      # 设置面板
└── DangerousCommandAlert.tsx   # 危险命令警告
```

#### Week 4: 集成测试

---

## 📱 Phase 2: Gateway 消息平台 (高价值)

### 价值：让 Omiga 成为"远程开发助手"

```
用户场景:
1. 在手机上通过 Telegram 发送指令
2. Omiga 在桌面端执行
3. 结果推送到手机

例:
手机: "帮我把 userService.ts 里的 getUser 改成 async"
Omiga: (本地执行修改)
手机: "修改完成，Diff: ..."
```

### 实现方案

```rust
// src-tauri/src/gateway/
├── mod.rs
├── manager.rs
├── telegram.rs    # Telegram Bot
└── session_router.rs

// 在 App.tsx 添加 Gateway 状态面板
<GatewayStatus />
```

### 配置示例

```yaml
# omiga.yaml 添加
gateway:
  telegram:
    enabled: true
    bot_token: "${TELEGRAM_BOT_TOKEN}"
  slack:
    enabled: false
```

---

## 🐳 Phase 3: 多后端执行

### 价值：安全隔离 + 远程开发

```rust
// 在设置中选择执行后端
enum ExecutionBackend {
    Local,      // 本地执行 (默认)
    Docker,     // Docker 容器
    Ssh,        // SSH 远程
}
```

### 使用场景

1. **Docker 后端**: 运行不可信代码
   ```
   用户: "帮我运行这个 Python 脚本"
   Omiga: 在 Docker 中执行，不污染本地环境
   ```

2. **SSH 后端**: 远程服务器开发
   ```
   用户: 连接到服务器修改配置
   Omiga: SSH 执行，本地编辑体验
   ```

---

## 📊 调整后的路线图

```
Week 1-4:   权限系统 (P0) - 安全保障
Week 5-8:   Gateway (P1) - 移动接入
Week 9-12:  多后端 (P1) - 安全/远程
Week 13-14: 上下文压缩 (P2) - 优化
```

---

## 🚀 立即开始

### 今天就可以做的：

```bash
# 1. 进入权限系统目录
cd omiga/src-tauri/src/domain/permissions

# 2. 已有的文件
ls -la
# mod.rs      ✅
# types.rs    ✅
# patterns.rs ✅

# 3. 接下来实现 manager.rs
touch manager.rs

# 4. 添加到 Cargo.toml
cd ../../..
grep -n "permissions" Cargo.toml || echo "需要添加 dependencies"
```

### manager.rs 骨架代码：

```rust
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct PermissionManager {
    rules: Arc<RwLock<Vec<PermissionRule>>>,
    patterns: DangerousPatternDB,
}

impl PermissionManager {
    pub fn new() -> Self {
        Self {
            rules: Arc::new(RwLock::new(Vec::new())),
            patterns: DangerousPatternDB::new(),
        }
    }
    
    /// 核心检查方法
    pub async fn check(&self, tool: &str, args: &serde_json::Value) -> PermissionDecision {
        // 1. 风险评估
        let risks = self.check_dangerous_patterns(tool, args);
        
        // 2. 如果有危险，要求确认
        if !risks.is_empty() {
            return PermissionDecision::RequireApproval(risks);
        }
        
        // 3. 检查规则
        if let Some(rule) = self.find_matching_rule(tool, args).await {
            return self.apply_rule(rule);
        }
        
        // 4. 默认允许（或询问）
        PermissionDecision::Allow
    }
    
    fn check_dangerous_patterns(&self, tool: &str, args: &serde_json::Value) -> Vec<DetectedRisk> {
        if tool != "bash" && tool != "shell" {
            return vec![];
        }
        
        let cmd = args.get("command").and_then(|v| v.as_str()).unwrap_or("");
        self.patterns.check(cmd)
    }
}
```

---

## 💡 关键决策

### 决策 1: 先做权限系统还是 Gateway？

**建议: 权限系统**

原因:
1. 安全是基础，没有权限控制不敢用工具
2. 实现相对独立，不影响其他功能
3. 用户立即能感受到价值（放心使用）

### 决策 2: 需要多后端吗？

**建议: 先做 Docker**

原因:
1. 本地开发很少需要 SSH
2. Docker 提供安全隔离，更有价值
3. 实现相对简单

---

## 📋 本周任务清单

- [ ] 完成 `manager.rs` 基础框架
- [ ] 集成到 Tauri AppState
- [ ] 创建前端 `PermissionDialog` 组件
- [ ] 在工具调用前检查权限
- [ ] 测试危险命令拦截 (rm -rf /)

---

*调整日期: 2026-04-09*  
*核心原则: 先做安全，再做扩展*
