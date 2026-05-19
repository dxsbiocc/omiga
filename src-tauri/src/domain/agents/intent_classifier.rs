//! Lightweight task intent classifier.
//!
//! Keyword-matching on the user message to suggest the most relevant
//! builtin subagent_type. No LLM call — purely deterministic.
//! The hint is injected as a trailing system-prompt section.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskIntent {
    CodeReview,
    Debugging,
    Security,
    Architecture,
    Testing,
    Research,
    DataAnalysis,
    Planning,
    Verification,
    General,
}

impl TaskIntent {
    pub fn suggested_subagent_type(&self) -> Option<&'static str> {
        match self {
            Self::CodeReview   => Some("code-reviewer"),
            Self::Debugging    => Some("debugger"),
            Self::Security     => Some("security-reviewer"),
            Self::Architecture => Some("architect"),
            Self::Testing      => Some("test-engineer"),
            Self::Research     => Some("deep-research"),
            Self::DataAnalysis => Some("data-analysis"),
            Self::Planning     => Some("plan"),
            Self::Verification => Some("verification"),
            Self::General      => None,
        }
    }
}

/// Classify the user message. Returns the first matching intent, or General.
pub fn classify(message: &str) -> TaskIntent {
    let m = message.to_ascii_lowercase();

    // Security — checked first to avoid masking by other patterns.
    // Use space-anchored or compound phrases to avoid single-word false-positives
    // (e.g. "secret project", "secret santa" should NOT route to security-reviewer).
    if contains_any(&m, &[
        "security audit", "security review", "security scan", "security check",
        "vulnerability", "vuln", "injection", "xss", "csrf",
        "secret key", "api secret", "hardcoded secret", "leaked secret",
        "credential leak", "credential exposure", "auth bypass",
        "privilege escal", "exploit", "cve-", "penetration test", "pentest",
    ]) {
        return TaskIntent::Security;
    }
    // Debugging
    if contains_any(&m, &[
        "bug", " error", "crash", "broken", "failing", "stack trace",
        "exception", "panic", "segfault", "why is", "not working",
        "doesn't work", "does not work", "debug", "traceback",
    ]) {
        return TaskIntent::Debugging;
    }
    // Code review
    if contains_any(&m, &[
        "review", "code review", "pr review", "pull request",
        " diff ", "check my code", "look at this code", "审查",
    ]) {
        return TaskIntent::CodeReview;
    }
    // Architecture
    if contains_any(&m, &[
        "architect", "system design", "scalab", "design pattern",
        "tradeoff", "trade-off", "infrastructure", "microservice",
        "data flow", "high level design",
    ]) {
        return TaskIntent::Architecture;
    }
    // Testing
    if contains_any(&m, &[
        "write test", "add test", "unit test", "integration test",
        "tdd", "test coverage", "test case", " spec ", "测试",
    ]) {
        return TaskIntent::Testing;
    }
    // Research — "survey" alone is too broad (e.g. "survey the codebase" = code review)
    if contains_any(&m, &[
        "research", "paper", "literature", "pubmed", "arxiv",
        "literature survey", "find papers", "related work", "state of the art",
        "systematic review", "meta-analysis",
    ]) {
        return TaskIntent::Research;
    }
    // Data analysis
    if contains_any(&m, &[
        "analys", "statistic", "dataset", "dataframe", "pandas",
        "numpy", "scipy", "r script", "correlation", "regression", "anova",
    ]) {
        return TaskIntent::DataAnalysis;
    }
    // Planning
    if contains_any(&m, &[
        "plan ", "planning", "design the", "how should i",
        "what's the best approach", "before implement", "规划",
    ]) {
        return TaskIntent::Planning;
    }
    // Verification
    if contains_any(&m, &[
        "verify", "validate", "check if", "does it work",
        "is it correct", "qa ", "quality assurance", "验证",
    ]) {
        return TaskIntent::Verification;
    }

    TaskIntent::General
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| haystack.contains(n))
}

/// Build a compact hint string to inject into the system prompt.
/// Returns None when intent is General (no hint — avoid noise for common tasks).
pub fn build_system_hint(intent: &TaskIntent) -> Option<String> {
    let agent = intent.suggested_subagent_type()?;
    Some(format!(
        "[Routing hint: This looks like a {intent:?} task. \
         Consider: Agent({{ subagent_type: \"{agent}\", prompt: \"...\" }}) \
         rather than handling inline — the specialist has the right tools and context.]"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn security_keywords() {
        assert_eq!(classify("check for sql injection vulnerabilities"), TaskIntent::Security);
        assert_eq!(classify("security audit of auth module"), TaskIntent::Security);
        assert_eq!(classify("scan for xss vulnerabilities"), TaskIntent::Security);
        assert_eq!(classify("there is a credential leak in the config"), TaskIntent::Security);
    }

    #[test]
    fn security_false_positive_guard() {
        // "secret" alone must NOT route to security
        assert_eq!(classify("I have a secret project idea"), TaskIntent::General);
        // "survey" alone must NOT route to research
        assert_eq!(classify("survey the codebase structure"), TaskIntent::General);
    }

    #[test]
    fn debug_keywords() {
        assert_eq!(classify("there is a bug in auth.rs"), TaskIntent::Debugging);
        assert_eq!(classify("why is this crashing on startup"), TaskIntent::Debugging);
    }

    #[test]
    fn code_review_keywords() {
        assert_eq!(classify("please review this PR"), TaskIntent::CodeReview);
        assert_eq!(classify("do a code review of main.rs"), TaskIntent::CodeReview);
    }

    #[test]
    fn testing_keywords() {
        assert_eq!(classify("write unit tests for the auth module"), TaskIntent::Testing);
    }

    #[test]
    fn research_keywords() {
        assert_eq!(classify("find papers on transformer attention"), TaskIntent::Research);
    }

    #[test]
    fn general_fallback() {
        assert_eq!(classify("add a new button to the settings page"), TaskIntent::General);
        assert_eq!(classify("hello"), TaskIntent::General);
    }

    #[test]
    fn hint_absent_for_general() {
        assert!(build_system_hint(&TaskIntent::General).is_none());
    }

    #[test]
    fn hint_contains_agent_name() {
        let h = build_system_hint(&TaskIntent::Security).unwrap();
        assert!(h.contains("security-reviewer"));
        let h2 = build_system_hint(&TaskIntent::Debugging).unwrap();
        assert!(h2.contains("debugger"));
    }
}
