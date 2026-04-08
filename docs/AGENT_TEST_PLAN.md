# Agent 系统测试计划

## 测试目标

验证 Agent 系统的所有功能正常工作，包括 Agent 路由、模型选择、工具过滤和提示词生成。

---

## 测试环境

### 编译测试
```bash
cd omiga/src-tauri
cargo check
cargo test domain::agents
```

### 运行测试
```bash
cargo tauri dev  # 启动 Omiga
```

---

## 测试用例

### TC1: General-Purpose Agent（默认）

**目的**: 验证默认 Agent 正常工作

**输入**:
```json
{
  "description": "分析代码库",
  "prompt": "分析这个项目的架构，列出主要模块和它们的关系"
}
```

**预期结果**:
- ✅ 使用父会话模型
- ✅ 可以使用大部分工具
- ✅ 系统提示词包含 "Sub-agent mode (general-purpose)"
- ✅ 提示词中包含技能发现（如果存在 .claude/skills）

**验证方法**:
1. 在 Chat 中发送 Agent 工具调用
2. 检查日志中的模型名称
3. 检查返回的提示词内容

---

### TC2: Explore Agent

**目的**: 验证 Explore Agent 的轻量级探索功能

**输入**:
```json
{
  "description": "搜索文件",
  "prompt": "找到所有定义 User、Task 或 Session 结构的 Rust 文件",
  "subagent_type": "Explore"
}
```

**预期结果**:
- ✅ 使用 haiku 模型（轻量级）
- ✅ 系统提示词包含 "You are a fast agent"
- ✅ 自动省略 CLAUDE.md（更快的启动）
- ✅ **无法**调用 FileEdit、FileWrite、NotebookEdit

**验证方法**:
1. 观察模型名称（应为 haiku）
2. 尝试让 Agent 修改文件（应该失败）
3. 检查系统提示词不含 CLAUDE.md 相关内容

---

### TC3: Plan Agent

**目的**: 验证 Plan Agent 的架构设计能力

**输入**:
```json
{
  "description": "设计 API",
  "prompt": "设计一个 REST API 来处理用户认证，包括：\n1. 登录\n2. 注册\n3. 密码重置\n4. 令牌刷新\n\n请提供详细的端点设计和数据流",
  "subagent_type": "Plan"
}
```

**预期结果**:
- ✅ 使用父会话模型（继承）
- ✅ 系统提示词包含 "software architect"
- ✅ 自动省略 CLAUDE.md
- ✅ **无法**调用文件修改工具
- ✅ 返回结构化的实施计划

**验证方法**:
1. 检查模型名称（应与父会话相同）
2. 验证无法修改文件
3. 检查返回的计划格式（应该有清晰的结构）

---

### TC4: Verification Agent

**目的**: 验证 Verification Agent 的对抗性测试（需要先实现后台支持）

**输入**:
```json
{
  "description": "验证代码",
  "prompt": "验证 src/domain/agents/definition.rs 中的 AgentDefinition trait 实现，检查：\n1. 是否有潜在的类型安全问题\n2. 错误处理是否完善\n3. 是否有遗漏的边界情况",
  "subagent_type": "verification"
}
```

**预期结果**:
- ✅ 后台执行（不阻塞主会话）
- ✅ 返回 PASS/FAIL/PARTIAL 判定
- ✅ 系统提示词包含对抗性测试指导
- ✅ 红色标记（UI 中）

**验证方法**:
1. 当前：应返回 "not supported" 错误
2. 实现后台后：应返回后台任务 ID

---

### TC5: 模型选择优先级

**目的**: 验证模型选择优先级正确

**测试用例 5a: 用户指定模型**
```json
{
  "description": "测试",
  "prompt": "简单测试",
  "subagent_type": "Explore",
  "model": "sonnet"
}
```
**预期**: 使用 sonnet（覆盖 Explore 默认的 haiku）

**测试用例 5b: Agent 配置模型**
```json
{
  "description": "测试",
  "prompt": "简单测试",
  "subagent_type": "Explore"
}
```
**预期**: 使用 haiku（Explore 默认配置）

**测试用例 5c: 继承模型**
```json
{
  "description": "测试",
  "prompt": "简单测试",
  "subagent_type": "Plan"
}
```
**预期**: 使用父会话模型（Plan 配置为 "inherit"）

---

### TC6: 工具过滤

**目的**: 验证 Agent 级别的工具过滤

**测试方法**:
让 Explore Agent 尝试修改文件：
```json
{
  "description": "修改测试",
  "prompt": "请修改 src/main.rs，在开头添加一个注释",
  "subagent_type": "Explore"
}
```

**预期结果**:
- ✅ FileEdit 工具不在可用工具列表中
- ✅ Agent 无法调用 FileEdit
- ✅ 返回错误提示

---

### TC7: 计划模式支持

**目的**: 验证 Plan 模式下 ExitPlanMode 可用

**前置条件**: 父会话处于计划模式

**输入**:
```json
{
  "description": "计划模式测试",
  "prompt": "检查当前状态",
  "subagent_type": "Explore"
}
```

**预期结果**:
- ✅ 系统提示词包含 "ExitPlanMode is available"
- ✅ Agent 可以调用 ExitPlanMode

---

### TC8: 嵌套 Agent 阻止

**目的**: 验证默认阻止嵌套 Agent

**输入**:
```json
{
  "description": "嵌套测试",
  "prompt": "使用 Agent 工具再创建一个子 Agent",
  "subagent_type": "general-purpose"
}
```

**预期结果**:
- ✅ Agent 工具不在可用工具列表中（默认情况下）
- ✅ 无法创建嵌套 Agent

**特殊测试**（设置 USER_TYPE=ant）:
```bash
USER_TYPE=ant cargo tauri dev
```
**预期**: 嵌套 Agent 可用

---

## 自动化测试代码

### Rust 单元测试

```rust
// domain/agents/tests.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_router_select() {
        let router = AgentRouter::new();
        
        // 测试默认 Agent
        let agent = router.select_agent(None);
        assert_eq!(agent.agent_type(), "general-purpose");
        
        // 测试 Explore Agent
        let agent = router.select_agent(Some("Explore"));
        assert_eq!(agent.agent_type(), "Explore");
        
        // 测试不存在的 Agent 回退到默认
        let agent = router.select_agent(Some("nonexistent"));
        assert_eq!(agent.agent_type(), "general-purpose");
    }

    #[test]
    fn test_explore_agent_disallowed_tools() {
        use crate::domain::agents::builtins::explore::ExploreAgent;
        use crate::domain::agents::definition::AgentDefinition;
        
        let agent = ExploreAgent;
        let disallowed = agent.disallowed_tools().unwrap();
        
        assert!(disallowed.contains(&"Agent".to_string()));
        assert!(disallowed.contains(&"FileEdit".to_string()));
        assert!(disallowed.contains(&"FileWrite".to_string()));
    }

    #[test]
    fn test_model_resolution_priority() {
        // 用户指定 > Agent 配置 > 继承
        let parent_model = "claude-sonnet-4-6";
        
        // 情况 1: 用户指定
        let user_model = Some("haiku");
        let agent_config = Some("opus");
        let result = resolve_model_priority(user_model, agent_config, parent_model);
        assert_eq!(result, "haiku");
        
        // 情况 2: Agent 配置
        let user_model = None;
        let agent_config = Some("opus");
        let result = resolve_model_priority(user_model, agent_config, parent_model);
        assert_eq!(result, "opus");
        
        // 情况 3: 继承
        let user_model = None;
        let agent_config = Some("inherit");
        let result = resolve_model_priority(user_model, agent_config, parent_model);
        assert_eq!(result, "claude-sonnet-4-6");
    }
}
```

---

## 测试步骤

### 手动测试清单

1. **启动 Omiga**
   ```bash
   cd omiga/src-tauri
   cargo tauri dev
   ```

2. **测试默认 Agent**
   - [ ] 发送通用任务
   - [ ] 验证模型继承
   - [ ] 验证工具可用性

3. **测试 Explore Agent**
   - [ ] 发送探索任务
   - [ ] 验证使用 haiku 模型
   - [ ] 验证无法修改文件

4. **测试 Plan Agent**
   - [ ] 发送设计任务
   - [ ] 验证返回结构化计划
   - [ ] 验证无法修改文件

5. **测试模型选择**
   - [ ] 测试用户指定模型覆盖
   - [ ] 测试 Agent 默认配置
   - [ ] 测试继承行为

6. **测试工具过滤**
   - [ ] 让 Explore 尝试修改文件（应失败）
   - [ ] 让 General 使用搜索工具（应成功）

7. **测试错误处理**
   - [ ] 验证后台 Agent 返回错误
   - [ ] 验证嵌套 Agent 被阻止

---

## 预期问题

| 问题 | 可能性 | 解决方案 |
|------|--------|---------|
| Verification Agent 后台不支持 | 高 | 实现后台任务系统 |
| 模型名称不匹配 | 中 | 检查 resolve_subagent_model 映射 |
| 工具过滤不生效 | 中 | 检查工具名称匹配（大小写） |
| 提示词过长 | 低 | 优化 Agent 系统提示词 |

---

## 通过标准

- ✅ 所有 Agent 类型可以正常创建
- ✅ 模型选择优先级正确
- ✅ 工具过滤生效（Explore/Plan 无法修改文件）
- ✅ 系统提示词包含 Agent 特定内容
- ✅ 编译无警告
- ✅ 不破坏现有功能

---

## 记录

| 日期 | 测试项 | 结果 | 备注 |
|------|--------|------|------|
| | | | |
