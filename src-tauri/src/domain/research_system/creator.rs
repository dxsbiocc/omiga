use super::models::{
    AgentCard, AgentPatchAction, AgentPatchProposal, AgentRegistryPatch, ApprovalStatus,
    RegistryPatchAction, RegistryPatchMode, RegistryPatchStep, TraceKind, TraceRecord,
};
use super::registry::AgentRegistry;
use super::stores::{AgentRegistryStore, ProposalStore, TraceStore};
use std::collections::HashMap;

#[derive(Debug, Default, Clone, Copy)]
pub struct Creator;

impl Creator {
    pub fn new() -> Self {
        Self
    }

    pub fn review_traces(
        &self,
        registry: &AgentRegistry,
        trace_store: &dyn TraceStore,
        proposal_store: &mut dyn ProposalStore,
    ) -> Result<Vec<AgentPatchProposal>, String> {
        let proposals = self.analyze_traces(registry, &trace_store.list());
        for proposal in &proposals {
            proposal_store.save(proposal.clone())?;
        }
        Ok(proposals)
    }

    pub fn analyze_traces(
        &self,
        registry: &AgentRegistry,
        traces: &[TraceRecord],
    ) -> Vec<AgentPatchProposal> {
        let mut proposals = Vec::new();
        let mut failing_agents: HashMap<String, usize> = HashMap::new();
        let mut repeating_goals: HashMap<String, usize> = HashMap::new();

        for trace in traces {
            if matches!(
                trace.kind,
                TraceKind::TaskReviewed | TraceKind::TaskCompleted
            ) && matches!(
                trace.status,
                super::models::ResultStatus::Failed | super::models::ResultStatus::NeedsRevision
            ) {
                if let Some(agent_id) = &trace.agent_id {
                    *failing_agents.entry(agent_id.clone()).or_insert(0) += 1;
                }
            }
            if matches!(trace.kind, TraceKind::TaskQueued) {
                if let Some(goal) = trace.detail.get("goal").and_then(|goal| goal.as_str()) {
                    *repeating_goals.entry(normalize_goal(goal)).or_insert(0) += 1;
                }
            }
        }

        for (agent_id, count) in failing_agents {
            if count < 2 {
                continue;
            }
            let action = if registry
                .get(&agent_id)
                .map(|card| card.capabilities.len() > 2)
                .unwrap_or(false)
            {
                AgentPatchAction::Split
            } else {
                AgentPatchAction::Retire
            };
            proposals.push(AgentPatchProposal {
                proposal_id: format!("proposal-{}", uuid::Uuid::new_v4()),
                action,
                candidate_agent: None,
                target_agents: vec![agent_id.clone()],
                reason: format!(
                    "Agent '{}' accumulated {} failing or revise outcomes in the trace history.",
                    agent_id, count
                ),
                expected_benefit: "Reduce repeated failure loops by tightening the capability boundary.".to_string(),
                required_tools: Vec::new(),
                eval_plan: vec![
                    "Replay the failing trace class against the proposed agent split.".to_string(),
                    "Check that revise loops drop below 2 for the same task family.".to_string(),
                ],
                rollback_plan: vec![
                    "Restore the previous registry entry if the new split does not improve outcomes.".to_string(),
                ],
                approval_status: ApprovalStatus::Pending,
                registry_patch: None,
            });
        }

        for (goal, count) in repeating_goals {
            if count < 3 {
                continue;
            }
            let candidate = build_candidate_agent(&goal);
            proposals.push(AgentPatchProposal {
                proposal_id: format!("proposal-{}", uuid::Uuid::new_v4()),
                action: AgentPatchAction::Create,
                candidate_agent: Some(candidate),
                target_agents: Vec::new(),
                reason: format!(
                    "Observed at least {} repeated tasks matching goal pattern '{}'.",
                    count, goal
                ),
                expected_benefit: "Introduce a dedicated agent for a recurring task family."
                    .to_string(),
                required_tools: vec!["file_search".to_string()],
                eval_plan: vec![
                    "Route the repeated goal family to the candidate agent in staging.".to_string(),
                    "Compare revise/fail rate before and after the candidate agent is introduced."
                        .to_string(),
                ],
                rollback_plan: vec![
                    "Disable the candidate agent and fall back to the previous routing rule."
                        .to_string(),
                ],
                approval_status: ApprovalStatus::Pending,
                registry_patch: None,
            });
        }

        proposals
    }

    pub fn approve_proposal(
        &self,
        proposal_id: &str,
        proposal_store: &mut dyn ProposalStore,
        mut registry_store: Option<&mut dyn AgentRegistryStore>,
    ) -> Result<AgentPatchProposal, String> {
        let mut proposal = proposal_store
            .get(proposal_id)
            .ok_or_else(|| format!("proposal '{}' not found", proposal_id))?;

        proposal.approval_status = ApprovalStatus::Approved;
        let registry_snapshot = if let Some(store) = registry_store.as_deref_mut() {
            Some(store.load()?)
        } else {
            None
        };
        proposal.registry_patch = Some(build_registry_patch(&proposal, registry_snapshot.as_ref()));

        if let Some(store) = registry_store {
            match proposal.action {
                AgentPatchAction::Create => {
                    if let Some(candidate) = &proposal.candidate_agent {
                        store.save_card(candidate)?;
                        proposal.approval_status = ApprovalStatus::Applied;
                    }
                }
                AgentPatchAction::Retire => {
                    for target in &proposal.target_agents {
                        store.disable_agent(target)?;
                    }
                    proposal.approval_status = ApprovalStatus::Applied;
                }
                AgentPatchAction::Split | AgentPatchAction::Merge => {}
            }
        }

        proposal_store.update(proposal.clone())?;
        Ok(proposal)
    }
}

fn build_registry_patch(
    proposal: &AgentPatchProposal,
    registry: Option<&AgentRegistry>,
) -> AgentRegistryPatch {
    match proposal.action {
        AgentPatchAction::Create => {
            let draft_cards = proposal
                .candidate_agent
                .clone()
                .into_iter()
                .collect::<Vec<_>>();
            AgentRegistryPatch {
                mode: RegistryPatchMode::Applied,
                summary: "Create the approved candidate agent card.".to_string(),
                steps: vec![RegistryPatchStep {
                    action: RegistryPatchAction::CreateCard,
                    target_agent: proposal
                        .candidate_agent
                        .as_ref()
                        .map(|card| card.id.clone()),
                    summary: "Write the approved candidate agent card into the registry."
                        .to_string(),
                }],
                draft_cards,
            }
        }
        AgentPatchAction::Retire => AgentRegistryPatch {
            mode: RegistryPatchMode::Applied,
            summary: "Disable the target agents in the registry.".to_string(),
            steps: proposal
                .target_agents
                .iter()
                .map(|target| RegistryPatchStep {
                    action: RegistryPatchAction::DisableAgent,
                    target_agent: Some(target.clone()),
                    summary: format!(
                        "Disable '{}' after validating the retirement rationale.",
                        target
                    ),
                })
                .collect(),
            draft_cards: Vec::new(),
        },
        AgentPatchAction::Split => {
            let draft_cards = registry
                .and_then(|registry| {
                    proposal
                        .target_agents
                        .first()
                        .and_then(|target| registry.get(target))
                })
                .map(build_split_draft_cards)
                .unwrap_or_default();
            let target = proposal
                .target_agents
                .first()
                .cloned()
                .unwrap_or_else(|| "unknown-agent".to_string());
            AgentRegistryPatch {
                mode: RegistryPatchMode::Manual,
                summary: format!(
                    "Create draft split cards for '{}' and review routing before disabling the source agent.",
                    target
                ),
                steps: build_split_steps(&target, &draft_cards),
                draft_cards,
            }
        }
        AgentPatchAction::Merge => {
            let draft_cards = registry
                .map(|registry| build_merge_draft_cards(&proposal.target_agents, registry))
                .unwrap_or_default();
            AgentRegistryPatch {
                mode: RegistryPatchMode::Manual,
                summary:
                    "Create a merged draft card, validate routing, then retire the source agents."
                        .to_string(),
                steps: build_merge_steps(&proposal.target_agents, &draft_cards),
                draft_cards,
            }
        }
    }
}

fn build_split_steps(target: &str, draft_cards: &[AgentCard]) -> Vec<RegistryPatchStep> {
    let mut steps = draft_cards
        .iter()
        .map(|card| RegistryPatchStep {
            action: RegistryPatchAction::CreateCard,
            target_agent: Some(card.id.clone()),
            summary: format!(
                "Create draft split card '{}' with narrowed capabilities.",
                card.id
            ),
        })
        .collect::<Vec<_>>();
    steps.push(RegistryPatchStep {
        action: RegistryPatchAction::UpdateRouting,
        target_agent: Some(target.to_string()),
        summary: format!(
            "Update routing and handoff rules so tasks previously assigned to '{}' are distributed across the split cards.",
            target
        ),
    });
    steps.push(RegistryPatchStep {
        action: RegistryPatchAction::ManualReview,
        target_agent: Some(target.to_string()),
        summary: "Run staged evals and only disable the source agent after the split cards pass."
            .to_string(),
    });
    steps
}

fn build_merge_steps(
    target_agents: &[String],
    draft_cards: &[AgentCard],
) -> Vec<RegistryPatchStep> {
    let mut steps = draft_cards
        .iter()
        .map(|card| RegistryPatchStep {
            action: RegistryPatchAction::MergeCards,
            target_agent: Some(card.id.clone()),
            summary: format!(
                "Create merged draft card '{}' that combines the source agent responsibilities.",
                card.id
            ),
        })
        .collect::<Vec<_>>();
    steps.push(RegistryPatchStep {
        action: RegistryPatchAction::UpdateRouting,
        target_agent: None,
        summary: format!(
            "Redirect routing from [{}] to the merged draft card during staged validation.",
            target_agents.join(", ")
        ),
    });
    for target in target_agents {
        steps.push(RegistryPatchStep {
            action: RegistryPatchAction::DisableAgent,
            target_agent: Some(target.clone()),
            summary: format!(
                "Retire '{}' only after the merged draft card completes staged evals.",
                target
            ),
        });
    }
    steps
}

fn build_split_draft_cards(card: &AgentCard) -> Vec<AgentCard> {
    let capability_groups = split_list(&card.capabilities);
    if capability_groups.len() < 2 {
        return Vec::new();
    }

    capability_groups
        .into_iter()
        .enumerate()
        .map(|(index, capabilities)| {
            let suffix = capability_suffix(&capabilities, index + 1);
            let mut draft = card.clone();
            draft.id = format!("{}.{}", card.id, suffix);
            draft.name = format!("{} ({})", card.name, title_case(&suffix));
            draft.version = "0.1.0".to_string();
            draft.description = format!(
                "Draft split of '{}' focusing on {}.",
                card.id,
                capabilities.join(", ")
            );
            draft.capabilities = capabilities;
            draft.use_when = vec![format!(
                "Use when work maps to the '{}' split lane.",
                suffix
            )];
            draft.avoid_when = vec![format!(
                "Avoid when the request still needs the full '{}' capability surface.",
                card.id
            )];
            draft.failure_modes = vec![format!(
                "Split draft '{}' may still overlap too much with sibling lanes.",
                draft.id
            )];
            draft.instructions = format!(
                "{}\n\nThis is a draft split candidate generated from '{}'.",
                card.instructions, card.id
            );
            draft
        })
        .collect()
}

fn build_merge_draft_cards(target_agents: &[String], registry: &AgentRegistry) -> Vec<AgentCard> {
    let cards = target_agents
        .iter()
        .filter_map(|target| registry.get(target).cloned())
        .collect::<Vec<_>>();
    if cards.len() < 2 {
        return Vec::new();
    }

    let first = &cards[0];
    let mut merged = first.clone();
    merged.id = format!("candidate.merge.{}", target_agents.join("_"));
    merged.name = format!(
        "Merged {}",
        cards
            .iter()
            .map(|card| card.name.clone())
            .collect::<Vec<_>>()
            .join(" + ")
    );
    merged.version = "0.1.0".to_string();
    merged.description = format!(
        "Draft merged agent generated from [{}].",
        target_agents.join(", ")
    );
    merged.capabilities = dedup_strings(
        cards
            .iter()
            .flat_map(|card| card.capabilities.clone())
            .collect(),
    );
    merged.use_when = vec![format!(
        "Use when tasks span the combined surface of [{}].",
        target_agents.join(", ")
    )];
    merged.avoid_when =
        vec!["Avoid if the merge broadens context too much for a single agent.".to_string()];
    merged.handoff_targets = dedup_strings(
        cards
            .iter()
            .flat_map(|card| card.handoff_targets.clone())
            .collect(),
    );
    merged.failure_modes = vec![format!(
        "Merged draft '{}' may reintroduce broad-context regressions.",
        merged.id
    )];
    merged.instructions = format!(
        "{}\n\nThis is a draft merged candidate generated from [{}].",
        merged.instructions,
        target_agents.join(", ")
    );
    vec![merged]
}

fn split_list(values: &[String]) -> Vec<Vec<String>> {
    if values.is_empty() {
        return Vec::new();
    }
    if values.len() == 1 {
        return vec![vec![values[0].clone()], vec![values[0].clone()]];
    }
    let midpoint = values.len().div_ceil(2);
    vec![values[..midpoint].to_vec(), values[midpoint..].to_vec()]
}

fn capability_suffix(capabilities: &[String], fallback_index: usize) -> String {
    capabilities
        .first()
        .map(|capability| normalize_goal(capability))
        .filter(|slug| !slug.is_empty())
        .unwrap_or_else(|| format!("lane-{}", fallback_index))
}

fn title_case(text: &str) -> String {
    text.split('-')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn dedup_strings(values: Vec<String>) -> Vec<String> {
    let mut unique = Vec::new();
    for value in values {
        if !unique.contains(&value) {
            unique.push(value);
        }
    }
    unique
}

fn normalize_goal(goal: &str) -> String {
    goal.to_lowercase()
        .chars()
        .filter(|ch| ch.is_alphanumeric() || ch.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .take(6)
        .collect::<Vec<_>>()
        .join("-")
}

fn build_candidate_agent(goal: &str) -> AgentCard {
    let slug = if goal.is_empty() {
        "candidate-specialist".to_string()
    } else {
        format!("candidate-{}", goal)
    };
    AgentCard {
        id: format!("candidate.{}", slug),
        name: "Candidate Specialist".to_string(),
        version: "0.1.0".to_string(),
        category: "candidate".to_string(),
        description: "Auto-generated proposal for a recurring task family.".to_string(),
        use_when: vec![format!("Tasks matching recurring goal family '{}'.", goal)],
        avoid_when: vec!["The recurring pattern disappears or broadens.".to_string()],
        capabilities: vec!["pattern_specialization".to_string()],
        tools: super::models::AgentToolPolicy {
            allowed: vec!["file_search".to_string()],
            forbidden: vec!["shell".to_string()],
        },
        permissions: super::models::PermissionSpec {
            read: vec!["task_context".to_string()],
            write: vec!["artifact_store".to_string()],
            execute: Vec::new(),
            external_side_effect: Vec::new(),
            human_approval_required: false,
        },
        memory_scope: super::models::MemoryScope {
            read: vec!["task_context".to_string()],
            write: vec!["artifact_store".to_string()],
        },
        context_policy: super::models::ContextPolicy {
            max_input_tokens: 4000,
            include: vec![
                "task_spec".to_string(),
                "upstream_results_summary".to_string(),
            ],
            exclude: vec!["full_conversation_history".to_string()],
            summarization_required: true,
        },
        input_schema: serde_json::json!({"type": "object"}),
        output_schema: serde_json::json!({"type": "object"}),
        handoff_targets: vec!["reporter.final".to_string()],
        failure_modes: vec!["Pattern is too broad for a dedicated agent.".to_string()],
        success_criteria: vec![
            "Improves revise or fail rate for the recurring task family.".to_string(),
        ],
        evals: vec!["candidate_agent_eval".to_string()],
        enabled: true,
        instructions: "This is a proposed agent card pending explicit approval.".to_string(),
    }
}
