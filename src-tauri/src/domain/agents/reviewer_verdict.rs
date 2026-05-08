//! Structured reviewer verdict extraction.
//!
//! Reviewer agents mostly emit free-form text today. This module extracts a
//! lightweight structured verdict so blocking semantics and synthesis can rely
//! on more than ad-hoc string checks.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewerSeverity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewerVerdictKind {
    Pass,
    Partial,
    Fail,
    Reject,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewerVerdict {
    pub agent_type: String,
    pub severity: ReviewerSeverity,
    pub verdict: ReviewerVerdictKind,
    pub summary: String,
    pub recommendation: Option<String>,
}

fn highest_severity(text: &str) -> ReviewerSeverity {
    let upper = text.to_ascii_uppercase();
    if upper.contains("CRITICAL") {
        ReviewerSeverity::Critical
    } else if upper.contains("HIGH") {
        ReviewerSeverity::High
    } else if upper.contains("MEDIUM") {
        ReviewerSeverity::Medium
    } else if upper.contains("LOW") {
        ReviewerSeverity::Low
    } else {
        ReviewerSeverity::Info
    }
}

fn verdict_kind(text: &str) -> ReviewerVerdictKind {
    let upper = text.to_ascii_uppercase();
    if upper.contains("VERDICT: REJECTED") || upper.contains("REJECTED") {
        ReviewerVerdictKind::Reject
    } else if upper.contains("VERDICT: FAIL") || upper.contains("FAIL") || upper.contains("BLOCKER")
    {
        ReviewerVerdictKind::Fail
    } else if upper.contains("VERDICT: PARTIAL") || upper.contains("PARTIAL") {
        ReviewerVerdictKind::Partial
    } else if upper.contains("VERDICT: PASS")
        || upper.contains("APPROVED")
        || upper.contains("PASS")
    {
        ReviewerVerdictKind::Pass
    } else {
        ReviewerVerdictKind::Unknown
    }
}

fn first_meaningful_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#') && !line.starts_with('-'))
        .unwrap_or("")
        .chars()
        .take(240)
        .collect()
}

fn recommendation_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|line| {
            let upper = line.to_ascii_uppercase();
            upper.starts_with("NEXT STEP")
                || upper.starts_with("RECOMMENDED")
                || upper.starts_with("RECOMMENDATION")
                || upper.starts_with("SUGGESTED FIX")
        })
        .map(|line| line.chars().take(240).collect())
}

pub fn parse_reviewer_verdict(agent_type: &str, text: &str) -> ReviewerVerdict {
    ReviewerVerdict {
        agent_type: agent_type.to_string(),
        severity: highest_severity(text),
        verdict: verdict_kind(text),
        summary: first_meaningful_line(text),
        recommendation: recommendation_line(text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fail_with_critical() {
        let v = parse_reviewer_verdict(
            "security-reviewer",
            "VERDICT: FAIL\nCRITICAL: secret exposed in config\nRecommendation: rotate keys",
        );
        assert_eq!(v.verdict, ReviewerVerdictKind::Fail);
        assert_eq!(v.severity, ReviewerSeverity::Critical);
        assert!(v.recommendation.is_some());
    }

    #[test]
    fn parses_partial_with_summary() {
        let v = parse_reviewer_verdict(
            "quality-reviewer",
            "# Review\nVERDICT: PARTIAL\nNeeds cleaner boundaries",
        );
        assert_eq!(v.verdict, ReviewerVerdictKind::Partial);
        assert_eq!(v.summary, "VERDICT: PARTIAL");
    }
}
