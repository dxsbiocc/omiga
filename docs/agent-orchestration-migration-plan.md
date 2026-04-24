# Agent Orchestration Migration Plan: oh-my-codex → Omiga

**版本:** v1.0  
**日期:** 2026-04-17  
**目标:** 将 oh-my-codex (OMX) 的完整 Agent 编排架构迁移并融合到 Omiga (Tauri 桌面应用) 中，升级为 v0.3.0

---

## 一、背景与目标

### 当前收敛待办（2026-04-22）

- [x] `/schedule` 真实执行链路已验证，不再先走 general/tool 主入口后再转编排
- [x] 编排完成后的总结 / 下一步建议，统一接回 orchestration 完成消息链路
- [x] 移除任务区右下角悬浮“后台任务运行中”提示，避免与任务面板信息重复
- [x] 输入框上方后台 Agent 路由条仅展示仍可跟进的运行中任务，完成后自动收起
- [ ] 任务面板顶部信息继续降噪，避免窄宽度下标题与 chip 过密/遮挡
- [ ] 为任务面板与对话区补一版更稳定的“执行中回执”展示，减少“计划已执行但中间无反馈”的割裂感
- [ ] 统一 Agent 头像 / 视觉层级，减少 dashboard、trace、task list 的认知负担

### oh-my-codex 核心架构优势

| 能力 | OMX 实现 | Omiga 现状 |
|------|----------|-----------|
| 45+ 专业 Agent 角色 | `src/agents/definitions.ts` | 4 个内置 Agent |
| 39 个工作流 Skill | `/skills/*.md` | Skill 调用框架已有 |
| 8 种编排模式 | ralph/team/autopilot 等 | 仅基础对话 |
| 关键词→Skill 自动路由 | `hooks/keyword-detector.ts` | 无 |
| 智能模型路由 | Frontier/Standard/Spark 三档 | 单一模型配置 |
| 持久化状态机 | `.omx/state/*.json` | 内存+SQLite |
| Runtime Overlay | `hooks/agents-overlay.ts` | 无 |
| Ralph 持久循环 | 最多 50 轮迭代+3 次修复 | MAX_TOOL_ROUNDS=100 |
| 团队并行编排 | Tmux 多 pane | BackgroundAgentManager |
| MCP 专用服务器 | 5 个专用 MCP server | 基础 MCP 集成 |

### 迁移核心原则

1. **保留 Omiga 优势**：多提供商 LLM、Tauri 原生 UI、SQLite 持久化、执行环境切换
2. **移植 OMX 智慧**：Agent 角色体系、编排模式、Skill 系统、关键词路由
3. **用 Tauri 替代 Tmux**：原生窗口/任务管理替代 tmux 多 pane 协调
4. **渐进式升级**：分 5 个阶段，每阶段可独立部署

---

## 二、架构蓝图

### 升级后系统架构

```
┌─────────────────────────────────────────────────────────┐
│                     Frontend (React)                     │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐   │
│  │ Skill    │ │ Mode     │ │ Agent    │ │ Team     │   │
│  │ Browser  │ │ Selector │ │ Monitor  │ │ Dashboard│   │
│  └──────────┘ └──────────┘ └──────────┘ └──────────┘   │
├─────────────────────────────────────────────────────────┤
│                 Tauri IPC Commands                       │
├──────────────┬──────────────┬────────────────────────────┤
│  Orchestrator│  Skill Engine│  Agent Registry            │
│  (Ralph/Team/│  (39 skills) │  (45+ roles + routing)    │
│   Autopilot) │              │                            │
├──────────────┴──────────────┴────────────────────────────┤
│              Domain Layer (Rust)                         │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐   │
│  │ Keyword  │ │ Model    │ │ State    │ │ Memory   │   │
│  │ Detector │ │ Router   │ │ Machine  │ │ System   │   │
│  └──────────┘ └──────────┘ └──────────┘ └──────────┘   │
├─────────────────────────────────────────────────────────┤
│              Infrastructure                              │
│  SQLite  │  .omiga/state/  │  MCP Servers  │  LLM APIs  │
└─────────────────────────────────────────────────────────┘
```

---

## 三、分阶段迁移计划

---

### Phase 1：Agent 角色体系扩展（预计 3-4 天）

**目标**：将 OMX 的 45+ 专业 Agent 角色融入 Omiga 现有的 `AgentDefinition` trait 体系

#### 1.1 扩展 AgentDefinition Trait

**文件**：`src-tauri/src/domain/agents/definition.rs`（新建，从 `builtins/` 提取）

```rust
pub trait AgentDefinition: Send + Sync {
    fn agent_type(&self) -> &str;
    fn display_name(&self) -> &str;
    fn description(&self) -> &str;
    fn when_to_use(&self) -> &str;
    fn category(&self) -> AgentCategory;          // 新增
    fn reasoning_effort(&self) -> ReasoningEffort; // 新增：low/medium/high
    fn model_class(&self) -> ModelClass;           // 新增：frontier/standard/spark
    fn posture(&self) -> AgentPosture;             // 新增：executor/reviewer/analyst
    fn system_prompt(&self, ctx: &AgentContext) -> String;
    fn allowed_tools(&self) -> Option<Vec<String>>;
    fn disallowed_tools(&self) -> Option<Vec<String>>;
    fn max_turns(&self) -> Option<usize>;
    fn background(&self) -> bool;
}

pub enum AgentCategory {
    Build,      // explore, analyst, planner, architect, debugger, executor, verifier
    Review,     // style-reviewer, quality-reviewer, security-reviewer, code-reviewer
    Domain,     // test-engineer, dependency-expert, git-master, researcher
    Product,    // product-manager, ux-researcher, product-analyst
    Coordination, // critic, vision, orchestrator
}

pub enum ModelClass {
    Frontier,  // 最高推理能力，复杂编排角色
    Standard,  // 主力开发、审查
    Spark,     // 轻量任务，频繁调用
}

pub enum ReasoningEffort {
    Low,
    Medium,
    High,
}
```

#### 1.2 实现 30 个核心 Agent 角色

**新建目录**：`src-tauri/src/domain/agents/roles/`

按 OMX 定义移植以下角色（优先级排序）：

**一级（Build/Analysis 核心）：**
- `executor.rs` - 任务执行者，所有工具访问
- `architect.rs` - 架构设计，高推理，Frontier 模型
- `debugger.rs` - 调试专家，Bash+Read 工具
- `explorer.rs` - 代码库探索（已有，升级）
- `planner.rs` - 任务规划（已有，升级）
- `analyst.rs` - 需求分析
- `verifier.rs` - 完成验证，标准模型

**二级（Review 专家）：**
- `code_reviewer.rs` - 代码审查
- `security_reviewer.rs` - 安全审查
- `performance_reviewer.rs` - 性能优化
- `style_reviewer.rs` - 代码风格
- `api_reviewer.rs` - API 设计审查
- `quality_reviewer.rs` - 质量保证

**三级（Domain 专家）：**
- `test_engineer.rs` - TDD 专家
- `build_fixer.rs` - 构建错误修复
- `dependency_expert.rs` - 依赖管理
- `git_master.rs` - Git 操作
- `researcher.rs` - 技术研究
- `writer.rs` - 文档撰写
- `designer.rs` - UI/UX 设计建议
- `qa_tester.rs` - QA 测试

**四级（Product/Coordination）：**
- `product_manager.rs` - PRD 生成
- `critic.rs` - 批判性评估
- `orchestrator.rs` - 多 Agent 协调（升级现有 Coordinator）

#### 1.3 Agent Prompt 文件迁移

**新建目录**：`src-tauri/resources/agent-prompts/`

从 OMX `/prompts/*.md` 移植 25 个角色提示词，转为 Rust `include_str!()` 资源或运行时文件加载。

每个 prompt 文件结构：
```markdown
# {Role} Agent

## Identity
...

## Execution Policy
...

## Tool Access
...

## Verification Protocol
...
```

#### 1.4 Agent Registry（注册中心）

**新建文件**：`src-tauri/src/domain/agents/registry.rs`

```rust
pub struct AgentRegistry {
    agents: HashMap<String, Box<dyn AgentDefinition>>,
}

impl AgentRegistry {
    pub fn new() -> Self {
        let mut r = Self { agents: HashMap::new() };
        // 注册所有内置 agent
        r.register(Box::new(ExecutorAgent));
        r.register(Box::new(ArchitectAgent));
        // ... 所有角色
        r
    }

    pub fn get(&self, agent_type: &str) -> Option<&dyn AgentDefinition>;
    pub fn list_by_category(&self, category: AgentCategory) -> Vec<&dyn AgentDefinition>;
    pub fn find_best_for_task(&self, task_desc: &str) -> &dyn AgentDefinition;
}
```

---

### Phase 2：智能模型路由（预计 2 天）

**目标**：实现 OMX 的 Frontier/Standard/Spark 三档模型路由策略

#### 2.1 Model Router

**新建文件**：`src-tauri/src/domain/agents/model_router.rs`

```rust
pub struct ModelRouter {
    frontier_model: ModelConfig,  // 最强推理：架构决策、复杂编排
    standard_model: ModelConfig,  // 主力：开发、审查、大多数任务
    spark_model: ModelConfig,     // 轻量：简单查询、频繁调用的 worker
}

impl ModelRouter {
    pub fn select_model(&self, agent: &dyn AgentDefinition, task: &Task) -> ModelConfig {
        match agent.model_class() {
            ModelClass::Frontier => self.frontier_model.clone(),
            ModelClass::Standard => self.standard_model.clone(),
            ModelClass::Spark => self.spark_model.clone(),
        }
    }

    pub fn from_config(llm_config: &LlmConfig) -> Self {
        // 从 omiga.yaml 读取三档模型配置
        // 支持同提供商不同模型（如 claude-opus-4/claude-sonnet-4/claude-haiku-4）
    }
}
```

#### 2.2 omiga.yaml 配置扩展

```yaml
# 现有配置
default_provider: anthropic

# 新增：模型路由配置
model_routing:
  frontier:
    model: claude-opus-4-7          # 最复杂任务
    reasoning_effort: high
  standard:
    model: claude-sonnet-4-6        # 主力模型
    reasoning_effort: medium
  spark:
    model: claude-haiku-4-5         # 轻量任务
    reasoning_effort: low

# 新增：编排配置
orchestration:
  max_team_agents: 6               # 最大并行 agent 数
  max_fix_attempts: 3              # ralph 模式最大修复次数
  max_ralph_iterations: 50         # ralph 最大迭代次数
  visual_verdict_threshold: 90     # 视觉验证评分阈值
```

#### 2.3 前端设置页更新

**文件**：`src/components/Settings/index.tsx`

新增"高级编排"设置面板：
- 三档模型选择器（Frontier / Standard / Spark）
- 并行 Agent 数量滑块（1-6）
- Ralph 模式配置（最大迭代、修复次数）
- 超时与重试策略

---

### Phase 3：Skill 系统完整移植（预计 4-5 天）

**目标**：将 OMX 的 39 个 Skill 完整移植为 Omiga 可调用的工作流模板

#### 3.1 Skill 数据模型扩展

**现有基础**：`src-tauri/src/domain/tools/skill_invoke.rs`（已有框架）

**扩展 Skill Schema**：
```rust
pub struct SkillDefinition {
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub category: SkillCategory,
    pub when_to_use: Vec<String>,
    pub when_not_to_use: Vec<String>,
    pub triggers: Vec<String>,      // 关键词触发词
    pub steps: Vec<SkillStep>,      // 执行步骤
    pub required_agents: Vec<String>, // 所需 agent 角色
    pub output_artifacts: Vec<String>, // 产出物（prd、test-spec 等）
}

pub enum SkillCategory {
    Development,    // tdd, code-review, build-fix, refactor
    Planning,       // plan, deep-interview, ralplan, autopilot
    Orchestration,  // ralph, team, swarm, ultrawork
    Research,       // autoresearch, web-research
    Quality,        // ultraqa, security-review, visual-verdict
    Documentation,  // document-release, wiki
}
```

#### 3.2 核心 Skill 实现（优先级排序）

**一级（立即实现）：**

| Skill 名 | OMX 原型 | 核心逻辑 |
|----------|---------|---------|
| `ralph` | `skills/ralph/SKILL.md` | 持久循环 + 验证 |
| `team` | `skills/team/SKILL.md` | 并行阶段执行 |
| `autopilot` | `skills/autopilot/SKILL.md` | 全自动流水线 |
| `plan` | `skills/plan/SKILL.md` | 结构化规划 |
| `tdd` | `skills/tdd/SKILL.md` | 测试驱动开发 |
| `code-review` | `skills/code-review/SKILL.md` | 多维代码审查 |
| `build-fix` | `skills/build-fix/SKILL.md` | 构建错误修复循环 |

**二级（第二批）：**

| Skill 名 | 功能 |
|----------|------|
| `deep-interview` | Socratic 需求澄清 |
| `ralplan` | 高风险变更审议 |
| `security-review` | 安全审查流程 |
| `ultraqa` | QA 循环测试 |
| `autoresearch` | 自主研究 |
| `document-release` | 发布后文档更新 |
| `visual-verdict` | UI 视觉评分 |
| `ecomode` | 低 token 预算模式 |

#### 3.3 Skill 执行引擎

**新建文件**：`src-tauri/src/domain/skills/engine.rs`

```rust
pub struct SkillEngine {
    registry: SkillRegistry,
    state_manager: SkillStateManager,
    agent_scheduler: AgentScheduler,
}

impl SkillEngine {
    pub async fn invoke(
        &self,
        skill_name: &str,
        ctx: &SkillContext,
        app: &AppHandle,
    ) -> Result<SkillResult, SkillError> {
        let skill = self.registry.get(skill_name)?;
        let state = self.state_manager.create_state(skill_name)?;

        // 执行 skill 的各个阶段
        for step in skill.steps() {
            self.execute_step(step, &state, ctx, app).await?;
            self.state_manager.checkpoint(&state).await?;
        }

        Ok(SkillResult { artifacts: state.artifacts() })
    }
}
```

#### 3.4 Skill 状态持久化

**新建目录**：`.omiga/state/skills/`

```
.omiga/state/
├── skill-active.json           # 当前活跃 skill 状态
├── skills/
│   ├── ralph-state.json        # ralph 循环状态
│   ├── team-state.json         # team 执行阶段
│   └── sessions/
│       └── {session_id}/       # session 级别状态覆盖
```

---

### Phase 4：编排模式实现（预计 5-6 天）

**目标**：实现 OMX 的核心编排模式，用 Tauri 原生机制替代 tmux

#### 4.1 Ralph 持久循环模式

**新建文件**：`src-tauri/src/domain/orchestration/ralph.rs`

```rust
pub struct RalphOrchestrator {
    max_iterations: usize,      // 默认 50
    max_fix_attempts: usize,    // 默认 3
    verifier: Box<dyn AgentDefinition>,
}

pub struct RalphState {
    pub iteration: usize,
    pub fix_attempts: usize,
    pub phase: RalphPhase,
    pub context_snapshot: PathBuf,  // .omiga/context/{slug}-{ts}.md
    pub progress_log: Vec<ProgressEntry>,
}

pub enum RalphPhase {
    PreContext,     // 任务快照
    Planning,       // 规划
    Execution,      // 专家委托执行
    Verification,   // 架构师验证
    Fixing,         // 错误修复
    Complete,
    Failed,
}

impl RalphOrchestrator {
    pub async fn run(
        &self,
        task: &str,
        ctx: &ToolContext,
        app: &AppHandle,
    ) -> Result<RalphResult, RalphError> {
        let state = self.init_state(task).await?;

        loop {
            if state.iteration >= self.max_iterations { break; }

            // 1. 快照当前上下文
            self.snapshot_context(&state, ctx).await?;

            // 2. 委托专家并行执行
            let results = self.delegate_to_specialists(&state, ctx, app).await?;

            // 3. 架构师验证
            let verdict = self.verify(&state, &results, ctx, app).await?;

            if verdict.is_complete() {
                state.phase = RalphPhase::Complete;
                break;
            }

            // 4. 修复循环
            if verdict.needs_fix() {
                if state.fix_attempts >= self.max_fix_attempts {
                    state.phase = RalphPhase::Failed;
                    break;
                }
                state.fix_attempts += 1;
                self.run_fix_cycle(&state, &verdict, ctx, app).await?;
            }

            state.iteration += 1;
            self.persist_state(&state).await?;
        }

        Ok(RalphResult::from_state(&state))
    }
}
```

#### 4.2 Team 并行编排模式

**新建文件**：`src-tauri/src/domain/orchestration/team.rs`

OMX 的 tmux 多 pane 方案替换为 Tauri 的 BackgroundAgentManager：

```rust
pub struct TeamOrchestrator {
    max_workers: usize,         // 默认 6
    max_fix_attempts: usize,    // 默认 3
}

pub enum TeamPhase {
    Planning,       // leader 规划任务分配
    Prd,            // 生成 PRD 和测试规范
    Execution,      // 并行 worker 执行
    Verification,   // 验证所有产出
    Fixing,         // 修复失败项
    Complete,
    Failed,
}

pub struct TeamState {
    pub phase: TeamPhase,
    pub leader_agent: String,
    pub workers: Vec<WorkerState>,
    pub phase_log: Vec<PhaseTransition>,
}

pub struct WorkerState {
    pub task_id: String,
    pub agent_type: String,
    pub assigned_slice: String,  // 分配的任务片段
    pub status: BackgroundAgentStatus,
    pub blockers: Vec<String>,
}

impl TeamOrchestrator {
    pub async fn run(
        &self,
        task: &str,
        ctx: &ToolContext,
        app: &AppHandle,
    ) -> Result<TeamResult, TeamError> {
        // Phase 1: Leader 规划
        let plan = self.plan_phase(task, ctx, app).await?;

        // Phase 2: PRD 生成（如需）
        if plan.needs_prd {
            self.prd_phase(&plan, ctx, app).await?;
        }

        // Phase 3: 并行执行（最多 6 个 worker）
        let workers = self.spawn_workers(&plan, ctx, app).await?;
        self.await_workers(&workers).await?;

        // Phase 4: 验证
        let verification = self.verify_phase(&workers, ctx, app).await?;

        // Phase 5: 修复循环（最多 3 次）
        for attempt in 0..self.max_fix_attempts {
            if verification.all_passed() { break; }
            self.fix_phase(&verification, ctx, app).await?;
        }

        Ok(TeamResult::collect(&workers))
    }

    async fn spawn_workers(
        &self,
        plan: &TeamPlan,
        ctx: &ToolContext,
        app: &AppHandle,
    ) -> Result<Vec<WorkerHandle>, TeamError> {
        // 使用现有的 BackgroundAgentManager
        let manager = BackgroundAgentManager::global(app);
        let mut handles = vec![];

        for (i, slice) in plan.task_slices.iter().enumerate() {
            let handle = manager.spawn_agent(
                &plan.worker_agents[i],
                slice,
                ctx,
                app,
            ).await?;
            handles.push(handle);
        }

        Ok(handles)
    }
}
```

#### 4.3 Autopilot 全自动流水线

**新建文件**：`src-tauri/src/domain/orchestration/autopilot.rs`

```rust
pub enum AutopilotPhase {
    Interview,      // 可选：deep-interview 澄清需求
    Planning,       // ralplan 审议计划
    Execution,      // ralph 或 team 执行
    Verification,   // verifier 验证
    Documentation,  // 更新文档
    Complete,
}

impl AutopilotOrchestrator {
    pub async fn run(&self, task: &str, ctx: &ToolContext, app: &AppHandle) {
        // 按 autopilot 流水线完整执行
        // 根据任务复杂度自动选择 ralph vs team 模式
    }
}
```

#### 4.4 编排模式注册与分发

**新建文件**：`src-tauri/src/domain/orchestration/mod.rs`

```rust
pub enum OrchestrationMode {
    Solo,           // 单 agent 直接执行（当前模式）
    Ralph,          // 持久循环
    Team,           // 并行团队
    Autopilot,      // 全自动流水线
    Ralplan,        // 高风险变更审议
    DeepInterview,  // 需求澄清
    UltraQA,        // QA 循环
    AutoResearch,   // 自主研究
}

pub struct OrchestrationDispatcher {
    ralph: RalphOrchestrator,
    team: TeamOrchestrator,
    autopilot: AutopilotOrchestrator,
}

impl OrchestrationDispatcher {
    pub async fn dispatch(
        &self,
        mode: OrchestrationMode,
        task: &str,
        ctx: &ToolContext,
        app: &AppHandle,
    ) -> Result<OrchestrationResult, OrchestrationError> {
        match mode {
            OrchestrationMode::Ralph => self.ralph.run(task, ctx, app).await,
            OrchestrationMode::Team => self.team.run(task, ctx, app).await,
            OrchestrationMode::Autopilot => self.autopilot.run(task, ctx, app).await,
            OrchestrationMode::Solo => self.run_solo(task, ctx, app).await,
            // ...
        }
    }
}
```

---

### Phase 5：关键词检测与智能路由（预计 2-3 天）

**目标**：实现 OMX 的关键词→Skill 自动路由机制

#### 5.1 关键词检测器

**新建文件**：`src-tauri/src/domain/routing/keyword_detector.rs`

```rust
pub struct KeywordDetector {
    rules: Vec<KeywordRule>,
}

pub struct KeywordRule {
    pub keywords: Vec<String>,      // 触发词（不区分大小写）
    pub target_skill: String,       // 目标 skill 名
    pub priority: u8,               // 1-10，高优先级覆盖低优先级
    pub match_type: MatchType,      // Explicit（精确）/ Implicit（语义）
}

// 从 OMX 移植的关键词映射：
impl KeywordDetector {
    pub fn default_rules() -> Vec<KeywordRule> {
        vec![
            KeywordRule {
                keywords: vec!["ralph", "don't stop", "keep going", "持续执行"],
                target_skill: "ralph".to_string(),
                priority: 10,
                match_type: MatchType::Explicit,
            },
            KeywordRule {
                keywords: vec!["team", "swarm", "parallel", "并行"],
                target_skill: "team".to_string(),
                priority: 9,
                match_type: MatchType::Explicit,
            },
            KeywordRule {
                keywords: vec!["plan this", "规划", "制定计划"],
                target_skill: "plan".to_string(),
                priority: 8,
                match_type: MatchType::Explicit,
            },
            KeywordRule {
                keywords: vec!["autopilot", "自动", "完全自动"],
                target_skill: "autopilot".to_string(),
                priority: 9,
                match_type: MatchType::Explicit,
            },
            KeywordRule {
                keywords: vec!["tdd", "test first", "测试驱动"],
                target_skill: "tdd".to_string(),
                priority: 7,
                match_type: MatchType::Implicit,
            },
            KeywordRule {
                keywords: vec!["security", "安全审查", "漏洞"],
                target_skill: "security-review".to_string(),
                priority: 7,
                match_type: MatchType::Implicit,
            },
            KeywordRule {
                keywords: vec!["review", "代码审查", "check my"],
                target_skill: "code-review".to_string(),
                priority: 6,
                match_type: MatchType::Implicit,
            },
            // ... 更多规则
        ]
    }

    pub fn detect(&self, message: &str) -> Option<SkillRoute> {
        // 从高到低检测优先级
        // 返回匹配的 skill 路由
    }
}
```

#### 5.2 Ralplan 安全门

OMX 的"ralph 在 PRD+test-spec 存在前不能执行"规则迁移到 Omiga：

```rust
pub struct RalplanGate {
    pub required_artifacts: Vec<ArtifactType>,  // prd, test-spec
}

impl RalplanGate {
    pub fn check(&self, session_id: &str) -> GateResult {
        // 检查 .omiga/plans/ 中是否存在 prd-*.md 和 test-spec-*.md
        // 如不存在，拦截 ralph 执行，提示先运行 ralplan
    }
}
```

#### 5.3 消息预处理中间件

**修改文件**：`src-tauri/src/commands/chat.rs`

在 `send_message` 入口处添加关键词检测：

```rust
pub async fn send_message(
    app: AppHandle,
    app_state: State<'_, OmigaAppState>,
    request: SendMessageRequest,
) -> CommandResult<MessageResponse> {
    // 新增：关键词检测与 skill 路由
    let route = keyword_detector.detect(&request.content);

    if let Some(SkillRoute { skill_name, mode }) = route {
        // 将消息路由到对应的 skill/orchestration 模式
        return dispatch_to_skill(skill_name, mode, &request, &app, &app_state).await;
    }

    // 原有逻辑...
}
```

---

### Phase 6：AGENTS.md Runtime Overlay 系统（预计 2-3 天）

**目标**：实现 OMX 的 Runtime Overlay——在每次 LLM 调用前动态注入上下文到系统提示词

#### 6.1 Overlay Manager

**新建文件**：`src-tauri/src/domain/agents/overlay.rs`

```rust
pub struct AgentOverlayManager {
    max_overlay_chars: usize,   // 默认 3500（OMX 限制）
}

pub struct OverlayContext {
    pub codebase_map: Option<String>,        // token-efficient 代码库结构图
    pub active_mode_state: Option<String>,   // 当前 ralph 迭代、autopilot 阶段
    pub notepad_content: Option<String>,     // 优先级 notepad 内容
    pub project_memory_summary: Option<String>, // 技术栈、约定
    pub session_metadata: SessionMetadata,
}

impl AgentOverlayManager {
    pub fn build_system_prompt(
        &self,
        base_prompt: &str,
        overlay: &OverlayContext,
    ) -> String {
        // 将 overlay 注入 base_prompt 中的标记位置
        // <!-- OMIGA:RUNTIME:START --> ... <!-- OMIGA:RUNTIME:END -->
        // 严格控制 max_overlay_chars
    }

    pub async fn build_overlay(
        &self,
        session_id: &str,
        ctx: &ToolContext,
    ) -> Result<OverlayContext, OverlayError> {
        // 并行获取：代码库地图、活跃状态、notepad、项目记忆
    }
}
```

#### 6.2 代码库地图生成器

**新建文件**：`src-tauri/src/domain/agents/codebase_map.rs`

```rust
pub struct CodebaseMapGenerator {
    max_tokens: usize,      // token-efficient，紧凑格式
}

impl CodebaseMapGenerator {
    pub async fn generate(&self, project_root: &Path) -> Result<String, MapError> {
        // 生成紧凑的目录/模块结构摘要
        // 格式：类似 tree 输出但过滤无关文件
        // 包含：主要模块、关键文件、最近修改
    }
}
```

---

### Phase 7：前端 UI 升级（预计 4-5 天）

**目标**：为新的编排能力提供完整的 UI 支持

#### 7.1 Skill Browser（Skill 浏览器）

**新建文件**：`src/components/SkillBrowser/index.tsx`

功能：
- 按分类展示所有可用 Skill（Development/Planning/Orchestration/Research/Quality）
- 每个 Skill 显示：名称、描述、适用场景、所需 Agent
- 点击调用：直接触发 Skill 执行，预填充当前任务
- 搜索过滤

#### 7.2 Orchestration Mode Selector（编排模式选择器）

**修改文件**：`src/components/Chat/Composer.tsx`（或新建 Composer 扩展）

功能：
- 消息输入框旁的模式下拉：Solo / Ralph / Team / Autopilot
- 选中 Ralph/Team 时显示配置选项（迭代数、并行 Agent 数）
- 快捷键触发（如 `/ralph`、`/team` 前缀）

#### 7.3 Agent Monitor（Agent 监控面板）

**新建文件**：`src/components/AgentMonitor/index.tsx`

功能（扩展现有的 Background Agent 面板）：
- 实时显示所有活跃 Agent（主 Agent + 所有 Worker）
- 每个 Agent 卡片：角色、状态、当前工具调用、Token 使用
- Ralph 模式：显示当前迭代数、验证分数、修复次数
- Team 模式：显示阶段（Planning→PRD→Execution→Verification→Complete）
- 可单独取消某个 Worker

#### 7.4 Team Dashboard（团队编排面板）

**新建文件**：`src/components/TeamDashboard/index.tsx`

功能：
- 团队执行的甘特图式时间线
- Worker 任务分配可视化
- 阶段转换日志
- 集体产出物（PRD、test-spec）链接

#### 7.5 Settings 高级编排配置

**修改文件**：`src/components/Settings/index.tsx`

新增"编排"配置 Tab：
- 模型路由配置（Frontier/Standard/Spark 三档）
- Ralph 模式参数（最大迭代、修复次数、验证阈值）
- Team 模式参数（最大 Worker 数、阶段超时）
- Keyword 路由规则管理（查看/禁用默认规则、添加自定义规则）
- Ralplan 安全门配置（高风险变更列表）

#### 7.6 活跃 Skill 状态指示器

在聊天界面 header 添加：
- 当前活跃 Skill badge（如 "● Ralph [iteration 3/50]"）
- 当前 Team 阶段指示器（如 "Team › Execution [2/6 workers]"）

---

## 四、数据流与状态管理

### 升级后的状态结构

```
.omiga/
├── memory/                    # 现有记忆系统（保留）
│   ├── wiki/
│   └── implicit/
├── state/                     # 新增编排状态
│   ├── skill-active.json      # 当前活跃 skill
│   ├── sessions/
│   │   └── {session_id}/
│   │       ├── ralph-state.json
│   │       ├── team-state.json
│   │       └── skill-active.json
│   ├── ralph-state.json       # root 级 ralph 状态
│   └── team-state.json        # root 级 team 状态
├── plans/                     # 新增计划产出物
│   ├── prd-{slug}-{ts}.md
│   └── test-spec-{slug}-{ts}.md
├── context/                   # 新增 ralph 上下文快照
│   └── {slug}-{ts}.md
└── wiki/                      # 移植自 OMX wiki
    └── *.md
```

### Zustand Store 扩展

**新建文件**：`src/state/orchestrationStore.ts`

```typescript
interface OrchestrationStore {
  // 当前活跃模式
  activeMode: OrchestrationMode | null;
  activeSkill: string | null;

  // Ralph 状态
  ralphState: {
    iteration: number;
    maxIterations: number;
    phase: RalphPhase;
    fixAttempts: number;
  } | null;

  // Team 状态
  teamState: {
    phase: TeamPhase;
    workers: WorkerState[];
    phaseLog: PhaseTransition[];
  } | null;

  // Actions
  setActiveMode: (mode: OrchestrationMode | null) => void;
  updateRalphState: (state: RalphState) => void;
  updateTeamState: (state: TeamState) => void;
}
```

---

## 五、Tauri IPC 新增命令

**文件**：`src-tauri/src/commands/orchestration.rs`（新建）

```rust
// 编排模式控制
#[tauri::command]
pub async fn start_orchestration_mode(
    mode: String,
    task: String,
    config: OrchestrationConfig,
    app: AppHandle,
    state: State<'_, OmigaAppState>,
) -> CommandResult<OrchestrationStartResult>;

#[tauri::command]
pub async fn cancel_orchestration(
    mode: String,
    session_id: String,
    app: AppHandle,
) -> CommandResult<()>;

#[tauri::command]
pub async fn get_orchestration_state(
    session_id: String,
    state: State<'_, OmigaAppState>,
) -> CommandResult<OrchestrationState>;

// Skill 管理
#[tauri::command]
pub async fn list_skills(
    category: Option<String>,
) -> CommandResult<Vec<SkillDefinition>>;

#[tauri::command]
pub async fn invoke_skill(
    skill_name: String,
    context: SkillInvokeContext,
    app: AppHandle,
    state: State<'_, OmigaAppState>,
) -> CommandResult<SkillInvokeResult>;

// Agent 管理
#[tauri::command]
pub async fn list_agent_roles(
    category: Option<String>,
) -> CommandResult<Vec<AgentRoleInfo>>;

#[tauri::command]
pub async fn get_model_routing_config(
    state: State<'_, OmigaAppState>,
) -> CommandResult<ModelRoutingConfig>;

#[tauri::command]
pub async fn update_model_routing_config(
    config: ModelRoutingConfig,
    state: State<'_, OmigaAppState>,
) -> CommandResult<()>;
```

---

## 六、关键风险与缓解策略

| 风险 | 影响 | 缓解措施 |
|------|------|---------|
| Tmux → Tauri 并行架构差异 | Team 模式实现复杂度高 | 复用现有 BackgroundAgentManager，Phase 3 团队协调模式先用轮询验证 |
| Ralph 循环超时 | 长任务 > Tauri 2 min 超时 | 用 Background Agent + Tauri Events 异步通信；每轮 checkpoint |
| Agent Prompt 翻译质量 | 中文项目提示词效果 | 双语提示词（英文主体+中文示例）；保留 OMX 英文模版备用 |
| 模型路由配置复杂度 | 用户体验 | 预设 3 种套餐（轻量/标准/旗舰），高级用户手动配置 |
| .omiga/state 与 SQLite 冲突 | 状态不一致 | 定义明确分工：SQLite 存消息/对话历史；.omiga/state 存编排状态（瞬态） |
| Skill 执行中断恢复 | 长 Skill 失败无法续传 | 每个 Skill Step 完成后写入 checkpoint；重启时检测并询问是否继续 |

---

## 七、执行计划与里程碑

### 开发时间线（约 20 个工作日）

```
Week 1 (Day 1-5):
  Day 1-2: Phase 1 - Agent 角色体系（Rust 核心数据结构）
  Day 3-4: Phase 1 - 移植 20 个 Agent 角色实现
  Day 5:   Phase 2 - 模型路由（ModelRouter + omiga.yaml 扩展）

Week 2 (Day 6-10):
  Day 6-7: Phase 3 - Skill 数据模型 + SkillEngine 核心
  Day 8-9: Phase 3 - 移植一级 7 个核心 Skill
  Day 10:  Phase 3 - Skill 状态持久化

Week 3 (Day 11-15):
  Day 11-12: Phase 4 - Ralph 持久循环实现
  Day 13-14: Phase 4 - Team 并行编排实现
  Day 15:    Phase 5 - 关键词检测器 + 消息路由中间件

Week 4 (Day 16-20):
  Day 16-17: Phase 6 - Runtime Overlay + 代码库地图
  Day 18-19: Phase 7 - 前端 UI（Skill Browser + Agent Monitor）
  Day 20:    Phase 7 - Settings 高级配置 + 集成测试
```

### 里程碑检查点

| 里程碑 | 验收标准 |
|-------|---------|
| M1: Agent Registry 完成 | `list_agent_roles` 返回 20+ 角色；Agent 可按 ModelClass 路由 |
| M2: Skill Engine 可用 | `invoke_skill("tdd", ...)` 完整执行 TDD 工作流 |
| M3: Ralph 可运行 | `/ralph` 触发持久循环，状态写入 .omiga/state/ralph-state.json |
| M4: Team 可运行 | `/team` 触发 6 worker 并行，阶段状态更新 |
| M5: UI 完整 | Skill Browser 展示所有 Skill；Agent Monitor 实时更新 |
| M6: v0.3.0 发布 | 所有 Phase 完成，E2E 测试通过，文档更新 |

---

## 八、与现有代码的兼容性

### 保持不变的部分

- ✅ SQLite 持久化层（`domain/persistence/`）
- ✅ 所有 30+ 现有工具（`domain/tools/`）
- ✅ LLM 多提供商适配器（`llm/`）
- ✅ 执行环境切换（SSH/Docker/本地）
- ✅ 现有 4 个内置 Agent（GeneralPurpose/Plan/Explore/Verification）
- ✅ 现有 Memory 系统（wiki + implicit）
- ✅ 现有 Background Agent Manager
- ✅ Coordinator 模式
- ✅ 现有 Settings UI

### 升级/扩展的部分

- 🔄 `AgentDefinition` trait：增加 category/model_class/reasoning_effort 字段
- 🔄 `send_message` 命令：前端加关键词检测中间件
- 🔄 `AgentScheduler`：集成 ModelRouter
- 🔄 Settings：新增"编排"配置 Tab
- 🔄 `omiga.yaml`：新增 model_routing 和 orchestration 配置段

### 新增的部分

- ➕ `domain/agents/roles/`：20+ 新 Agent 角色
- ➕ `domain/agents/registry.rs`：Agent 注册中心
- ➕ `domain/agents/model_router.rs`：三档模型路由
- ➕ `domain/agents/overlay.rs`：Runtime Overlay 管理
- ➕ `domain/skills/`：完整 Skill 系统
- ➕ `domain/orchestration/`：ralph/team/autopilot 编排器
- ➕ `domain/routing/keyword_detector.rs`：关键词路由
- ➕ `commands/orchestration.rs`：新 Tauri 命令
- ➕ `src/components/SkillBrowser/`：Skill 浏览器
- ➕ `src/components/AgentMonitor/`：Agent 监控
- ➕ `src/components/TeamDashboard/`：团队面板
- ➕ `src/state/orchestrationStore.ts`：编排状态

---

## 九、参考资源

- **OMX Agent 定义**：`/oh-my-codex/src/agents/definitions.ts`
- **OMX Skill 模版**：`/oh-my-codex/skills/*/SKILL.md`
- **OMX Ralph 技术**：`/oh-my-codex/src/ralph/persistence.ts`
- **OMX Team 编排**：`/oh-my-codex/src/team/orchestrator.ts`
- **OMX 关键词检测**：`/oh-my-codex/src/hooks/keyword-detector.ts`
- **OMX Runtime Overlay**：`/oh-my-codex/src/hooks/agents-overlay.ts`
- **Omiga Agent 基础**：`src-tauri/src/domain/agents/`
- **Omiga Background Agent**：`src-tauri/src/domain/agents/background.rs`
- **Omiga 现有 Skill 工具**：`src-tauri/src/domain/tools/skill_invoke.rs`
