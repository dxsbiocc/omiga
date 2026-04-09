//! User input routing toward Claude Code–aligned semantics (incremental).
//!
//! Optional `input_target` on [`crate::commands::chat::send_message`] selects where
//! text goes. Default is the main session (“leader”).

/// Where a user message should be applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatInputTarget {
    /// Main chat session (default).
    Leader,
    /// Additional user text for a running background Agent task (`task_id` from `BackgroundAgentManager`).
    BackgroundAgentFollowup {
        task_id: String,
    },
}

impl ChatInputTarget {
    /// Parse `input_target` from the wire.
    /// - `None` / empty / `leader` / `main` → [`Leader`].
    /// - `bg:<task_id>` → follow-up for that background task.
    pub fn parse(raw: Option<&str>) -> Result<Self, &'static str> {
        let s = raw.map(str::trim).filter(|s| !s.is_empty());
        match s {
            None => Ok(ChatInputTarget::Leader),
            Some(t) if t.eq_ignore_ascii_case("leader") || t.eq_ignore_ascii_case("main") => {
                Ok(ChatInputTarget::Leader)
            }
            Some(t) if t.starts_with("bg:") => {
                let id = t[3..].trim();
                if id.is_empty() {
                    return Err("input_target bg: requires a task id");
                }
                Ok(ChatInputTarget::BackgroundAgentFollowup {
                    task_id: id.to_string(),
                })
            }
            Some(_) => Err("unknown input_target; omit, use leader, or bg:<task_id>"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_defaults() {
        assert_eq!(ChatInputTarget::parse(None).unwrap(), ChatInputTarget::Leader);
        assert_eq!(
            ChatInputTarget::parse(Some("")).unwrap(),
            ChatInputTarget::Leader
        );
        assert_eq!(
            ChatInputTarget::parse(Some("leader")).unwrap(),
            ChatInputTarget::Leader
        );
    }

    #[test]
    fn parse_bg() {
        match ChatInputTarget::parse(Some("bg:abc-123")).unwrap() {
            ChatInputTarget::BackgroundAgentFollowup { task_id } => {
                assert_eq!(task_id, "abc-123");
            }
            _ => panic!("expected bg"),
        }
    }
}
