//! 用户级 Omiga 上下文：自定义人格、`SOUL.md` / `MEMORY.md` / `USER.md`（仅 `~/.omiga/`，无项目级叠层）。
//!
//! - **`~/.omiga/config.yaml`** 中的 `agent.personalities` — 命名人格预设
//! - **`~/.omiga/BOOTSTRAP.md`** — 首次引导指令（存在时注入系统提示；Agent 完成引导后自行删除）
//! - **`~/.omiga/SOUL.md` / `MEMORY.md` / `USER.md`** — 身份、长期笔记、用户画像

use std::collections::HashMap;
use std::path::PathBuf;

use serde::Deserialize;

/// BOOTSTRAP.md 最大注入长度：引导指令应简洁，2KB 足够
const BOOTSTRAP_MAX_BYTES: usize = 2 * 1024;
/// SOUL.md 最大注入长度：Agent 身份应简洁，4KB ≈ 1000 tokens
const SOUL_MAX_BYTES: usize = 4 * 1024;
/// USER.md 最大注入长度：用户简介应精炼，8KB ≈ 2000 tokens
const USER_MAX_BYTES: usize = 8 * 1024;
/// MEMORY.md 最大注入长度：超出此限制则跳过注入以防止 token 爆炸，16KB ≈ 4000 tokens
const MEMORY_MAX_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Default)]
pub struct UserOmigaContext {
    /// `~/.omiga/config.yaml` 中 `agent.personalities`（键名为小写存储）
    pub personalities: HashMap<String, String>,
    /// `~/.omiga/BOOTSTRAP.md`：首次引导指令，存在时优先注入，Agent 完成后自行删除
    pub bootstrap_md: Option<String>,
    pub soul: Option<String>,
    pub memory_md: Option<String>,
    pub user_profile_md: Option<String>,
}

#[derive(Deserialize, Default)]
struct ConfigRoot {
    #[serde(default)]
    agent: AgentYaml,
}

#[derive(Deserialize, Default)]
struct AgentYaml {
    #[serde(default)]
    personalities: HashMap<String, String>,
}

fn user_omiga_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".omiga"))
}

fn load_personalities_yaml(path: &std::path::Path) -> HashMap<String, String> {
    if !path.exists() {
        return HashMap::new();
    }
    let Ok(text) = std::fs::read_to_string(path) else {
        return HashMap::new();
    };
    serde_yaml::from_str::<ConfigRoot>(&text)
        .map(|c| c.agent.personalities)
        .unwrap_or_default()
}

fn normalize_personality_map(m: HashMap<String, String>) -> HashMap<String, String> {
    m.into_iter()
        .map(|(k, v)| (k.trim().to_lowercase(), v.trim().to_string()))
        .filter(|(_, v)| !v.is_empty())
        .collect()
}

/// 读取并截断 UTF-8 文本文件。
///
/// - `max_bytes`：超出时截断并追加提示（SOUL/USER 截断；MEMORY 超限直接跳过）。
/// - `skip_if_over`：超出 `max_bytes` 时返回 None 而非截断（用于 MEMORY.md）。
fn read_optional_markdown(
    path: &std::path::Path,
    max_bytes: usize,
    skip_if_over: bool,
) -> Option<String> {
    if !path.is_file() {
        return None;
    }
    let data = std::fs::read(path).ok()?;
    if skip_if_over && data.len() > max_bytes {
        tracing::warn!(
            path = %path.display(),
            size = data.len(),
            limit = max_bytes,
            "Skipping injection: file exceeds size limit"
        );
        return None;
    }
    let mut text = String::from_utf8_lossy(&data).into_owned();
    if text.len() > max_bytes {
        let mut end = max_bytes;
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push_str("\n\n[Omiga: truncated — edit ~/.omiga/ file to reduce size]");
    }
    let text = text.trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// 仅从用户主目录 `~/.omiga/` 加载角色上下文与人格配置（不读取项目 `.omiga`）。
pub fn load_user_omiga_context() -> UserOmigaContext {
    let personalities = user_omiga_dir()
        .map(|udir| normalize_personality_map(load_personalities_yaml(&udir.join("config.yaml"))))
        .unwrap_or_default();

    let (bootstrap_md, soul, memory_md, user_profile_md) = if let Some(ref udir) = user_omiga_dir() {
        (
            // BOOTSTRAP.md：首次引导指令，小文件，截断即可
            read_optional_markdown(&udir.join("BOOTSTRAP.md"), BOOTSTRAP_MAX_BYTES, false),
            // SOUL.md：截断而非跳过，确保 Agent 身份始终可见
            read_optional_markdown(&udir.join("SOUL.md"), SOUL_MAX_BYTES, false),
            // MEMORY.md：超限跳过，避免 token 爆炸；用户应拆分或精简
            read_optional_markdown(&udir.join("MEMORY.md"), MEMORY_MAX_BYTES, true),
            // USER.md：截断而非跳过
            read_optional_markdown(&udir.join("USER.md"), USER_MAX_BYTES, false),
        )
    } else {
        (None, None, None, None)
    };

    UserOmigaContext {
        personalities,
        bootstrap_md,
        soul,
        memory_md,
        user_profile_md,
    }
}

impl UserOmigaContext {
    /// 注入主系统提示的片段。
    ///
    /// 注入顺序（参考 CoPaw）：
    /// 1. BOOTSTRAP.md — 首次引导指令（最高优先级，存在时放最前）
    /// 2. SOUL.md      — Agent 身份
    /// 3. USER.md      — 用户画像
    /// 4. MEMORY.md    — 长期笔记
    pub fn main_system_prompt_sections(&self) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(ref s) = self.bootstrap_md {
            out.push(format!("# BOOTSTRAP.md\n\n{}", s));
        }
        if let Some(ref s) = self.soul {
            out.push(format!("# SOUL.md\n\n{}", s));
        }
        if let Some(ref s) = self.user_profile_md {
            out.push(format!("# USER.md\n\n{}", s));
        }
        if let Some(ref s) = self.memory_md {
            out.push(format!("# MEMORY.md\n\n{}", s));
        }
        out
    }

    /// 供 Agent 人格叠层解析使用（与内置表合并查找）。
    pub fn personalities_ref(&self) -> &HashMap<String, String> {
        &self.personalities
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_personality_trims_keys_and_values() {
        let mut m = HashMap::new();
        m.insert("  Teacher  ".into(), "  hello  ".into());
        let n = normalize_personality_map(m);
        assert_eq!(n.get("teacher").map(String::as_str), Some("hello"));
    }
}
