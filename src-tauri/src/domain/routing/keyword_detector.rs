//! Keyword Detector — route user messages to research workflow skills/modes
//!
//! Detects trigger keywords in the user message and returns the skill to route to.
//! Tailored for the AI personal research assistant use case.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillRoute {
    pub skill_name: String,
    pub args: String,
    pub priority: u8,
}

struct KeywordRule {
    keywords: &'static [&'static str],
    skill: &'static str,
    priority: u8,
    word_boundary: bool,
    /// If any of these strings appear within 25 bytes before a keyword match, the match is
    /// skipped. Used to prevent negation phrases ("don't stop ralph") from triggering cancel.
    negation_guards: &'static [&'static str],
}

static RULES: &[KeywordRule] = &[
    // Cancel — explicit cancellation only; "stop" alone is excluded because
    // it conflicts with Ralph's "don't stop" / "dont stop" trigger phrases.
    // negation_guards ensures "don't stop ralph" / "不要停止" do NOT route here.
    KeywordRule {
        keywords: &[
            "cancel",
            "abort",
            "stop ralph",
            "stop team",
            "cancel ralph",
            "cancel team",
            "停止",
            "取消",
            "中止",
        ],
        skill: "cancel",
        priority: 11,
        word_boundary: true,
        negation_guards: &["don't", "dont", "not ", "never ", "不要", "别", "请不要"],
    },
    // Explicit autopilot resume should beat Ralph's generic "resume" trigger.
    KeywordRule {
        keywords: &[
            "resume autopilot",
            "continue autopilot",
            "继续自动执行",
            "恢复自动执行",
            "从上次自动执行继续",
            "继续 autopilot",
        ],
        skill: "autopilot",
        priority: 10,
        word_boundary: false,
        negation_guards: &[],
    },
    // Pipeline monitoring — highest priority
    KeywordRule {
        keywords: &[
            "run pipeline",
            "start pipeline",
            "run workflow",
            "start workflow",
            "run snakemake",
            "run nextflow",
            "跑流水线",
            "运行流水线",
            "运行工作流",
            "启动流水线",
            "ralph",
            "don't stop",
            "dont stop",
            "keep going",
            "持续执行",
            "不要停",
            // Chinese negation of "stop" — explicitly route here to beat cancel
            "不要停止",
            "别停止",
            "请不要停止",
            // Resume triggers — load existing ralph state and continue
            "resume",
            "continue analysis",
            "pick up where",
            "继续分析",
            "恢复执行",
            "继续上次",
            "从上次继续",
        ],
        skill: "ralph",
        priority: 10,
        word_boundary: true,
        negation_guards: &[],
    },
    // Parallel analysis — team mode
    KeywordRule {
        keywords: &[
            "parallel analysis",
            "run in parallel",
            "team mode",
            "并行分析",
            "并行执行",
            "团队模式",
        ],
        skill: "team",
        priority: 9,
        word_boundary: false,
        negation_guards: &[],
    },
    // Full autonomous pipeline
    KeywordRule {
        keywords: &[
            "autopilot",
            "autonomous",
            "build me",
            "create me",
            "make me",
            "full auto",
            "自动执行",
            "全自动",
        ],
        skill: "autopilot",
        priority: 8,
        word_boundary: false,
        negation_guards: &[],
    },
    // Literature search / research analysis
    KeywordRule {
        keywords: &[
            "search literature",
            "search papers",
            "find papers",
            "pubmed",
            "arxiv",
            "biorxiv",
            "literature review",
            "文献检索",
            "检索文献",
            "查文献",
            "搜索论文",
            "找论文",
            // Research status / review queries
            "research review",
            "state of the art",
            "survey of",
            "review of",
            "research status",
            "research landscape",
            "field overview",
            "研究现状",
            "研究进展",
            "领域综述",
            "综述",
            "研究综述",
            "领域分析",
            "领域研究",
            "研究领域",
            "最新进展",
            "进展综述",
            "分析领域",
            "领域现状",
            "现状分析",
            "研究动态",
        ],
        skill: "literature-search",
        priority: 8,
        word_boundary: false,
        negation_guards: &[],
    },
    // Data analysis + result interpretation
    KeywordRule {
        keywords: &[
            "interpret results",
            "analyze results",
            "result interpretation",
            "解读结果",
            "结果解读",
            "分析结果",
            "解释结果",
        ],
        skill: "interpret-results",
        priority: 8,
        word_boundary: false,
        negation_guards: &[],
    },
    // Manuscript / writing
    KeywordRule {
        keywords: &[
            "write paper",
            "write manuscript",
            "draft results",
            "draft discussion",
            "写论文",
            "撰写论文",
            "写结果",
            "写讨论",
            "论文草稿",
        ],
        skill: "write-paper",
        priority: 7,
        word_boundary: false,
        negation_guards: &[],
    },
    // Visualization
    KeywordRule {
        keywords: &[
            "make plot",
            "create plot",
            "generate figure",
            "visualize",
            "画图",
            "生成图表",
            "可视化",
            "出图",
            "绘图",
        ],
        skill: "visualize",
        priority: 7,
        word_boundary: false,
        negation_guards: &[],
    },
    // Planning / analysis design
    KeywordRule {
        keywords: &[
            "plan this",
            "plan the analysis",
            "design analysis",
            "规划",
            "制定计划",
            "分析方案",
            "设计分析",
        ],
        skill: "plan",
        priority: 7,
        word_boundary: false,
        negation_guards: &[],
    },
    // Build error resolution
    KeywordRule {
        keywords: &[
            "fix build",
            "build error",
            "build failed",
            "build fail",
            "compile error",
            "compilation error",
            "type error",
            "type check",
            "编译错误",
            "构建失败",
            "编译失败",
            "类型错误",
        ],
        skill: "build-fix",
        priority: 7,
        word_boundary: false,
        negation_guards: &[],
    },
    // QA cycling
    KeywordRule {
        keywords: &[
            "ultraqa",
            "qa cycle",
            "qa pass",
            "qa sweep",
            "测试巡检",
            "质量巡检",
        ],
        skill: "ultraqa",
        priority: 7,
        word_boundary: false,
        negation_guards: &[],
    },
    // Code review
    KeywordRule {
        keywords: &[
            "code review",
            "review code",
            "review my code",
            "review this",
            "review the diff",
            "review pr",
            "审查代码",
            "代码审查",
            "看一下代码",
        ],
        skill: "code-review",
        priority: 6,
        word_boundary: false,
        negation_guards: &[],
    },
    // Test-Driven Development
    KeywordRule {
        keywords: &[
            "write tests first",
            "test driven",
            "tdd",
            "test-driven",
            "write failing test",
            "red green refactor",
            "测试驱动",
            "先写测试",
            "写测试",
        ],
        skill: "tdd",
        priority: 6,
        word_boundary: true,
        negation_guards: &[],
    },
    // Requirements clarification
    KeywordRule {
        keywords: &["deep interview", "clarify requirements", "需求澄清"],
        skill: "deep-interview",
        priority: 6,
        word_boundary: false,
        negation_guards: &[],
    },
];

pub fn detect_skill_route(message: &str) -> Option<SkillRoute> {
    let lower = message.to_lowercase();
    let mut best: Option<(u8, &KeywordRule)> = None;

    for rule in RULES {
        let matches = if rule.word_boundary {
            rule.keywords.iter().any(|kw| {
                let kw_lower = kw.to_lowercase();
                let mut start = 0;
                while let Some(idx) = lower[start..].find(&kw_lower) {
                    let abs_idx = start + idx;
                    // Use byte-slice + chars() so multibyte characters are handled
                    // correctly — find() returns byte offsets, not char indices.
                    // Word-boundary only blocks ASCII word characters; CJK and other
                    // non-ASCII chars do not form run-on words so they never block.
                    let is_ascii_word = |c: char| c.is_ascii() && c.is_alphanumeric();
                    let before_ok = abs_idx == 0
                        || !lower[..abs_idx]
                            .chars()
                            .last()
                            .map(is_ascii_word)
                            .unwrap_or(false);
                    let end_idx = abs_idx + kw_lower.len();
                    let after_ok = end_idx >= lower.len()
                        || !lower[end_idx..]
                            .chars()
                            .next()
                            .map(is_ascii_word)
                            .unwrap_or(false);
                    if before_ok && after_ok {
                        // Check negation guards — scan up to 25 bytes before this match.
                        // window_start must land on a char boundary; saturating_sub(25) may
                        // not, so floor to the nearest char boundary by re-finding the start.
                        let raw_window = abs_idx.saturating_sub(25);
                        let window_start = lower[..abs_idx]
                            .char_indices()
                            .find(|(i, _)| *i >= raw_window)
                            .map(|(i, _)| i)
                            .unwrap_or(0);
                        let prefix = &lower[window_start..abs_idx];
                        let negated = rule.negation_guards.iter().any(|ng| prefix.contains(*ng));
                        if !negated {
                            return true;
                        }
                    }
                    // Advance past this occurrence. Using kw_lower.len() (not +1) keeps us on
                    // char boundaries for multi-byte CJK keywords.
                    start = abs_idx + kw_lower.len();
                    if start >= lower.len() {
                        break;
                    }
                }
                false
            })
        } else {
            rule.keywords
                .iter()
                .any(|kw| lower.contains(&kw.to_lowercase()))
        };

        if matches {
            if best.map(|(p, _)| rule.priority > p).unwrap_or(true) {
                best = Some((rule.priority, rule));
            }
        }
    }

    best.map(|(_, rule)| SkillRoute {
        skill_name: rule.skill.to_string(),
        args: extract_args(message, rule.keywords),
        priority: rule.priority,
    })
}

fn extract_args(message: &str, keywords: &[&str]) -> String {
    let lower = message.to_lowercase();
    for kw in keywords {
        let kw_lower = kw.to_lowercase();
        if let Some(idx) = lower.find(&kw_lower) {
            // Use kw_lower.len() — byte length of the lowercased form, matching the idx offset.
            let after = message[idx + kw_lower.len()..].trim();
            if !after.is_empty() {
                return after.to_string();
            }
        }
    }
    message.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_pipeline() {
        let r = detect_skill_route("run snakemake workflow for RNA-seq analysis").unwrap();
        assert_eq!(r.skill_name, "ralph");
    }

    #[test]
    fn detects_literature_search() {
        let r = detect_skill_route("search papers on single cell RNA-seq clustering").unwrap();
        assert_eq!(r.skill_name, "literature-search");
    }

    #[test]
    fn detects_chinese_literature() {
        let r = detect_skill_route("帮我检索文献，关于CRISPR基因编辑").unwrap();
        assert_eq!(r.skill_name, "literature-search");
    }

    #[test]
    fn detects_interpret_results() {
        let r = detect_skill_route("解读结果，DESeq2差异分析完成了").unwrap();
        assert_eq!(r.skill_name, "interpret-results");
    }

    #[test]
    fn detects_visualization() {
        let r = detect_skill_route("画图：火山图和热图").unwrap();
        assert_eq!(r.skill_name, "visualize");
    }

    #[test]
    fn detects_manuscript() {
        let r = detect_skill_route("写论文结果章节").unwrap();
        assert_eq!(r.skill_name, "write-paper");
    }

    #[test]
    fn detects_parallel_analysis() {
        let r = detect_skill_route("并行分析这三组样本").unwrap();
        assert_eq!(r.skill_name, "team");
    }

    #[test]
    fn detects_autopilot() {
        let r = detect_skill_route("autopilot build me a verified feature").unwrap();
        assert_eq!(r.skill_name, "autopilot");
    }

    #[test]
    fn detects_autopilot_chinese() {
        let r = detect_skill_route("请全自动完成这个需求").unwrap();
        assert_eq!(r.skill_name, "autopilot");
    }

    #[test]
    fn detects_autopilot_resume_over_ralph_resume() {
        let r = detect_skill_route("resume autopilot from the last QA cycle").unwrap();
        assert_eq!(r.skill_name, "autopilot");
    }

    #[test]
    fn detects_research_status_chinese() {
        let r = detect_skill_route("帮我分析一下CRISPR基因编辑领域的研究现状").unwrap();
        assert_eq!(r.skill_name, "literature-search");
    }

    #[test]
    fn detects_research_survey_chinese() {
        let r = detect_skill_route("写一篇关于单细胞测序的综述").unwrap();
        assert_eq!(r.skill_name, "literature-search");
    }

    #[test]
    fn detects_state_of_the_art() {
        let r = detect_skill_route("what is the state of the art in protein structure prediction?")
            .unwrap();
        assert_eq!(r.skill_name, "literature-search");
    }

    #[test]
    fn detects_research_landscape() {
        let r =
            detect_skill_route("give me an overview of the research landscape in LLM alignment")
                .unwrap();
        assert_eq!(r.skill_name, "literature-search");
    }

    #[test]
    fn detects_latest_progress_chinese() {
        let r = detect_skill_route("分析一下深度学习领域的最新进展").unwrap();
        assert_eq!(r.skill_name, "literature-search");
    }

    #[test]
    fn no_match_returns_none() {
        let r = detect_skill_route("hello, how are you?");
        assert!(r.is_none());
    }

    #[test]
    fn pipeline_higher_priority_than_plan() {
        let r = detect_skill_route("运行流水线并规划后续分析").unwrap();
        assert_eq!(r.skill_name, "ralph");
    }

    #[test]
    fn detects_cancel_english() {
        let r = detect_skill_route("cancel").unwrap();
        assert_eq!(r.skill_name, "cancel");
    }

    #[test]
    fn detects_abort() {
        let r = detect_skill_route("abort the task").unwrap();
        assert_eq!(r.skill_name, "cancel");
    }

    #[test]
    fn detects_stop_ralph() {
        let r = detect_skill_route("stop ralph").unwrap();
        assert_eq!(r.skill_name, "cancel");
    }

    #[test]
    fn detects_cancel_chinese() {
        let r = detect_skill_route("停止当前任务").unwrap();
        assert_eq!(r.skill_name, "cancel");
    }

    #[test]
    fn cancel_beats_ralph() {
        // "cancel ralph" should route to cancel, not ralph
        let r = detect_skill_route("cancel ralph run").unwrap();
        assert_eq!(r.skill_name, "cancel");
    }

    // Bug 1 regression: "don't stop" must NOT route to cancel
    #[test]
    fn dont_stop_routes_to_ralph_not_cancel() {
        let r = detect_skill_route("don't stop until it finishes").unwrap();
        assert_eq!(r.skill_name, "ralph");
    }

    #[test]
    fn dont_stop_variant_routes_to_ralph() {
        let r = detect_skill_route("dont stop the analysis").unwrap();
        assert_eq!(r.skill_name, "ralph");
    }

    // Bug 3 regression: mixed-language boundary detection
    #[test]
    fn chinese_prefix_before_english_cancel_keyword() {
        // "请 cancel" — multibyte char before ASCII keyword
        let r = detect_skill_route("请 cancel this").unwrap();
        assert_eq!(r.skill_name, "cancel");
    }

    #[test]
    fn chinese_prefix_before_ralph_keyword() {
        // Ensure "请ralph" does NOT match (no boundary between 请 and ralph)
        // But "请 ralph" (with space) should match
        let r = detect_skill_route("请 ralph 执行").unwrap();
        assert_eq!(r.skill_name, "ralph");
    }

    #[test]
    fn detects_build_fix_english() {
        let r = detect_skill_route("fix build errors in the project").unwrap();
        assert_eq!(r.skill_name, "build-fix");
    }

    #[test]
    fn detects_build_fix_chinese() {
        let r = detect_skill_route("有编译错误，帮我修复").unwrap();
        assert_eq!(r.skill_name, "build-fix");
    }

    #[test]
    fn detects_ultraqa() {
        let r = detect_skill_route("run an ultraqa pass on this project").unwrap();
        assert_eq!(r.skill_name, "ultraqa");
    }

    #[test]
    fn detects_code_review() {
        let r = detect_skill_route("code review this PR").unwrap();
        assert_eq!(r.skill_name, "code-review");
    }

    #[test]
    fn detects_code_review_chinese() {
        let r = detect_skill_route("帮我做代码审查").unwrap();
        assert_eq!(r.skill_name, "code-review");
    }

    #[test]
    fn detects_tdd() {
        let r = detect_skill_route("let's do tdd for this feature").unwrap();
        assert_eq!(r.skill_name, "tdd");
    }

    #[test]
    fn detects_tdd_chinese() {
        let r = detect_skill_route("测试驱动开发，先写测试").unwrap();
        assert_eq!(r.skill_name, "tdd");
    }

    #[test]
    fn build_fix_beats_plan() {
        // build-fix (p7) should beat plan (p7) only if both match — ensure no regression
        let r = detect_skill_route("build error in the analysis plan").unwrap();
        assert_eq!(r.skill_name, "build-fix");
    }

    // Regression: "don't stop ralph" must NOT route to cancel via "stop ralph" substring match
    #[test]
    fn dont_stop_ralph_routes_to_ralph_not_cancel() {
        let r = detect_skill_route("don't stop ralph please").unwrap();
        assert_eq!(r.skill_name, "ralph");
    }

    #[test]
    fn dont_stop_team_routes_to_ralph_not_cancel() {
        // "dont stop team" — "stop team" is a cancel keyword, but negation guard blocks it
        // ralph then wins on "dont stop"
        let r = detect_skill_route("dont stop team mode").unwrap();
        assert_eq!(r.skill_name, "ralph");
    }

    // Regression: Chinese negation "不要停止" must NOT route to cancel via "停止" match
    #[test]
    fn chinese_negation_not_stop_routes_to_ralph() {
        let r = detect_skill_route("不要停止流水线").unwrap();
        assert_eq!(r.skill_name, "ralph");
    }

    #[test]
    fn chinese_bie_stop_routes_to_ralph() {
        let r = detect_skill_route("别停止，继续执行").unwrap();
        assert_eq!(r.skill_name, "ralph");
    }

    // Positive: bare "停止" (no negation) still routes to cancel
    #[test]
    fn bare_chinese_stop_routes_to_cancel() {
        let r = detect_skill_route("停止任务").unwrap();
        assert_eq!(r.skill_name, "cancel");
    }
}
