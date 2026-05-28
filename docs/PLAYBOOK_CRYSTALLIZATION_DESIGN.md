# Playbook 固化系统设计 (Task Graph Crystallization)

> 把"被验证过的执行轨迹"逐步晋升为"可检索、可重放、带指纹"的确定性流程,
> 在保住 Agent 灵活性的同时,为高重复任务提供流程化的稳定性与 token 节省。

---

## 1. 背景与目标

### 1.1 问题

Agent 对**重复任务**每次都重新规划:既浪费 token,又因 LLM 规划的非确定性导致结果
flaky。人类的做法是"第一次深思,之后变成反射"。我们要把这种"程序性记忆"工程化。

### 1.2 目标

- **省 token**:重复任务跳过"重新规划"(但**不跳过验证**)。
- **稳定性**:用冻结、验证过的任务图消除规划方差。
- **不牺牲灵活性**:只有指纹精确命中才走反射路径,其余作"先验"偏置推理。
- **可治理**:固化必须经人工确认,可审计、可撤销、可自动退役。

### 1.3 非目标(明确不做)

- 不做基于相似度的"跳过思考"——那是会反射性犯错的陷阱。
- 不引入 embedding / 向量库等重型基础设施(首版用现有关键词检索即可)。
- 不静默改写 Operator / Template / Skill(沿用现有 proposal-first 边界)。

---

## 2. 实现状态审计(2026-05-27,文档真相校准)

设计必须建在"既真实又已接线"的模块上。下表是当前对相关代码的核实结论。

| 模块 | 状态 | 证据 / 位置 |
|---|---|---|
| operator 运行 + chain | ✅ 真实现 & 已接线 & 写 `ExecutionRecord` | operator runtime 成功/失败路径 best-effort 写 `kind:"operator"` 记录 |
| template 运行 | ✅ 真实现 & 已接线 & 写 `ExecutionRecord` | `domain/templates.rs` 创建/更新 `kind:"template"` 记录 |
| template → backing operator 血缘 | ✅ 已接线 | Template 父记录会通过 `runContext.parentExecutionId` 传给 delegated/rendered backing Operator |
| ExecutionRecord 捕获 | ✅ operator/template 已捕获; daily/planner ❌ | 生产路径已有 `"operator"` 与 `"template"`;日常 chat/turn 和 planner 轨迹仍未统一捕获 |
| ExecutionRecord 字段完整度 | 🟡 **仍非完整重放载体** | 有 `param_hash`、`input_hash`、metadata/runtime/output summary 等;仍缺稳定 version 字段和完整、脱敏、可重放 params payload |
| learning_proposals 治理流 | ✅ 真实现 & 已接线 | `domain/learning_proposals.rs::generate_learning_proposals_from_records`;lib.rs 注册 `learning_proposal_*` |
| self_evolution 草稿/晋升 | ✅ 真实现 & 已接线 | lib.rs 注册 `self_evolution_drafts::*`(草稿 placeholder 是设计上要求人工替换) |
| TaskGraph 数据模型 | ✅ 真(类型可复用) | `research_system/models.rs::TaskGraph` |
| JsonFileTaskGraphStore | ✅ 真实现,🟡 仅 mock CLI 使用 | 只在 `research_system/cli.rs::run_flow` 实例化 |
| memory / pageindex 检索 | ✅ 真(关键词/词频打分,**无 embedding**) | `pageindex/query.rs::score_terms_against_text` |
| **research_system 整条流水线** | 🟡 **Mock 脚手架** | `run_flow` 对 director+executor 全用 `MockAgentRunner` |
| `LlmProviderAgentRunner` | ❌ 孤儿代码 | 定义存在,生产路径零实例化 |
| 日常 chat/turn 轨迹捕获 | ❌ 完全没有 | `commands/chat/` 内零个 `record_execution` |
| `crystallize_workflow` proposal + 血缘蒸馏 | ❌ 没有 | 现有 proposal 只基于**单条**记录 |
| Fingerprint 结构 / PlaybookStore | ❌ 没有 | `param_hash`/`canonical_id` 原料齐备,但 **version 与可重放 payload 需在执行当下从活 spec/invocation 采集** |

### 审计带来的硬性约束

1. **MVP 可以建在 operator/template ExecutionRecord 上**——二者都已是生产捕获路径。
   但 chat/turn 与 planner 轨迹仍未统一捕获,所以日常任务固化仍只能作为后续阶段接入。
2. **指纹 / 可重放数据在"执行成功的当下"从活 spec + invocation 采集**,不从历史
   `ExecutionRecord` 反推。原因:记录里仍无稳定 version 字段、无完整脱敏 params payload。
   `ExecutionRecord` 可作 provenance 与哈希来源,但不能独自承担 replay。
   因此 Phase 0 的接口应优先是 `Fingerprint::from_parts(canonical_id, version,
   param_hash, env)`,或显式接收活 spec/invocation 的 builder,而不是只靠历史记录。
3. **`TaskGraph` 不直接作重放载体**:它是规划结构(goal/constraints),不含具体
   operator params。MVP 用 operator 原生的可重放模型(`canonical_id` + 完整 `params`
   + `PlaybookVerification`)。`VerificationSpec` 等规划类型留待 Phase 2+ 评估复用。
4. **不假设 planner 在生产产图**;接通 research_system 需先把 `LlmProviderAgentRunner`
   接线 —— 独立前置工作,本设计不含。
5. **L1 模糊匹配不得复用 `fuzzy_match.rs`**(那是 Edit 工具的文本查找替换);
   语义层先用 memory 关键词打分,且因可靠性顾虑**推迟到 Phase 3**。

---

## 3. 核心概念

### 3.1 Playbook(剧本)

一份*被验证过、可重放、带指纹*的任务图。"剧本"语义自带护栏——它是操作手册,
不是模糊联想。

### 3.2 晋升阶梯

任务从"全新"到"完全稳定"是一条光谱,**落在哪一级由指纹质量决定,而非走哪条路径**:

```
L0 全新 / 指纹模糊   → 完整推理,产出新轨迹
L1 相似 / 指纹部分   → Playbook 作"先验"注入上下文(偏置推理,不替代)
L2 稳定 / 指纹精确   → 确定性重放 Playbook(不调 LLM 规划,但仍跑验证)
```

```
指纹干净(operator/template) ── 可达 L2
指纹模糊(日常自由对话)      ── 大多止于 L1;最强者晋升为 Skill/Operator
```

最右一格:日常轨迹中最稳定的,应经现有 `self_evolution_creator` 晋升为 Skill/Operator
(指纹随之变干净),而非永久停留在被重放的 Playbook。

---

## 4. 架构:五阶段闭环

```
  ① 捕获          ② 蒸馏           ③ 晋升          ④ 匹配          ⑤ 执行+反馈
ExecutionRecord → learning      → PlaybookStore → resolve_      → 守卫执行 →
(✅已有)          proposal         (❌新,.omiga/    playbook       回写 health
                  (✅扩展)          playbooks/)      (❌新)                  ↓
                                                                    失败→降级 L0
     ↑___________________________________________________________________|
                       auto-demote:成功率跌破阈值 → 退役回 proposal 队列
```

| 阶段 | 落点 | 现状 |
|---|---|---|
| ① 捕获 | `domain/execution_records.rs` | operator/template ✅;daily/planner ❌ |
| ② 蒸馏 | `domain/learning_proposals.rs` | 扫描框架 ✅;血缘蒸馏 + 新 proposal 类型 ❌ |
| ③ 晋升 | 新 `domain/playbooks/`(照搬 `JsonFileTaskGraphStore`) | ❌ |
| ④ 匹配 | 新 resolver + operator 执行入口前置 hook | ❌ |
| ⑤ 执行+反馈 | operator 执行守卫 + health 回写 | ❌ |

---

## 5. 数据模型(尽量复用现有类型)

```rust
/// 匹配与失效的唯一依据。逐字段相等才算 Exact。
struct Fingerprint {
    canonical_id: String,        // 来自 ExecutionRecord(已有)
    operator_version: String,    // 来自 operators metadata.version —— 失效触发器
    param_schema_hash: String,   // 输入形状(由 param_hash 派生)
    env_signature: Option<String>, // 运行时/平台(必要时)
}

struct Playbook {
    playbook_id: String,
    title: String,
    fingerprint: Fingerprint,
    graph: TaskGraph,            // ★ 直接复用 research_system/models.rs::TaskGraph
    param_slots: Vec<ParamSlot>, // 蒸馏时被参数化的位置
    provenance: Provenance {     // 审计,呼应 proposal-first
        distilled_from: Vec<String>, // execution_id
        proposal_id: Option<String>,
        approved_at: String,
    },
    health: Health {             // 闭环反馈端
        hit_count: u64,
        success_count: u64,
        last_verified_at: String,
        status: Active | Stale | Quarantined,
    },
}
```

> `graph` 复用 `TaskGraph` 后,`TaskSpec` 里的 `success_criteria` / `verification` /
> `stop_conditions` 全部继承,**验证契约无需重新设计**。

---

## 6. 可靠性守卫(每个失败模式对一个机制)

"可靠产品"与"能跑的 demo"的分界线。每条都非可选项。

| 失败模式 | 守卫机制 | 落点 |
|---|---|---|
| 反射性地做错(假阳性匹配) | **仅指纹逐字段相等走 L2**;模糊一律降级 L1,绝不绕过推理 | resolver `Exact` 判定 |
| 缓存图过期 | 指纹含 `operator_version`,版本一变指纹不匹配,自动失效 | 指纹计算 |
| "省 token 又保准确"的张力 | **VerificationSpec 在所有路径都跑**;验证挂 → 立即降级 L0 | 执行守卫 |
| 静默改写核心实现 | proposal-first:固化必须 approve,`provenance` 可审计、可撤销 | 沿用 learning 流 |
| 能力固化 / 局部最优 | **探索阀门**:命中后以概率 ε(或陈旧度超阈值)仍冷执行一次,对比并回写 | 执行守卫 |
| 缓存腐烂没人管 | **health 统计 + auto-demote**:成功率跌破阈值 → `Quarantined`,退出匹配池 | 反馈回写 |
| 在不该用的地方滥用 | **scope guard**:operator/template 域允许 L2;chat/research 仅允许 L1 | resolver 开关 |

---

## 7. 算法效率 & 代码简洁准则

本设计在工程上遵守以下硬性准则(贯穿所有阶段):

- **匹配 O(1)**:Playbook 按 `Fingerprint` 的稳定哈希建索引,精确匹配是 HashMap 查表,
  **绝不**全表线性扫描。失效判定也归结为哈希不等,无额外开销。
- **复用胜过新建(DRY)**:
  - `PlaybookStore` 照搬 `JsonFileTaskGraphStore` 的结构,不另造持久化机制。
  - 图类型复用 `TaskGraph`,治理流复用 `learning_proposals`,晋升复用 `self_evolution`。
- **避免过早复杂化(YAGNI)**:L1 语义匹配先用现成关键词打分,**不引入 embedding/向量库**;
  确有需求再升级。首版甚至可只做 L2、完全不做 L1。
- **best-effort 记录**:轨迹捕获沿用 `record_execution_best_effort` 语义,
  **记录失败绝不拖垮主执行流**。
- **小文件高内聚**:新增模块按 200–400 行/文件拆分(指纹、store、resolver、守卫各一),
  单文件不超 800 行,与项目编码规范一致。
- **纯函数优先**:指纹计算、匹配判定写成无副作用纯函数,便于全单测覆盖(目标 ≥80%)。
- **不可变更新**:health 回写返回新 Playbook 副本,不就地修改。

---

## 8. 分阶段计划清单

按"风险从低到高、价值从高到低"排序。每阶段可独立交付、独立验证假设。

### Phase 0 — 地基对齐(纯后端,无 LLM,可全单测)
- [ ] 定义 `Fingerprint` 结构 + 稳定哈希
- [ ] 定义 `Playbook` / `ParamSlot` / `Provenance` / `Health`,`graph` 复用 `TaskGraph`
- [ ] 实现 `PlaybookStore`(`.omiga/playbooks/`,照搬 `JsonFileTaskGraphStore`),
      CRUD + **按指纹哈希建索引**(O(1) 查表)
- [ ] `fingerprint_from_execution(...)`:显式接收 `ExecutionRecord` + 活 operator/template
      spec,用 `canonical_id`、`param_hash`、spec version/env 计算;不要假设历史记录本身
      含完整 replay payload
- [ ] 单测:指纹稳定性、版本变更→指纹变化、store round-trip、O(1) 查表

### Phase 1 — L2 精确重放 MVP(operator 域,验证核心假设)
- [ ] 工具 `operator_playbook_save`:用户对**一次成功的** operator 执行手动存为 Playbook
      (先跳过自动蒸馏)
- [ ] `resolve_playbook(intent, env) -> Exact | Hint | None`,Phase 1 只实现 **Exact**
- [ ] operator 执行入口前置 resolver hook;命中 Exact → 走重放
- [ ] **守卫执行**:重放时强制跑 `VerificationSpec`;失败 → 立即降级 L0 + 记日志
- [ ] **指纹失效**:`operator_version` 变 → 指纹不匹配 → 自动失效(无需额外逻辑)
- [ ] health 回写:`hit_count`/`success_count`/`last_verified_at`
- [ ] **auto-demote**:成功率跌破阈值 → `Quarantined`,退出匹配池
- [ ] **探索阀门**:命中后以概率 ε(或陈旧度超阈值)仍冷执行一次,对比并回写
- [ ] E2E:① 同 operator+同参第二次命中 ② 升版后失效回退 ③ 验证失败降级 ④ 探索阀门触发
- [ ] **假设验证**:误命中率≈0?重放确实省 token 且不掉准确率?——过了则核心假设成立

### Phase 2 — 自动蒸馏 + proposal 治理(把"手动存"变"自动建议")
- [ ] `learning_proposals` 新增类型 `crystallize_workflow`
- [ ] **血缘蒸馏**:沿 `parent_execution_id` 重建血缘 → 参数化 `TaskGraph`
      (具体值替换为 `param_slots`);触发条件:成功 ∧ 过验证 ∧ 同指纹出现 ≥N 次
- [ ] approve→apply 写入 `PlaybookStore`(**完全复用现有 proposal-first 流**)
- [ ] 前端:proposal 卡片(沿用现有学习建议 UI)+ Playbook 管理面板(列表/health/手动退役)

### Phase 3 — 接通日常任务(补捕获缺口 → L1)
- [ ] `commands/chat/turn.rs` / `tool_exec.rs` 记录会话轨迹:
      `record_execution_best_effort(kind="trajectory", session_id=...)`
- [ ] 意图提取 → 候选生成**复用 memory 关键词打分**(`score_terms_against_text`),
      **不**用 `fuzzy_match.rs`,**不**引 embedding
- [ ] resolver `Hint` 路径:matched Playbook 作"先验"注入 turn/planner 上下文(偏置,不替代)
- [ ] 最强轨迹出口:接现有 `self_evolution_creator`,晋升为 Skill/Operator
- [ ] **scope guard 配置**:operator/template 域允许 L2;chat/research 仅允许 L1

### Phase 4 — 跨路径统一 + 可观测(依赖前置工作)
- [ ] (前置)research_system 接通真 `LlmProviderAgentRunner` —— 独立任务
- [ ] `ExecutionRecord` 加 `task_graph_id`,把 planner 的 TaskGraph 喂进固化基底
- [ ] resolver 统一前置到三入口(operator entry / turn loop / planner)
- [ ] 可观测:累计 `tokens_saved`、命中率、降级率 dashboard

---

## 9. 风险与开放问题

- **指纹粒度**:`param_schema_hash` 取"参数形状"还是"参数值"?太粗会误命中,太细永不命中。
  Phase 1 用"值哈希"最保守(只命中完全相同的输入),Phase 2 再放宽到"形状+槽位"。
- **探索阀门的 ε 取值**:过大浪费、过小僵化。建议从 ε=0.1 起,按 dashboard 调。
- **research_system 的归宿**:它当前是 mock 脚手架。是接通真 runner 后纳入固化基底,
  还是维持独立?Phase 4 前需单独决策。
- **跨项目共享**:Playbook 是否随项目走(`.omiga/`)还是可导出共享?首版仅项目级。

---

## 10. 一句话总结

固化能力的等级由**指纹质量**决定,而非任务走哪条路径。operator 先行,因为它指纹最干净
且已被捕获;planner 与日常任务各自补不同缺口接入。本质是"把验证过的轨迹逐步晋升为
确定性流程",而不是"用相似度跳过思考"。
---

## 11. Phase 1 实现决议(MVP,Wave 2)

调查链执行流后定下两项决议,破解"匹配/省 token"的循环困境:

**决议一:可重放单元 = operator 链(多步)。** `run_operator_chain(steps: Vec<ChainStep>)`
是固化目标;省的是"链路规划"——token 成本真正所在。单 template 调用省不了多少(参数已定)。

**决议二:凭引用重放(类 Skill)。**
- 发现的循环:在 `run_operator_chain` 处挂钩省不了 token(LLM 已把 steps 想好传入);
  而"意图→链"的预规划匹配是模糊的(= 推迟到 Phase 3 的 L1 难题)。
- 破解:Playbook 像 Skill 一样**带描述暴露给 agent**;agent 自己识别并调
  `playbook_replay(id)` 直接重放整条链,**不重新推理重建 steps**。
- 指纹职责收窄为:① 重放前**失效检测**(算子版本漂移→拒绝重放,回退正常规划);② 重放后**验证**。
  **不**用于预规划匹配(避开模糊误命中陷阱)。

**Wave 2 模块划分(契约冻结后并行)**:
- `chain.rs`(Codex C/写侧):`chain_canonical_id` / `chain_composite_version` / `build_chain_playbook`
  ——`kind="chain"`,`params` 存序列化 `Vec<ChainStep>`,指纹经 `Fingerprint::from_invocation`。
- `replay.rs`(Codex D/读+反馈侧):`resolve_for_replay`(重算指纹比对→Ready/Invalidated/NotFound/Inactive)、
  `record_replay_outcome`(health 回写 + 成功率阈值 auto-demote)、`should_explore`(探索阀门)。
- 集成(orchestrator):`playbook_replay`/`playbook_list` 工具 + 成功链保存 + 算子版本解析 +
  verify-on-replay 接入实时路径(冲突易发胶水,亲自接线)。
