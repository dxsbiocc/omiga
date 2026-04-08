# Agent 系统集成示例

## 修改 `run_subagent_session` 使用 Agent 路由

这是将新的 Agent 系统集成到现有 Chat 系统的示例代码。

### 修改后的 `run_subagent_session` 函数

```rust
// omiga/src-tauri/src/commands/chat.rs

async fn run_subagent_session(
    app: &AppHandle,
    message_id: &str,
    session_id: &str,
    tool_results_dir: &Path,
    project_root: &Path,
    session_todos: Option<Arc<Mutex<Vec<TodoItem>>>>,
    session_agent_tasks: Option<Arc<Mutex<Vec<AgentTask>>>>,
    args: &crate::domain::tools::agent::AgentArgs,
    runtime: &AgentLlmRuntime,
    subagent_execute_depth: u8,
    brave_search_api_key: Option<String>,
    skill_cache: Arc<StdMutex<skills::SkillCacheMap>>,
) -> Result<String, String> {
    // ===== 新增：使用 Agent 路由系统 =====
    let router = crate::domain::agents::get_agent_router();
    let agent = router.select_agent(args.subagent_type.as_deref());
    
    // 获取 Agent 配置
    let parent_in_plan = if let Some(ref pm) = runtime.plan_mode_flag {
        *pm.lock().await
    } else {
        false
    };
    
    // 构建 Agent 会话配置
    let agent_config = crate::domain::agents::prepare_agent_session_config(
        router,
        args.subagent_type.as_deref(),
        &runtime.llm_config.model,
        parent_in_plan,
        runtime.allow_nested_agent,
    );
    
    // 检查是否为后台 Agent
    if args.run_in_background == Some(true) || agent_config.background {
        return Err(
            "`run_in_background` is not supported for the Agent tool in Omiga yet.".to_string(),
        );
    }
    
    let effective_root = resolve_agent_cwd(project_root, args.cwd.as_deref());
    let subagent_skill_task_context = format!("{} {}", args.description.trim(), args.prompt.trim());
    
    // ===== 修改：使用 Agent 配置的模型 =====
    let mut sub_cfg = runtime.llm_config.clone();
    sub_cfg.model = agent_config.model;
    
    // Fast existence check for subagent
    let skills_exist = skills::skills_any_exist(&effective_root, &skill_cache).await;
    
    // ===== 修改：构建系统提示词 =====
    let mut prompt_parts: Vec<String> = Vec::new();
    
    // 基础系统提示词（来自 constants/agent_prompt.rs）
    prompt_parts.push(agent_prompt::build_system_prompt(
        &effective_root,
        &sub_cfg.model,
    ));
    
    // Agent 特定的系统提示词
    prompt_parts.push(agent_config.system_prompt);
    
    // 技能发现（如果存在）
    if skills_exist && !agent_config.omit_claude_md {
        prompt_parts.push(skills::format_skills_discovery_system_section());
    }
    
    sub_cfg.system_prompt = Some(prompt_parts.join("\n\n"));
    
    let client = create_client(sub_cfg).map_err(|e| e.to_string())?;
    
    // ===== 修改：使用 Agent 配置的工具过滤 =====
    let subagent_opts = SubagentFilterOptions {
        parent_in_plan_mode: parent_in_plan,
        allow_nested_agent: runtime.allow_nested_agent && 
            !agent_config.disallowed_tools.contains(&"Agent".to_string()),
    };
    
    let mut tools = build_subagent_tool_schemas(
        &effective_root,
        skills_exist,
        subagent_opts,
    )
    .await;
    
    // 应用 Agent 特定的工具过滤
    if let Some(ref allowed) = agent_config.allowed_tools {
        // 只允许指定的工具
        tools.retain(|t| allowed.contains(&t.name));
    }
    // 应用禁止的工具列表
    for disallowed in &agent_config.disallowed_tools {
        tools.retain(|t| &t.name != disallowed);
    }
    
    // ===== 用户任务提示词 =====
    let user_text = format!(
        "## Sub-agent task: {}\n\n{}",
        args.description.trim(),
        args.prompt.trim()
    );
    let mut transcript: Vec<Message> = vec![Message::User { content: user_text }];
    
    // ===== 执行 Agent 会话 =====
    for _round_idx in 0..MAX_SUBAGENT_TOOL_ROUNDS {
        if *runtime.cancel_flag.read().await {
            return Err("Sub-agent cancelled.".to_string());
        }
        
        let api_msgs = SessionCodec::to_api_messages(&transcript);
        let llm_messages = api_messages_to_llm(&api_msgs);
        
        let (tool_calls, assistant_text, cancelled) = stream_llm_response_with_cancel(
            client.as_ref(),
            app,
            message_id,
            &runtime.round_id,
            &llm_messages,
            &tools,
            &runtime.pending_tools,
            &runtime.cancel_flag,
            runtime.repo.clone(),
        )
        .await
        .map_err(|e| e.to_string())?;
        
        if cancelled {
            return Err("Sub-agent cancelled.".to_string());
        }
        
        let tc = completed_to_tool_calls(&tool_calls);
        transcript.push(Message::Assistant {
            content: assistant_text.clone(),
            tool_calls: tc.clone(),
        });
        
        if tool_calls.is_empty() {
            return Ok(assistant_text);
        }
        
        let results = execute_tool_calls(
            &tool_calls,
            app,
            message_id,
            session_id,
            tool_results_dir,
            &effective_root,
            session_todos.clone(),
            session_agent_tasks.clone(),
            Some(runtime),
            subagent_execute_depth,
            Some(subagent_skill_task_context.as_str()),
            brave_search_api_key.clone(),
            skill_cache.clone(),
        )
        .await;
        
        for (tool_use_id, output, _) in &results {
            transcript.push(Message::Tool {
                tool_call_id: tool_use_id.clone(),
                output: output.clone(),
            });
        }
    }
    
    Err(format!(
        "Sub-agent exceeded maximum tool rounds ({MAX_SUBAGENT_TOOL_ROUNDS})."
    ))
}
```

## 关键修改点

### 1. Agent 路由

```rust
let router = crate::domain::agents::get_agent_router();
let agent = router.select_agent(args.subagent_type.as_deref());
```

根据 `subagent_type` 参数自动选择正确的 Agent 定义。

### 2. 系统提示词构建

```rust
// 基础提示词 + Agent 特定提示词
prompt_parts.push(agent_prompt::build_system_prompt(...));
prompt_parts.push(agent_config.system_prompt);
```

系统提示词现在由两部分组成：
- 基础提示词（环境信息、工具说明等）
- Agent 特定的角色定义和行为准则

### 3. 工具过滤

```rust
// 应用 Agent 特定的工具限制
if let Some(ref allowed) = agent_config.allowed_tools {
    tools.retain(|t| allowed.contains(&t.name));
}
for disallowed in &agent_config.disallowed_tools {
    tools.retain(|t| &t.name != disallowed);
}
```

根据 Agent 定义动态过滤可用工具：
- **Explore/Plan Agent**: 禁止使用文件修改工具（FileEdit, FileWrite 等）
- **General-Purpose Agent**: 可以使用所有工具（除了防止递归的 Agent 工具）

### 4. 模型选择

```rust
sub_cfg.model = agent_config.model;
```

支持 Agent 级别的模型覆盖：
- `"inherit"` → 继承父会话模型
- `"sonnet"` / `"opus"` / `"haiku"` → 使用特定模型
- 具体模型 ID → 直接使用

## 使用示例

### 使用 Explore Agent

```rust
Agent({
    "description": "搜索代码库结构",
    "prompt": "找到所有使用 User 模型的文件，并分析它们的关系",
    "subagent_type": "Explore",
    "cwd": "/path/to/project"
})
```

**效果**:
- 使用轻量级模型（haiku）
- 禁止文件修改操作
- 优化的代码探索提示词

### 使用 Plan Agent

```rust
Agent({
    "description": "设计认证系统",
    "prompt": "设计一个基于 JWT 的认证系统，考虑刷新令牌、黑名单和安全最佳实践",
    "subagent_type": "Plan"
})
```

**效果**:
- 使用更强的推理模型（继承父模型）
- 返回结构化的实施计划
- 识别关键文件和依赖

### 使用 General-Purpose Agent（默认）

```rust
Agent({
    "description": "通用研究任务",
    "prompt": "研究这个代码库的架构，重点关注数据流和模块边界"
})
```

**效果**:
- 使用默认模型
- 可以使用所有工具
- 适合复杂的多步骤研究任务

## 下一步扩展

### 1. 添加 Verification Agent

创建 `domain/agents/builtins/verification.rs`:

```rust
pub struct VerificationAgent;

impl AgentDefinition for VerificationAgent {
    fn agent_type(&self) -> &str { "verification" }
    
    fn when_to_use(&self) -> &str {
        "Use this agent to verify that implementation work is correct..."
    }
    
    fn system_prompt(&self, _ctx: &ToolContext) -> String {
        "You are a verification specialist...".to_string()
    }
    
    fn disallowed_tools(&self) -> Option<Vec<String>> {
        Some(vec![
            "Agent".to_string(),
            "FileEdit".to_string(),
            "FileWrite".to_string(),
        ])
    }
    
    fn background(&self) -> bool { true } // 始终在后台运行
    fn color(&self) -> Option<&str> { Some("red") }
}
```

### 2. 支持后台 Agent

修改 `run_subagent_session` 支持异步执行：

```rust
if args.run_in_background == Some(true) || agent_config.background {
    // 启动后台任务
    tokio::spawn(async move {
        // 执行 Agent 会话
        // 完成后发送通知
    });
    
    return Ok("Agent started in background.".to_string());
}
```

### 3. 支持 Fork 模式

实现轻量级 Fork，继承父上下文：

```rust
// 在 AgentArgs 中添加 fork 参数
pub struct AgentArgs {
    // ... 现有字段 ...
    pub fork: Option<bool>,
}

// 在 run_subagent_session 中处理 fork
if args.fork == Some(true) {
    // 克隆父会话完整上下文
    // 共享 prompt cache
    // 异步执行
}
```
