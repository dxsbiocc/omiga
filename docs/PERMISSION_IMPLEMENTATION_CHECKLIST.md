# 权限系统实施检查清单

> 逐步实施指南，确保不遗漏关键功能

---

## Phase 1: 基础架构 (Day 1-2)

### 1.1 创建目录结构

```
src-tauri/src/domain/permissions/
├── mod.rs              # 模块导出
├── types.rs            # 核心类型定义
├── manager.rs          # PermissionManager
├── engine.rs           # 规则匹配引擎
├── risk.rs             # 风险评估
├── patterns.rs         # 危险模式数据库
├── presets.rs          # 预设配置
├── audit.rs            # 审计日志
└── tests.rs            # 单元测试
```

**检查点**:
- [ ] 目录创建完成
- [ ] mod.rs 正确导出所有模块
- [ ] Cargo.toml 添加必要依赖

### 1.2 核心类型定义 (types.rs)

```rust
必须实现的类型:
□ PermissionMode (6种模式)
□ PermissionRule (完整字段)
□ PermissionContext
□ PermissionDecision
□ RiskLevel (5个等级)
□ RiskAssessment
```

**检查点**:
- [ ] 所有类型实现 Serialize/Deserialize
- [ ] 类型可以正确序列化为 JSON
- [ ] 前端 TypeScript 类型已定义

### 1.3 PermissionManager 基础框架 (manager.rs)

```rust
必须实现的方法:
□ new() - 构造函数
□ check_permission() - 核心检查方法
□ add_rule() - 添加规则
□ delete_rule() - 删除规则
□ list_rules() - 列出规则
```

**检查点**:
- [ ] 可以编译通过
- [ ] 基础单元测试通过

---

## Phase 2: 风险评估 (Day 3-4)

### 2.1 危险模式数据库 (patterns.rs)

```rust
必须实现的检测模式:
□ rm -rf /
□ Fork bomb
□ > /dev/sda
□ mkfs 格式化
□ chmod -R 777 /
□ curl | sh
□ sudo 提权
□ 系统关键文件修改
□ 敏感凭证文件
```

**检查点**:
- [ ] 每个模式有对应的单元测试
- [ ] 误报率 < 5%
- [ ] 漏报率 = 0%

### 2.2 风险评估器 (risk.rs)

```rust
必须实现的功能:
□ 工具级别风险评估
□ 参数级别风险评估
□ 路径级别风险评估
□ 综合风险计算
□ 风险描述生成
□ 缓解建议生成
```

**检查点**:
- [ ] 风险等级计算正确
- [ ] 危险命令被正确识别
- [ ] 误报率低

### 2.3 规则匹配引擎 (engine.rs)

```rust
必须实现的匹配器:
□ ToolMatcher::Exact
□ ToolMatcher::Wildcard
□ ToolMatcher::Regex
□ PathMatcher::Exact
□ PathMatcher::Prefix
□ PathMatcher::Glob
□ PathMatcher::Regex
```

**检查点**:
- [ ] 通配符匹配正确 (*, ?)
- [ ] Glob 模式匹配正确 (**/*)
- [ ] 正则匹配正确

---

## Phase 3: 持久化与状态 (Day 5-6)

### 3.1 SQLite 表设计

```sql
必须创建的表:
□ permission_rules
□ permission_approvals (会话级)
□ permission_denials (拒绝历史)
□ audit_log (审计日志)
```

**检查点**:
- [ ] 表结构正确
- [ ] 索引优化
- [ ] 迁移脚本

### 3.2 状态管理

```rust
必须实现的状态:
□ 会话级批准缓存
□ 时间窗口批准缓存
□ 规则使用统计
□ 最近拒绝历史
```

**检查点**:
- [ ] 会话隔离正确
- [ ] 过期清理机制
- [ ] 内存使用合理

---

## Phase 4: 前端 UI (Day 7-10)

### 4.1 权限确认对话框

```tsx
必须实现的功能:
□ 风险等级显示 (颜色+图标)
□ 工具信息展示
□ 参数格式化显示
□ 风险详情展开
□ 模式选择下拉框
□ 危险操作二次确认
□ 键盘快捷键支持
```

**检查点**:
- [ ] UI 美观易用
- [ ] 响应式设计
- [ ] 动画流畅

### 4.2 规则管理面板

```tsx
必须实现的功能:
□ 规则列表展示
□ 添加/编辑/删除规则
□ 规则排序 (优先级)
□ 预设配置应用
□ 规则测试 (模拟匹配)
```

**检查点**:
- [ ] 操作反馈明确
- [ ] 表单验证完整

### 4.3 拒绝历史面板

```tsx
必须实现的功能:
□ 拒绝记录列表
□ 一键添加规则
□ 批量清除
□ 导出日志
```

---

## Phase 5: Tauri 集成 (Day 11-12)

### 5.1 命令注册

```rust
必须实现的命令:
□ permission_check
□ permission_approve
□ permission_deny
□ permission_list_rules
□ permission_add_rule
□ permission_delete_rule
□ permission_apply_preset
□ permission_get_recent_denials
```

**检查点**:
- [ ] 所有命令已注册
- [ ] 错误处理完善
- [ ] 前端可以正常调用

### 5.2 AppState 集成

```rust
必须添加的字段:
□ permission_manager: Arc<PermissionManager>
```

**检查点**:
- [ ] 生命周期管理正确
- [ ] 多线程安全

---

## Phase 6: 测试与优化 (Day 13-15)

### 6.1 单元测试

```rust
必须测试的场景:
□ 精确规则匹配
□ 通配符规则匹配
□ 正则规则匹配
□ 路径匹配
□ 参数条件匹配
□ 风险等级计算
□ 危险命令检测
□ 会话级批准
□ 时间窗口批准
□ 规则优先级
```

**检查点**:
- [ ] 覆盖率 > 80%
- [ ] 所有测试通过

### 6.2 集成测试

```
必须测试的场景:
□ 完整权限检查流程
□ 对话框交互
□ 规则 CRUD
□ 预设应用
□ 会话隔离
```

### 6.3 性能测试

```
性能指标:
□ 权限检查延迟 < 10ms
□ 规则匹配 < 5ms (100条规则)
□ 内存占用 < 50MB
```

---

## 快速启动模板

### 1. 创建基础文件

```bash
cd omiga/src-tauri/src/domain
mkdir -p permissions
touch permissions/{mod,types,manager,engine,risk,patterns,presets,audit,tests}.rs
```

### 2. 添加依赖

```toml
# Cargo.toml
[dependencies]
regex = "1.10"
glob = "0.3"
chrono = { version = "0.4", features = ["serde"] }
```

### 3. 基础代码模板

```rust
// permissions/mod.rs
pub mod types;
pub mod manager;
pub mod engine;
pub mod risk;
pub mod patterns;
pub mod presets;
pub mod audit;

pub use types::*;
pub use manager::PermissionManager;
```

### 4. 注册到 lib.rs

```rust
// lib.rs
pub mod domain;

// domain/mod.rs
pub mod permissions;
```

### 5. 前端状态

```bash
cd omiga/src/state
touch permissionStore.ts
```

---

## 常见问题排查

### 问题1: 规则匹配失败

```
症状: 规则不生效
排查:
1. 检查规则优先级 (数字越小优先级越高)
2. 检查通配符语法 (* 匹配任意字符, ** 匹配路径)
3. 检查正则表达式语法
4. 查看日志确认匹配过程
```

### 问题2: 危险命令漏报

```
症状: 危险命令没有被拦截
排查:
1. 检查 DangerousPatternDB 是否加载
2. 检查正则表达式是否正确
3. 检查命令解析逻辑
4. 添加单元测试复现问题
```

### 问题3: 会话隔离失败

```
症状: 会话A的批准影响到会话B
排查:
1. 检查 session_id 是否正确传递
2. 检查 session_approvals HashMap 键值
3. 检查并发访问安全
```

---

## 验收标准

### 功能验收

- [ ] 6 种权限模式正常工作
- [ ] 危险命令 100% 被检测
- [ ] 规则匹配准确率 > 95%
- [ ] 会话隔离正确
- [ ] 审计日志完整

### 性能验收

- [ ] 权限检查 < 10ms
- [ ] 内存占用 < 50MB
- [ ] UI 响应流畅

### 用户体验验收

- [ ] 对话框清晰易懂
- [ ] 风险信息明确
- [ ] 操作反馈及时
- [ ] 学习成本低

---

*最后更新: 2026-04-07*
