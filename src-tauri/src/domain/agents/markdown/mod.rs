//! 内置角色与上下文 Markdown / 配置片段模板（**仅用户级**：`~/.omiga/`）。
//!
//! 运行时加载逻辑见 [`crate::domain::agents::user_context`]。
//!
//! 以下常量便于在应用内展示「默认模板」或生成脚手架（复制到 `~/.omiga/` 即可）。

/// `SOUL.md` 模板全文（身份与语气）。
pub const TEMPLATE_SOUL_MD: &str = include_str!("SOUL.md");

/// `MEMORY.md` 模板全文（长期手写笔记）。
pub const TEMPLATE_MEMORY_MD: &str = include_str!("MEMORY.md");

/// `USER.md` 模板全文（用户画像）。
pub const TEMPLATE_USER_MD: &str = include_str!("USER.md");

/// `BOOTSTRAP.md` 内容：首次引导指令，由 Agent 在第一次对话中执行。
pub const TEMPLATE_BOOTSTRAP_MD: &str = include_str!("BOOTSTRAP.md");

/// `agent.personalities` 配置片段示例（YAML），合并进 `~/.omiga/config.yaml`。
pub const EXAMPLE_AGENT_PERSONALITIES_YAML: &str = include_str!("agent-personalities.example.yaml");
