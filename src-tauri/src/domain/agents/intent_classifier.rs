//! Lightweight task intent classifier.
//!
//! Scores each intent by counting keyword matches, then picks the highest-scoring
//! non-zero intent. This avoids first-match ordering bugs: a message like
//! "review the security audit" scores 2 for Security vs 1 for CodeReview
//! and correctly routes to security-reviewer.
//!
//! No LLM call — purely deterministic. The hint is injected as a trailing
//! system-prompt section.

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
    Performance,
    Refactoring,
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
            Self::Performance  => Some("performance-reviewer"),
            Self::Refactoring  => Some("refactor-cleaner"),
            Self::General      => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Keyword tables
// Each pattern is a lowercase substring to search for in the lowercased message.
// ---------------------------------------------------------------------------

/// Weight-2 patterns: strong signals. A single hit scores 2.
static SECURITY_STRONG: &[&str] = &[
    "vulnerability", "vuln", "injection", "xss", "csrf",
    "secret key", "api secret", "hardcoded secret", "leaked secret",
    "credential leak", "credential exposure", "auth bypass",
    "privilege escal", "exploit", "cve-", "penetration test", "pentest",
    "sql injection", "command injection", "path traversal", "directory traversal",
    "owasp", "rce ", "remote code execution", "安全漏洞", "注入攻击", "越权",
    // Compound security phrases score strong so they beat lone "review" or "check"
    "security audit", "security review", "security scan", "security check",
    "security vulnerab", "unsafe code", "unsafe block",
];
/// Weight-1 patterns: supporting signals (rare; most security signals are strong).
static SECURITY_WEAK: &[&str] = &[
    "权限检查", "敏感信息", "密钥泄露",
];

static DEBUGGING_STRONG: &[&str] = &[
    "stack trace", "traceback", "segfault", "null pointer", "null dereference",
    "panic at", "runtime error", "assertion failed",
    "core dump", "bus error", "undefined behavior",
];
static DEBUGGING_WEAK: &[&str] = &[
    "bug", " error", "crash", "broken", "failing", "exception",
    " panic", "why is", "not working", "doesn't work", "does not work",
    "debug", "doesn't compile", "compilation error", "linker error",
    "报错", "崩溃", "异常", "调试", "为什么报错", "不工作", "出错了",
];

static CODE_REVIEW_STRONG: &[&str] = &[
    "code review", "pr review", "pull request review", "review this pr",
    "代码审查", "code quality check",
];
static CODE_REVIEW_WEAK: &[&str] = &[
    "review", " diff ", "check my code", "look at this code",
    "审查", "review these changes", "lgtm", "feedback on my",
];

static ARCHITECTURE_STRONG: &[&str] = &[
    "system design", "high level design", "architecture design",
    "design the system", "microservice", "service mesh",
    "事件驱动", "架构设计", "系统设计",
];
static ARCHITECTURE_WEAK: &[&str] = &[
    "architect", "scalab", "design pattern", "tradeoff", "trade-off",
    "infrastructure", "data flow", "distributed", "monolith",
    "模块划分", "解耦", "高内聚",
];

static TESTING_STRONG: &[&str] = &[
    "write test", "add test", "write unit test", "add unit test",
    "write integration test", "test coverage", "tdd",
    "写测试", "补测试", "单元测试", "集成测试",
];
static TESTING_WEAK: &[&str] = &[
    "unit test", "integration test", "test case", " spec ",
    "mock", "stub", "fixture", "测试", "e2e test", "end-to-end",
];

static RESEARCH_STRONG: &[&str] = &[
    "literature survey", "find papers", "related work", "state of the art",
    "systematic review", "meta-analysis", "pubmed", "arxiv",
    "文献综述", "相关研究",
];
static RESEARCH_WEAK: &[&str] = &[
    "research", "paper", "literature", "survey paper",
    "academic", "citation", "study on", "investigate",
    "调研", "研究一下",
];

static DATA_ANALYSIS_STRONG: &[&str] = &[
    "pandas", "numpy", "scipy", "dataframe", "r script", "ggplot",
    "correlation matrix", "regression analysis", "anova", "t-test",
    "数据分析", "统计分析",
];
static DATA_ANALYSIS_WEAK: &[&str] = &[
    "analys", "statistic", "dataset", "correlation", "regression",
    "distribution", "histogram", "scatter plot", "hypothesis test",
    "数据集", "可视化", "分析数据",
];

static PLANNING_STRONG: &[&str] = &[
    "plan before", "before implement", "design first",
    "need a plan", "implementation plan", "roadmap",
    "制定计划", "实施方案",
];
static PLANNING_WEAK: &[&str] = &[
    "plan ", "planning", "how should i approach", "best approach",
    "规划", "怎么设计", "先规划",
];

static VERIFICATION_STRONG: &[&str] = &[
    "quality assurance", "verify that", "validate that",
    "does it work correctly", "regression test",
    "验证功能", "确认是否",
];
static VERIFICATION_WEAK: &[&str] = &[
    "verify", "validate", "check if", "does it work",
    "is it correct", "qa ", "sanity check",
    "验证", "检验", "核实",
];

static PERFORMANCE_STRONG: &[&str] = &[
    "n+1 query", "n+1 problem", "slow query", "query optimization",
    "memory leak", "cpu spike", "latency spike",
    "性能瓶颈", "查询优化", "内存泄漏",
];
static PERFORMANCE_WEAK: &[&str] = &[
    "slow", "performance", "profiling", "profile", "latency",
    "bottleneck", "optimize", "benchmark", "throughput", "memory usage",
    "cpu usage", "response time", "timeout",
    "慢", "性能", "优化", "卡顿",
];

static REFACTORING_STRONG: &[&str] = &[
    "refactor this", "clean up this", "extract function", "extract method",
    "rename variable", "dead code", "remove duplication",
    "重构", "代码清理",
];
static REFACTORING_WEAK: &[&str] = &[
    "refactor", "clean up", "reorganize", "restructure",
    "simplify", "dedup", "consolidate", "tidy",
    "整理代码", "简化",
];

// ---------------------------------------------------------------------------
// Scoring
// ---------------------------------------------------------------------------

fn score(m: &str, strong: &[&str], weak: &[&str]) -> usize {
    let s: usize = strong.iter().filter(|p| m.contains(**p)).count() * 2;
    let w: usize = weak.iter().filter(|p| m.contains(**p)).count();
    s + w
}

/// Classify the user message. Returns the highest-scoring intent, or General.
pub fn classify(message: &str) -> TaskIntent {
    let m = message.to_ascii_lowercase();

    let scores: [(usize, TaskIntent); 11] = [
        (score(&m, SECURITY_STRONG,     SECURITY_WEAK),     TaskIntent::Security),
        (score(&m, DEBUGGING_STRONG,    DEBUGGING_WEAK),     TaskIntent::Debugging),
        (score(&m, CODE_REVIEW_STRONG,  CODE_REVIEW_WEAK),   TaskIntent::CodeReview),
        (score(&m, ARCHITECTURE_STRONG, ARCHITECTURE_WEAK),  TaskIntent::Architecture),
        (score(&m, TESTING_STRONG,      TESTING_WEAK),       TaskIntent::Testing),
        (score(&m, RESEARCH_STRONG,     RESEARCH_WEAK),      TaskIntent::Research),
        (score(&m, DATA_ANALYSIS_STRONG,DATA_ANALYSIS_WEAK), TaskIntent::DataAnalysis),
        (score(&m, PLANNING_STRONG,     PLANNING_WEAK),      TaskIntent::Planning),
        (score(&m, VERIFICATION_STRONG, VERIFICATION_WEAK),  TaskIntent::Verification),
        (score(&m, PERFORMANCE_STRONG,  PERFORMANCE_WEAK),   TaskIntent::Performance),
        (score(&m, REFACTORING_STRONG,  REFACTORING_WEAK),   TaskIntent::Refactoring),
    ];

    scores
        .into_iter()
        .filter(|(s, _)| *s > 0)
        .max_by_key(|(s, _)| *s)
        .map(|(_, intent)| intent)
        .unwrap_or(TaskIntent::General)
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
    fn security_beats_code_review_on_overlap() {
        // "review the security audit" scores 2 for Security, 1 for CodeReview
        assert_eq!(classify("review the security audit of auth"), TaskIntent::Security);
        assert_eq!(classify("do a security review"), TaskIntent::Security);
    }

    #[test]
    fn security_false_positive_guard() {
        assert_eq!(classify("I have a secret project idea"), TaskIntent::General);
        assert_eq!(classify("survey the codebase structure"), TaskIntent::General);
        assert_eq!(classify("secure your future"), TaskIntent::General);
    }

    #[test]
    fn debug_keywords() {
        assert_eq!(classify("there is a bug in auth.rs"), TaskIntent::Debugging);
        assert_eq!(classify("why is this crashing on startup"), TaskIntent::Debugging);
        assert_eq!(classify("stack trace: thread main panicked"), TaskIntent::Debugging);
    }

    #[test]
    fn code_review_keywords() {
        assert_eq!(classify("please review this PR"), TaskIntent::CodeReview);
        assert_eq!(classify("do a code review of main.rs"), TaskIntent::CodeReview);
    }

    #[test]
    fn architecture_keywords() {
        assert_eq!(classify("design the system architecture for a microservice"), TaskIntent::Architecture);
        assert_eq!(classify("what are the tradeoffs of this design pattern"), TaskIntent::Architecture);
    }

    #[test]
    fn testing_keywords() {
        assert_eq!(classify("write unit tests for the auth module"), TaskIntent::Testing);
        assert_eq!(classify("add integration tests for the api"), TaskIntent::Testing);
    }

    #[test]
    fn research_keywords() {
        assert_eq!(classify("find papers on transformer attention"), TaskIntent::Research);
        assert_eq!(classify("literature survey on diffusion models"), TaskIntent::Research);
    }

    #[test]
    fn data_analysis_keywords() {
        assert_eq!(classify("analyze this dataset with pandas"), TaskIntent::DataAnalysis);
        assert_eq!(classify("run a correlation analysis"), TaskIntent::DataAnalysis);
    }

    #[test]
    fn performance_keywords() {
        assert_eq!(classify("this page is really slow, help me optimize"), TaskIntent::Performance);
        assert_eq!(classify("there's a memory leak in the server"), TaskIntent::Performance);
        assert_eq!(classify("n+1 query in the ORM"), TaskIntent::Performance);
    }

    #[test]
    fn refactoring_keywords() {
        assert_eq!(classify("refactor this module to reduce duplication"), TaskIntent::Refactoring);
        assert_eq!(classify("clean up this code and extract functions"), TaskIntent::Refactoring);
    }

    #[test]
    fn chinese_keywords() {
        assert_eq!(classify("帮我进行代码审查"), TaskIntent::CodeReview);
        assert_eq!(classify("这里有个崩溃异常"), TaskIntent::Debugging);
        assert_eq!(classify("写单元测试"), TaskIntent::Testing);
        assert_eq!(classify("性能优化，页面太卡了"), TaskIntent::Performance);
        assert_eq!(classify("重构这段代码"), TaskIntent::Refactoring);
        assert_eq!(classify("数据分析和统计"), TaskIntent::DataAnalysis);
        assert_eq!(classify("安全漏洞检测"), TaskIntent::Security);
    }

    #[test]
    fn planning_keywords() {
        assert_eq!(classify("plan before we implement the new feature"), TaskIntent::Planning);
    }

    #[test]
    fn verification_keywords() {
        assert_eq!(classify("verify that the auth flow works correctly"), TaskIntent::Verification);
    }

    #[test]
    fn general_fallback() {
        assert_eq!(classify("add a new button to the settings page"), TaskIntent::General);
        assert_eq!(classify("hello"), TaskIntent::General);
        assert_eq!(classify("what does this function do"), TaskIntent::General);
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
        let h3 = build_system_hint(&TaskIntent::Performance).unwrap();
        assert!(h3.contains("performance-reviewer"));
        let h4 = build_system_hint(&TaskIntent::Refactoring).unwrap();
        assert!(h4.contains("refactor-cleaner"));
    }
}
