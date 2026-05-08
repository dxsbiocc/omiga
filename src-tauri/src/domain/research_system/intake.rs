use super::models::{ExecutionRoute, IntakeAssessment};

#[derive(Debug, Default, Clone, Copy)]
pub struct IntakeAnalyzer;

impl IntakeAnalyzer {
    pub fn new() -> Self {
        Self
    }

    pub fn analyze(&self, user_request: &str) -> IntakeAssessment {
        let lowered = user_request.to_lowercase();
        let retrieval = contains_any(
            &lowered,
            &[
                "检索",
                "文献",
                "论文",
                "search",
                "research",
                "rna-seq",
                "scrna",
                "single-cell",
            ],
        );
        let analysis = contains_any(
            &lowered,
            &["分析", "analysis", "compare", "适用场景", "interpret"],
        );
        let visualization = contains_any(
            &lowered,
            &["可视化", "visual", "plot", "chart", "figure", "图"],
        );
        let report = contains_any(&lowered, &["报告", "report", "总结", "汇报"]);
        let code = contains_any(&lowered, &["代码", "实现", "code", "implement"]);
        let debug = contains_any(&lowered, &["调试", "debug", "修复", "error", "fix"]);
        let biology = contains_any(&lowered, &["生物", "rna", "single cell", "单细胞", "seq"]);
        let method = contains_any(&lowered, &["方法", "method", "算法", "algorithm"]);
        let processing = contains_any(
            &lowered,
            &["清洗", "process", "格式", "预处理", "normalize"],
        );
        let explicit_multi_agent = contains_any(
            &lowered,
            &[
                "multi-agent",
                "multiple agents",
                "多 agent",
                "多智能体",
                "并行",
                "team",
            ],
        );

        let capability_hits = [
            retrieval,
            analysis,
            visualization,
            report,
            code,
            debug,
            biology,
            method,
            processing,
        ]
        .into_iter()
        .filter(|flag| *flag)
        .count();
        let clause_bonus = user_request
            .chars()
            .filter(|ch| matches!(ch, '，' | ',' | '；' | ';'))
            .count()
            .min(2);
        let complexity_score = capability_hits + clause_bonus;

        let execution_route = if explicit_multi_agent || complexity_score >= 5 {
            ExecutionRoute::MultiAgent
        } else if complexity_score <= 1 && user_request.chars().count() < 80 {
            ExecutionRoute::Solo
        } else {
            ExecutionRoute::Workflow
        };

        let mut assumptions = vec![
            "Intake uses deterministic heuristics in the MVP runtime.".to_string(),
            "High-risk actions remain subject to explicit permission checks.".to_string(),
        ];
        if retrieval {
            assumptions.push("The request benefits from external evidence collection.".to_string());
        }
        if analysis && !report {
            assumptions.push(
                "Analysis output should still be packaged as a user-facing synthesis.".to_string(),
            );
        }

        let mut ambiguities = Vec::new();
        if !report && !visualization && !code && !debug {
            ambiguities.push("未明确最终交付物，系统将默认输出结构化结论。".to_string());
        }
        if retrieval && !contains_any(&lowered, &["时间范围", "最新", "recent", "latest"]) {
            ambiguities.push("未指定检索时间范围，MVP 将优先使用通用方法综述视角。".to_string());
        }
        if contains_any(&lowered, &["帮我看下", "看一下", "看看"]) && capability_hits <= 1
        {
            ambiguities.push("请求措辞较泛，Intake 会采用保守解释并保留不确定性。".to_string());
        }

        IntakeAssessment {
            user_goal: user_request.to_string(),
            assumptions,
            ambiguities,
            complexity_score,
            execution_route,
        }
    }
}

fn contains_any(text: &str, keywords: &[&str]) -> bool {
    keywords.iter().any(|keyword| text.contains(keyword))
}
