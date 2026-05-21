//! 工作区安全守卫：判断操作是否限制在项目根目录内，以实现免弹窗自动放行。
//! 这是 `PermissionManager` 的子模块，可访问父模块的私有字段。

use super::super::types::*;

impl super::PermissionManager {
    /// 对路径执行尽力而为的规范化：向上寻找最长已存在前缀，再拼接剩余部分。
    ///
    /// 解决两个场景：
    /// 1. 新建文件路径在磁盘上还不存在，`canonicalize` 失败
    /// 2. macOS `/var` → `/private/var` 符号链接导致 `starts_with` 误判
    pub(super) fn canonicalize_best_effort(path: &std::path::Path) -> std::path::PathBuf {
        // 快路径：路径本身可以直接规范化
        if let Ok(canon) = std::fs::canonicalize(path) {
            return canon;
        }

        // 逐级向上寻找可规范化的祖先目录。
        // 注意：先把当前节点的文件名入栈，再尝试规范化父级，
        // 否则直接父级可以规范化时当前文件名会被漏掉。
        let mut components: Vec<std::ffi::OsString> = Vec::new();
        let mut current = path.to_path_buf();
        loop {
            // 先把当前文件名压栈
            if let Some(name) = current.file_name() {
                components.push(name.to_os_string());
            }
            let parent = match current.parent() {
                Some(p) if p != current => p.to_path_buf(),
                _ => break,
            };
            if let Ok(canon_parent) = std::fs::canonicalize(&parent) {
                // 找到了可规范化的祖先，将剩余部分（倒序）附加回去
                let mut result = canon_parent;
                for component in components.iter().rev() {
                    result.push(component);
                }
                return result;
            }
            current = parent;
        }

        // 完全无法规范化：原样返回
        path.to_path_buf()
    }

    /// 判断当前操作是否「工作区安全」——满足所有条件时可无弹窗自动放行：
    ///
    /// 1. `project_root` 已配置（用户在输入框设置了工作路径）
    /// 2. 操作不是破坏性删除（工具名含 `delete`/`remove`，或 bash 中含 DataLoss 风险）
    /// 3. bash/shell 无 High/Critical 危险模式（rm -rf、fork bomb 等）
    /// 4. 所有涉及路径均在 `project_root` 之内（或 bash 命令未显式引用任何绝对路径）
    pub(super) fn is_workspace_safe(&self, context: &PermissionContext) -> bool {
        // 条件 1：必须有配置好的项目根目录
        let Some(ref root) = context.project_root else {
            return false;
        };

        let tool = context.tool_name.as_str();

        // 条件 2：破坏性工具名直接排除
        if tool.contains("delete") || tool.contains("remove") || tool == "file_delete" {
            return false;
        }

        // 条件 3：bash/shell 危险模式检测
        if tool == "bash" || tool == "shell" {
            if let Some(cmd) = context.arguments.get("command").and_then(|v| v.as_str()) {
                let pattern_risks = self.patterns.check(cmd);
                // 任何 High/Critical 模式 → 不放行
                if pattern_risks
                    .iter()
                    .any(|r| r.severity >= RiskLevel::High)
                {
                    return false;
                }
                // DataLoss 类（rm -rf 是 Medium DataLoss）→ 不放行
                if pattern_risks
                    .iter()
                    .any(|r| r.category == RiskCategory::DataLoss)
                {
                    return false;
                }
            }
        }

        // 条件 4：路径范围检查 + 排除路径检查
        let canonical_root = std::fs::canonicalize(root).unwrap_or_else(|_| root.clone());
        let exclusions = self
            .workspace_exclusions
            .read()
            .map(|g| g.clone())
            .unwrap_or_default();

        if let Some(ref paths) = context.file_paths {
            if !paths.is_empty() {
                for path in paths {
                    let abs_path = if path.is_absolute() {
                        path.clone()
                    } else {
                        root.join(path)
                    };
                    // 文件可能还不存在（写新文件场景）——向上逐级找到最长已存在前缀再拼接。
                    // 这同时解决了 macOS /var → /private/var 符号链接问题。
                    let canonical_path = Self::canonicalize_best_effort(&abs_path);
                    if !canonical_path.starts_with(&canonical_root) {
                        return false;
                    }
                    // 排除路径检查：如果路径命中排除规则，则需要手动确认
                    if !exclusions.is_empty() {
                        // 取路径相对于 project_root 的部分
                        let rel = canonical_path
                            .strip_prefix(&canonical_root)
                            .unwrap_or(&canonical_path);
                        let rel_str = rel.to_string_lossy();
                        if exclusions.iter().any(|ex| {
                            let ex = ex.trim_start_matches('/');
                            rel_str.starts_with(ex) || rel_str == ex
                        }) {
                            return false;
                        }
                    }
                }
            }
        }

        true
    }
}
