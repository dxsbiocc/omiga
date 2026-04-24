# Agent 编排真实运行态验收准备清单 v1

> 目标：为 `/schedule`、`/team`、`/autopilot` 三条主链路准备**真实运行态验收环境**。  
> 适用前提：Mock 场景验收已经通过，当前剩余的核心阻塞是“缺少可运行的 LLM provider 环境”。

---

## 1. 当前阻塞结论

当前项目已经具备：

- 主聊天输入框编排入口
- Dashboard / Timeline / Phase History / Trace Panel
- 真实 orchestration event log
- Mock orchestration harness

当前尚未完成的关键项是：

> **真实 provider 环境下的端到端运行态验收**

也就是还没有真正验证：

- `/schedule`
- `/team`
- `/autopilot`

在真实 LLM 执行环境中是否能够完整闭环。

---

## 2. 推荐的最小可行路径

为了最小代价完成真实运行态验收，建议优先顺序如下：

### 路线 A：Custom(OpenAI-compatible) 本地/私有兼容端点

推荐指数：**最高**

适用场景：

- 已有本地 vLLM / LM Studio / Ollama OpenAI-compatible 代理
- 已有公司内部兼容 OpenAI Chat Completions 的服务

优点：

- 不依赖 Anthropic / OpenAI 官方控制台
- 对本地调试最友好
- 可控性高，便于后续形成稳定验收环境

建议模型能力：

- 至少支持多轮 chat completion
- 最好支持 tool calling
- 如果不支持 tool calling，则只能做部分链路验收

---

### 路线 B：OpenAI / DeepSeek / Moonshot 等现成 provider

推荐指数：**次高**

适用场景：

- 已有现成 API key
- 希望尽快完成一次真实云端运行态验收

优点：

- 部署快
- 很容易立刻开始跑

缺点：

- 成本不可控
- 稳定性依赖外部服务
- 不一定适合长期 CI / 回归

---

### 路线 C：继续只依赖 Mock Harness

推荐指数：**不推荐作为最终验收**

适用场景：

- UI / trace / event flow 开发调试

不足：

- 无法证明真实 LLM planner / worker / reviewer 链路
- 无法证明真实运行时闭环

---

## 3. 推荐执行方案

### 短期推荐

先用：

- **路线 B：一个最容易拿到 key 的真实 provider**

快速完成一次“真实端到端通过”。

### 中期推荐

再落地：

- **路线 A：Custom(OpenAI-compatible) 固定验收环境**

作为长期回归环境。

---

## 4. 真实运行态验收前必须满足的条件

以下条件必须至少满足其中一组：

### 方案 1：通过 Settings → ProviderManager 配置

在应用中：

- 打开 `Settings`
- 进入 Provider 管理
- 配置任一 provider：
  - OpenAI
  - Anthropic
  - DeepSeek
  - Moonshot
  - Custom(OpenAI-compatible)
- 设为默认或当前会话 active provider

建议验收优先选择：

- `Custom (OpenAI-compatible)`
- 或 `DeepSeek`

---

### 方案 2：通过 `omiga.yaml`

可在项目根目录放置：

- `omiga.yaml`

或用户目录：

- `~/.config/omiga/config.yaml`

配置 provider。

> 注：仓库中当前没有现成的 `config.example.yaml` 文件，但代码里支持生成/读取 `omiga.yaml` 形式配置。

---

### 方案 3：通过环境变量

例如：

#### OpenAI

```bash
export OPENAI_API_KEY="..."
export LLM_PROVIDER="openai"
```

#### DeepSeek

```bash
export DEEPSEEK_API_KEY="..."
export LLM_PROVIDER="deepseek"
```

#### Custom(OpenAI-compatible)

```bash
export LLM_PROVIDER="custom"
export LLM_API_KEY="dummy-or-real-key"
export LLM_BASE_URL="http://localhost:11434/v1/chat/completions"
export LLM_MODEL="your-model-name"
```

> 备注：如果端点不是标准 Chat Completions URL，需要按 Omiga 当前 custom provider 的格式提供兼容接口。

---

## 5. 推荐的最小 provider 配置顺序

为了让真实运行态验收尽快开始，建议按下面顺序尝试：

### P1. Custom OpenAI-compatible

如果你已经有：

- Ollama + OpenAI-compatible bridge
- vLLM
- LM Studio
- 公司内兼容端点

那这是最快路径。

---

### P2. DeepSeek

原因：

- 配置简单
- 国内可用性通常较好
- 对多轮编排测试足够友好

---

### P3. OpenAI / Anthropic / Moonshot

根据你手头 key 的可用性决定。

---

## 6. 真正开始验收前的检查表

在开始真实 provider 验收前，请确认：

- [ ] `Settings -> ProviderManager` 中至少有一个可用 provider
- [ ] 当前 session 已加载 active provider
- [ ] 能在普通聊天中收到至少一条 assistant 回复
- [ ] tool mode 未被全局关闭
- [ ] 当前会话已设置工作目录

如果以上任一项失败，不要直接跑 `/schedule /team /autopilot`，先修环境。

---

## 7. 真实运行态验收顺序

建议严格按下面顺序来，不要三条一起跑：

### Step 1：普通聊天冒烟

输入一条普通消息，例如：

- “输出一句 hello，并说明当前 provider 名称”

目标：

- 验证当前 provider 可用
- 验证流式返回正常

---

### Step 2：`/schedule`

建议输入：

```text
/schedule 把登录流程重构为 token refresh + error boundary，并补充验证
```

验收目标：

- scheduler plan 生成
- worker 启动
- reviewer verdict
- summary 完成

---

### Step 3：`/team`

建议输入：

```text
/team 修复导出流程里的并发问题，并确保结果可验证
```

验收目标：

- team phases 进入
- verifying
- 必要时 fixing
- synthesizing
- complete / failed

---

### Step 4：`/autopilot`

建议输入：

```text
/autopilot 实现一个带回归验证的设置同步功能
```

验收目标：

- implementation
- qa
- validation
- complete / stop

---

## 8. 每条真实场景需要记录什么

每次运行都应记录：

- 输入命令
- 当前 provider / model
- 是否产生 scheduler plan
- Timeline 中出现的关键事件
- Phase History 是否符合预期
- Dashboard 状态是否正确
- 是否出现 blocker
- 是否能 drill-down 到 transcript
- 最终状态：完成 / 部分完成 / 失败 / 取消

---

## 9. 通过标准

以下标准满足，才算该场景“真实运行态通过”：

### `/schedule`

- [ ] 生成计划
- [ ] 启动至少一个 worker
- [ ] reviewer verdict 可见
- [ ] 最终 summary 结束

### `/team`

- [ ] team phase 进入 executing
- [ ] verifying 可见
- [ ] 如失败可进入 fixing
- [ ] synthesizing 可见
- [ ] 最终 complete / failed

### `/autopilot`

- [ ] implementation 可见
- [ ] qa cycle 可见
- [ ] validation 可见
- [ ] 最终 complete / stop

---

## 10. 验收通过后建议删除/收敛的代码

在真实 provider 验收通过后，优先考虑收敛：

### 可以优先移除或隐藏

- `MockScenarioLauncher`
- Settings 中普通用户可见的 mock 场景入口

### 建议保留

- mock seed helper（用于回归测试）
- orchestration event log
- trace panel / phase history / timeline

原因：

- 前者是验收辅助暴露层
- 后者已经是系统真实运行时的一部分

---

## 11. 当前推荐动作

### 当前最推荐的下一步

1. 先选一个最容易接通的 provider  
2. 跑一次普通聊天冒烟  
3. 再依次跑：
   - `/schedule`
   - `/team`
   - `/autopilot`

不要再继续扩表面积，直到这三条真实运行态验收有明确结果。

