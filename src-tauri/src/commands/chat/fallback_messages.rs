use super::*;

/// Max assistant↔tool iterations per user send (safety valve; raised to support
/// longer evidence-first investigation and multi-step execution in the main agent).
pub(super) const MAX_TOOL_ROUNDS: usize = 100;

pub(super) fn tool_round_limit_message(max_rounds: usize) -> String {
    format!(
        "\n\n### 已达到工具调用上限（{max_rounds} 轮）\n\n\
这不是用户取消，也不是会话崩溃；系统已主动停止继续调用工具，避免在同一类错误里无限循环。\n\n\
**当前状态**\n\
- 上方工具记录已经保留，可以继续作为上下文使用。\n\
- 本轮没有稳定收敛，通常需要先缩小问题、改用更简单的单步命令，或由用户确认关键约束。\n\n\
**建议下一步**\n\
1. 先让我整理：已完成内容、失败点、可疑根因和下一步方案。\n\
2. 如果继续执行，请让我按“小步验证 → 再写入”的方式继续，避免重复同一失败路径。\n\
3. 如果你已经知道正确约束，可以直接补充约束后让我继续。\n"
    )
}

pub(super) fn tool_round_limit_follow_ups() -> Vec<FollowUpSuggestion> {
    vec![
        FollowUpSuggestion {
            label: "先总结问题".to_string(),
            prompt: "先不要继续调用工具。请基于上方记录，总结：已完成内容、失败点、根因假设、还缺哪些信息，以及建议我下一步怎么做。".to_string(),
        },
        FollowUpSuggestion {
            label: "小步继续".to_string(),
            prompt: "继续执行，但请先给出一个最小可验证步骤；每次只运行一个简单命令，成功后再进入下一步，避免重复刚才失败的工具调用方式。".to_string(),
        },
        FollowUpSuggestion {
            label: "让我补充约束".to_string(),
            prompt: "请列出你继续前必须由我确认的 3 个以内关键问题，不要继续自动调用工具。".to_string(),
        },
    ]
}

fn truncate_tool_error_for_fallback(output: &str) -> String {
    truncate_tool_result_for_fallback(output, true)
}

fn truncate_tool_result_for_fallback(output: &str, is_error: bool) -> String {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return if is_error {
            "工具返回了错误，但没有提供可展示的错误文本。".to_string()
        } else {
            "工具执行完成，但没有返回可展示文本。".to_string()
        };
    }
    let prefix = truncate_utf8_prefix(trimmed, 240).trim();
    if prefix.len() < trimmed.len() {
        format!("{prefix}…")
    } else {
        prefix.to_string()
    }
}

pub(super) fn tool_no_final_answer_message(tool_results: &[(String, String, bool)]) -> String {
    let has_errors = tool_results.iter().any(|(_, _, is_error)| *is_error);
    if has_errors {
        return tool_failure_no_final_answer_message(tool_results);
    }

    let snippets: Vec<String> = tool_results
        .iter()
        .rev()
        .take(5)
        .map(|(name, output, is_error)| {
            let summary = truncate_tool_result_for_fallback(output, *is_error).replace('`', "'");
            format!("`{}`：{}", name.replace('`', "'"), summary)
        })
        .collect();

    let mut message = String::from(
        "### 工具已执行，但没有生成最终回复\n\n\
上方工具调用已经返回结果，但模型没有继续输出总结。为避免界面显示“已完成”却没有交代，我已停止本轮并保留工具记录。\n\n",
    );

    if !snippets.is_empty() {
        message.push_str("**最近工具结果**\n");
        for (idx, snippet) in snippets.iter().enumerate() {
            message.push_str(&format!("{}. {snippet}\n", idx + 1));
        }
        message.push('\n');
    }

    message.push_str(
        "**建议下一步**\n\
1. 展开上方最后几个工具，确认写入文件、输出和错误状态是否符合预期。\n\
2. 让我基于当前工具结果继续整理最终答案，不要重新从头执行。\n\
3. 如果要继续自动执行，请先按“一个最小命令 → 验证 → 再下一步”的方式收敛。\n",
    );
    message
}

fn tool_failure_no_final_answer_message(tool_results: &[(String, String, bool)]) -> String {
    let snippets: Vec<String> = tool_results
        .iter()
        .filter(|(_, _, is_error)| *is_error)
        .take(3)
        .map(|(_, output, _)| truncate_tool_error_for_fallback(output))
        .collect();

    let mut message = String::from(
        "### 本轮没有稳定完成\n\n\
最近的工具调用返回了错误，而且模型没有生成可交付的最终回复。系统已停止继续自动尝试，避免在同一失败路径里反复调用工具或误写记忆。\n\n",
    );

    if !snippets.is_empty() {
        message.push_str("**最近错误摘要**\n");
        for (idx, snippet) in snippets.iter().enumerate() {
            message.push_str(&format!("{}. `{}`\n", idx + 1, snippet.replace('`', "'")));
        }
        message.push('\n');
    }

    message.push_str(
        "**建议下一步**\n\
1. 展开上方错误工具，确认具体 stderr / 参数。\n\
2. 让我按“单个最小命令 → 验证 → 再写入”的方式继续。\n\
3. 如果当前环境或路径有特殊约束，先补充约束再继续。\n",
    );
    message
}

pub(super) fn should_update_memory_after_turn(final_reply: &str, had_tool_errors: bool) -> bool {
    let trimmed = final_reply.trim();
    if trimmed.is_empty() {
        return false;
    }
    if had_tool_errors {
        return false;
    }
    let lower = trimmed.to_lowercase();
    ![
        "已达到工具调用上限",
        "本轮没有稳定完成",
        "没有生成可交付的最终回复",
        "工具已执行，但没有生成最终回复",
        "模型没有继续输出总结",
        "exceeded maximum tool rounds",
        "autopilot stopped",
        "[cancelled",
        "cancelled by user",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_final_after_tool_error_gets_visible_failure_message() {
        let results = vec![
            (
                "tool-1".to_string(),
                "Error: command failed because destination path does not exist".to_string(),
                true,
            ),
            ("tool-2".to_string(), "ok".to_string(), false),
        ];

        let message = tool_failure_no_final_answer_message(&results);

        assert!(message.contains("本轮没有稳定完成"));
        assert!(message.contains("最近错误摘要"));
        assert!(message.contains("destination path does not exist"));
        assert!(!should_update_memory_after_turn(&message, true));
    }

    #[test]
    fn empty_final_after_successful_tools_gets_visible_summary_message() {
        let results = vec![
            (
                "file_write".to_string(),
                "File created (1251 bytes)".to_string(),
                false,
            ),
            (
                "bash".to_string(),
                "Created /tmp/run_slurm.sh".to_string(),
                false,
            ),
        ];

        let message = tool_no_final_answer_message(&results);

        assert!(message.contains("工具已执行，但没有生成最终回复"));
        assert!(message.contains("最近工具结果"));
        assert!(message.contains("File created"));
        assert!(message.contains("Created /tmp/run_slurm.sh"));
        assert!(message.contains("建议下一步"));
        assert!(!should_update_memory_after_turn(&message, false));
    }

    #[test]
    fn safety_stop_replies_do_not_update_memory() {
        assert!(!should_update_memory_after_turn("", false));
        assert!(!should_update_memory_after_turn(
            &tool_round_limit_message(100),
            false
        ));
        assert!(!should_update_memory_after_turn(
            "Autopilot stopped after exceeding max argumentation cycles (3/3).",
            false,
        ));
        assert!(!should_update_memory_after_turn(
            "任务完成：中间有一次命令失败但后来恢复。",
            true,
        ));
        assert!(!should_update_memory_after_turn(
            "### 工具已执行，但没有生成最终回复\n\n模型没有继续输出总结。",
            false,
        ));
        assert!(should_update_memory_after_turn(
            "任务完成：已写入文件并通过测试。",
            false
        ));
    }
}
