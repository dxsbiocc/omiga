use super::models::{AgentCard, AgentResult, AssembledContext, TaskGraph, TaskSpec};
use super::stores::{ArtifactStore, EvidenceStore};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Default, Clone, Copy)]
pub struct ContextAssembler;

impl ContextAssembler {
    pub fn new() -> Self {
        Self
    }

    pub fn assemble(
        &self,
        graph: &TaskGraph,
        task: &TaskSpec,
        agent: &AgentCard,
        results: &BTreeMap<String, AgentResult>,
        evidence_store: &dyn EvidenceStore,
        artifact_store: &dyn ArtifactStore,
    ) -> AssembledContext {
        let dependency_results = task
            .dependencies
            .iter()
            .filter_map(|dep| results.get(dep))
            .cloned()
            .collect::<Vec<_>>();

        let evidence_refs = collect_evidence_refs(task, &dependency_results, evidence_store);
        let artifact_refs = collect_artifact_refs(task, &dependency_results, artifact_store);

        let mut candidates = BTreeMap::new();
        candidates.insert(
            "global_context".to_string(),
            json!({
                "user_goal": graph.user_goal,
                "assumptions": graph.assumptions,
                "ambiguities": graph.ambiguities,
                "execution_route": graph.execution_route,
                "global_constraints": graph.global_constraints,
            }),
        );
        candidates.insert("user_goal".to_string(), json!(graph.user_goal));
        candidates.insert("assumptions".to_string(), json!(graph.assumptions));
        candidates.insert(
            "global_constraints".to_string(),
            json!(graph.global_constraints),
        );
        candidates.insert("task_spec".to_string(), json!(task));
        candidates.insert("task_context".to_string(), json!(task));
        candidates.insert(
            "upstream_results_summary".to_string(),
            json!(summarize_results(&dependency_results)),
        );
        candidates.insert(
            "prior_evidence_summary".to_string(),
            json!(summarize_evidence(&evidence_refs, evidence_store)),
        );
        candidates.insert("evidence_refs".to_string(), json!(evidence_refs));
        candidates.insert("artifact_refs".to_string(), json!(artifact_refs));
        candidates.insert("agent_instructions".to_string(), json!(agent.instructions));

        let include = build_include_set(&agent.context_policy.include);
        let exclude = build_include_set(&agent.context_policy.exclude);
        let baseline = baseline_sections();
        let mut sections = BTreeMap::new();
        let mut omitted_sections = Vec::new();

        for (key, value) in candidates {
            let should_include = if include.is_empty() {
                baseline.contains(&key)
            } else {
                include.contains(&key)
                    || key == "task_spec"
                    || key == "agent_instructions"
                    || key == "global_context"
            };
            if should_include && !exclude.contains(&key) {
                sections.insert(
                    key.clone(),
                    truncate_value(value, agent.context_policy.max_input_tokens),
                );
            } else {
                omitted_sections.push(key);
            }
        }

        let token_estimate = estimate_tokens(&sections);

        AssembledContext {
            agent_id: agent.id.clone(),
            task_id: task.task_id.clone(),
            sections,
            omitted_sections,
            token_estimate,
        }
    }
}

fn build_include_set(values: &[String]) -> BTreeSet<String> {
    values.iter().cloned().collect()
}

fn baseline_sections() -> BTreeSet<String> {
    [
        "user_goal",
        "assumptions",
        "global_constraints",
        "global_context",
        "task_spec",
        "task_context",
        "upstream_results_summary",
        "prior_evidence_summary",
        "evidence_refs",
        "artifact_refs",
        "agent_instructions",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn collect_evidence_refs(
    task: &TaskSpec,
    dependency_results: &[AgentResult],
    evidence_store: &dyn EvidenceStore,
) -> Vec<String> {
    let mut refs = task
        .input_refs
        .iter()
        .filter(|item| evidence_store.get(item).is_some())
        .cloned()
        .collect::<Vec<_>>();
    for result in dependency_results {
        refs.extend(result.evidence_refs.clone());
    }
    refs.sort();
    refs.dedup();
    refs
}

fn collect_artifact_refs(
    task: &TaskSpec,
    dependency_results: &[AgentResult],
    artifact_store: &dyn ArtifactStore,
) -> Vec<String> {
    let mut refs = task
        .input_refs
        .iter()
        .filter(|item| artifact_store.get(item).is_some())
        .cloned()
        .collect::<Vec<_>>();
    for result in dependency_results {
        refs.extend(result.artifact_refs.clone());
    }
    refs.sort();
    refs.dedup();
    refs
}

fn summarize_results(results: &[AgentResult]) -> Vec<Value> {
    results
        .iter()
        .map(|result| {
            json!({
                "task_id": result.task_id,
                "agent_id": result.agent_id,
                "status": result.status,
                "summary": summarize_output(&result.output),
                "issues": result.issues,
            })
        })
        .collect()
}

fn summarize_evidence(ids: &[String], store: &dyn EvidenceStore) -> Vec<Value> {
    ids.iter()
        .filter_map(|id| store.get(id))
        .map(|evidence| {
            json!({
                "id": evidence.id,
                "summary": evidence.summary,
                "source": evidence.source,
                "quality": evidence.quality,
            })
        })
        .collect()
}

fn summarize_output(value: &Value) -> Value {
    match value {
        Value::String(text) => Value::String(truncate_string(text, 240)),
        Value::Array(items) => json!(items.iter().take(3).cloned().collect::<Vec<_>>()),
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (index, (key, value)) in map.iter().enumerate() {
                if index >= 5 {
                    break;
                }
                out.insert(key.clone(), value.clone());
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

fn truncate_value(value: Value, max_input_tokens: usize) -> Value {
    let max_chars = max_input_tokens.saturating_mul(4);
    truncate_value_to_chars(value, max_chars)
}

fn truncate_value_to_chars(value: Value, max_chars: usize) -> Value {
    if max_chars == 0 || value.to_string().chars().count() <= max_chars {
        return value;
    }

    match value {
        Value::String(text) => Value::String(truncate_string(&text, max_chars)),
        Value::Array(items) => truncate_array(items, max_chars),
        Value::Object(map) => truncate_object(map, max_chars),
        other => other,
    }
}

fn truncate_array(items: Vec<Value>, max_chars: usize) -> Value {
    let mut kept = Vec::new();
    for item in items {
        let current_len = Value::Array(kept.clone()).to_string().chars().count();
        if current_len >= max_chars {
            break;
        }
        let remaining = max_chars.saturating_sub(current_len + 2);
        if remaining == 0 {
            break;
        }
        let truncated = truncate_value_to_chars(item, remaining);
        kept.push(truncated);
        if Value::Array(kept.clone()).to_string().chars().count() > max_chars {
            kept.pop();
            break;
        }
    }
    Value::Array(kept)
}

fn truncate_object(map: serde_json::Map<String, Value>, max_chars: usize) -> Value {
    let mut kept = serde_json::Map::new();
    for (key, value) in map {
        let current_len = Value::Object(kept.clone()).to_string().chars().count();
        if current_len >= max_chars {
            break;
        }
        let reserved_for_key = key.chars().count() + 8;
        let remaining = max_chars.saturating_sub(current_len + reserved_for_key);
        if remaining == 0 {
            break;
        }
        let truncated = truncate_value_to_chars(value, remaining);
        let inserted_key = key.clone();
        kept.insert(key, truncated);
        if Value::Object(kept.clone()).to_string().chars().count() > max_chars {
            kept.remove(&inserted_key);
            break;
        }
    }
    Value::Object(kept)
}

fn truncate_string(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    text.chars().take(max_chars).collect::<String>()
}

fn estimate_tokens(sections: &BTreeMap<String, Value>) -> usize {
    sections
        .values()
        .map(|value| value.to_string().chars().count() / 4 + 1)
        .sum()
}
