# 下一轮优化计划（2026-07-09 评估产出）

> 本文档由 2026-07-09 的整体评估产出，作为下一次执行的任务书基础。
> 前序工作：G1-G17 见 `docs/OPTIMIZATION_GOALS.md`（在 `feature/projectized-sidebar` 分支上）；
> operator 环境供给五阶段（a85d923..f4f001d）已合并 main。
> 执行基线（main = f4f001d）：`cargo test --lib` = **1237 passed / 0 failed / 2 ignored**。

## 状态总览

| # | 目标 | 优先级 | 状态 |
|---|------|--------|------|
| N0 | 合并 `feature/projectized-sidebar` 进 main（G1-G17 成果落地） | **P0** | ✅ 完成（a56bbc0，2026-07-10）|
| N1 | operator 主执行链路继承环境清洗（env_hygiene 扩展） | P1 | ✅ 完成（829abf6+8c1290f）|
| N2 | `operators/mod.rs`（13.8k 行）/ `plugins.rs`（8.9k 行）G8 式拆分 | P1 | ✅ 完成（operators 8 阶段 f2483cd..12a14df；plugins 3 阶段 327864e..bb753d2）。遗留：两个 mod.rs 根测试区（各 ~3.2k 行）按域拆分、plugins/mod.rs 生产核心 ~2.7k 行可再分 manifest/loading 两域 |
| N3 | 网络代理式策略（对齐 codex network-proxy，堵子进程绕过） | P2 | ✅ 完成（d74de09/00c3b26/d5b00b9，macOS opt-in）。遗留：exec_session 长驻会话未纳入沙箱/代理（既有边界）；单例假设策略 env 恒定 |
| N4 | 远端 surface 的环境预检与预热（SSH/Modal/Daytona） | P2 | ✅ 完成（b1155bb N4a 探测 + N4b 预热）。生命周期钩子默认仍 local，会话 ctx 自动继承留后续 |
| N5 | conda lockfile 可复现性（conda-lock + ExecutionRecord 记录解析 hash） | P2 | ✅ 完成（f53938b N5a lock优先 + N5b 指纹）。指纹已入 provenance.json；ExecutionRecord 列表页展示需额外接线（可选后续） |
| N6 | 冗余清理（legacy operator 工具、migrationTarget、双轨自动化、分支垃圾） | P2 | 待办 |
| N7 | 测试稳定性治理（环境敏感测试、定时 flake、unreachable! 收敛） | P3 | 待办 |
| N8 | Windows 沙箱 + bash 静态危险分析（codex shell-command/execpolicy 对齐） | P3 | 待办 |
| N9 | G16 Landlock / G17 OTLP 的 Linux 实机验证 | P3 | 待办 |

## N0 合并 projectized-sidebar（P0，一切的前置）

**现状**：G 系列全部成果（25 提交、173 文件、+2.3万行）滞留分支未合并。main 上
`commands/chat/mod.rs` 仍是 5436 行巨石——G8 拆分成果在 main 上不存在。拖越久冲突面越大。

**做法**：merge（预期主要冲突点：`lib.rs` 命令注册、operator 相关文件与 f4f001d 的交叠——
2026-07-09 时确认过零重叠，合并前需复核）。合并后全量 `cargo test --lib` + 前端 `tsc` + vitest 作 gate。

**验收**：main 包含 G8 模块树与 G15-G17；全量测试全绿；分支删除。

## N1 operator 主执行链路继承环境清洗（P1）

**现状**：阶段4 只覆盖了显式导出（shell_export_lines）与预热/探测两个执行面；
operator 真正执行业务命令的子进程仍继承 app 全部环境变量（含 LLM API keys）。
codex 的 `ShellEnvironmentPolicy` 是全工具面策略。

**做法**：在 operator 执行 spawn 点（本地 execution 路径）复用 `domain/env_hygiene`
对继承环境做敏感名 env_remove，`OMIGA_ENV_KEEP` 豁免沿用；注意不能误伤 conda 激活
（PATH/HOME/CONDA_*/OMIGA_* 不在敏感模式内，需测试钉住）。远端 surface 的等价策略一并评估
（导出脚本侧已过滤，远端继承环境属远端宿主，明确边界即可）。
**风险**：用户依赖继承 token 的 operator 会破——发布说明 + OMIGA_ENV_KEEP 文档要同步。

**验收**：注入 FAKE_SECRET_TOKEN 后 operator 子进程内 printenv 不可见；conda 环境 operator 正常跑通；既有测试全绿。

## N2 operators/mod.rs 与 plugins.rs 拆分（P1，技术债）

**现状**：`domain/operators/mod.rs` 13814 行（G8 拆的 chat/mod.rs 的 2.5 倍）、
`domain/plugins.rs` 8890 行。模块注释自己承认"Revisit these clippy allowances when the
runtime is split into smaller registry/validation/execution modules"。

**做法**：完全复制 G8 方法论（见 OPTIMIZATION_GOALS.md 的 G8 完成记录）：分阶段提交、
每步 codex review + 主会话逐行集合比对、调用方零 diff、每轮全量测试 gate。
建议切分：registry / validation / execution / conda_env / container / scripts(shell 生成) / summary / preflight。

**验收**：单文件 ≤ 2000 行；调用方零 diff；测试数不减、全绿。

## N3 网络代理式策略（P2）

**现状**：G13 的 seatbelt 域名过滤**子进程可绕过**（文档已注明）；codex 用 network-proxy
强制所有子进程流量过本地代理实现真管控。

**做法**：参考 `codex-rs/network-proxy`；omiga 侧最小形态 = 本地 HTTP(S) 代理 +
沙箱策略只放行代理端口 + 子进程注入 HTTP(S)_PROXY。与 NetworkPolicy allow/deny 列表打通。

**验收**：沙箱内子进程直连外网被拒、走代理按域名策略放行；操作系统层验证（seatbelt/landlock 两平台）。

**已完成（2026-07-11）**：分三步实现，macOS + `OMIGA_SANDBOX_NETWORK_PROXY=on` opt-in，默认关闭时零行为变化。
- N3a（d74de09）：本地回环策略代理核心（CONNECT 隧道 + 明文转发，NetworkPolicy 四态 + 域名/通配/端口规则，热更新/优雅关闭）。安全 review 抓到并修复裸 `*` 策略翻转 bug（`DENY=*` 曾被反转为放行全部）。
- N3b-1（00c3b26）：进程级代理单例生命周期（`ensure_proxy_for_policy`，仅域名过滤模式按需启停+热同步）。
- N3b-2（d5b00b9）：seatbelt 只放行 loopback 代理端口 + bash 子进程注入 HTTP(S)_PROXY，域名过滤真正强制。安全 review 结论 PASS：`(deny default)` 兜底，直连外网 IP/inbound/bind 无绕过路径；启动失败 fail 回旧 seatbelt 行为（非放宽外网）。

**已知限制/后续项**：
- exec_session（G4 长驻 bash 会话）从 G6/G12 起就不走沙箱，也不走本代理——一次性 bash 工具外的执行面未覆盖。若要闭合，需把 sandbox+proxy 组装延伸到 exec_session/process.rs 的 spawn。
- Linux landlock（AllowList/DenyList 网络维度未实现）与 Windows（无沙箱）无 OS 层强制；按用户决策本轮仅 macOS。
- 代理单例假设策略来自恒定的进程 env；运行期改 `OMIGA_SANDBOX_NETWORK*` 会有并发策略漂移（见 proxy.rs 注释）。

## N4 远端环境预检与预热（P2）

**现状**：可用性探测 `scope != local` 直接 notRun；预热 plan 只为 local 构建。
SSH/Modal/Daytona 用户首跑仍会撞环境缺失/长构建。

**做法**：探测脚本（CONDA_MANAGER_PROBE_SCRIPT 等）本就是 POSIX shell，经 execution 后端在远端执行即可；
缓存记录的 `scope` 字段已预留（区分 local/远端目标标识）。预热在远端执行面复用同一 PrewarmTask 脚本
（bootstrap 已内置于脚本，天然可远端）。注意远端并发/超时策略与本地隔离。

**验收**：SSH 环境下 refresh_environment_availability 产出 scope=ssh 的真实记录；
远端预热后首跑命中 env_hash 缓存不再构建。

## N5 conda lockfile 可复现性（P2）

**现状**：conda.yaml 是范围声明，跨机/跨时间解析漂移，"同一 runtime 结果确定"不完全成立。

**做法**：插件侧支持 conda-lock 文件（存在则优先用）；ExecutionRecord 记录实际解析出的环境内容 hash
（`micromamba env export` 摘要）；文档给插件作者生成 lock 的指引。

**验收**：带 lock 的环境两次创建内容 hash 一致；ExecutionRecord 可见环境指纹。

## N6 冗余清理（P2）

按盘点顺序，每项先确认无存量调用再删：

1. `operator__{id}` 动态工具兼容路径（OPERATOR_TOOL_PREFIX）——文档已declare非主发现面。
2. Template `migrationTarget` 委托——迁移完成的模板改为原生执行后删机制。
3. `docs/operator-user-validation` 与 `-clean` 双胞胎分支、过时 codex/* claude/* 分支清理。
4. browser_operator（Python）与 computer_use 双轨自动化面的收敛方案盘点（先出对比文档再动手）。
5. agent 状态子系统盘点：blackboard/team_state/ralph_state/autopilot_state/orchestration/research_system
   与 playbook↔skill↔template 边界——产出去重方案，不在本项直接动刀。

**验收**：每删一项全量测试全绿 + grep 无残留引用；盘点项产出文档。

## N7 测试稳定性治理（P3）

1. `fetch::web::streaming_body_reader_enforces_running_limit` 依赖 socket bind，沙箱内必挂
   ——改为 bind 失败时显式 skip（打印原因）而非 fail，或抽象出不依赖真 socket 的 reader 测试。
2. `exec_session` 两个定时测试高负载 flake——去时序化（逻辑时钟/显式事件）或标记串行执行。
3. 2 个 `#[ignore]` 测试（Docker daemon、web search）建立按需运行的 CI job 或本地 checklist。
4. 5 处依赖调用方约定的 `unreachable!`（plugins.rs:4481、connectors.rs:2855、operators/mod.rs:4678 等）
   换成显式错误返回。

**验收**：沙箱环境全量测试 0 环境性失败；高负载 `--test-threads` 默认值下连续 5 轮 0 flake。

## N8 Windows 沙箱 + bash 静态分析（P3，视用户量）

- Windows 当前裸执行；codex 参考 `windows-sandbox-rs`。
- bash 命令执行前静态危险分析：codex 参考 `shell-command`/`execpolicy`（危险命令模式识别，
  与现有权限审批流衔接——识别为高危 → 走 G15 审批闭环）。

## N9 Linux 实机验证（P3，随 N0 合并后尽快）

G16 Landlock 与 G17 OTLP exporter 均在 macOS 上开发，无 Linux 实机验证记录。
准备 Linux 环境（容器不行，Landlock 需内核支持）跑通：沙箱拦截矩阵 + OTLP 上报链路。

## 执行纪律（沿用已验证流程）

1. 实现主力用 `codex exec --sandbox workspace-write -m gpt-5.3-codex-spark -c model_reasoning_effort=xhigh -c features.image_generation=false` 直连（勿走插件管道）；
   review 用 `--sandbox read-only`。已知事故模式与处置见项目记忆 `codex-delegation-workflow`：
   管道僵死看日志 mtime、capacity 崩溃留占位、投喂测试、oh-my-codex 孤儿进程累积（每 ~10 轮 `pkill -f "oh-my-codex/dist/mcp"`）。
2. 每阶段：任务书（scratchpad 文件，含现状锚点/硬约束/交付报告要求）→ codex 实现 →
   主会话验收（独立复跑测试 + diff 审查）→ codex review → 复核定性 → 修复 → 提交。
3. 每步 `cargo test --lib` 全绿才推进；禁止 codex commit；工作区任何时刻可编译；
   codex 沙箱内 bind 类测试失败按环境性失败甄别（主会话沙箱外复跑为准）。
4. 提交落功能分支，ff 同步工作分支；阶段全部完成后整合 review（跨阶段交互专项）再收尾。
