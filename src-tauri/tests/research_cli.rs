use omiga_lib::domain::research_system::{
    run_research_cli, AgentPatchAction, AgentPatchProposal, ApprovalStatus, JsonFileProposalStore,
    ProposalStore,
};
use tempfile::tempdir;

#[test]
fn cli_init_and_list_agents_smoke() {
    let temp = tempdir().expect("temp dir");

    let init = run_research_cli(&["init".to_string()], temp.path()).expect("run init");
    assert!(init.contains("\"agents_dir\""));
    assert!(init.contains(".research"));

    let list =
        run_research_cli(&["list-agents".to_string()], temp.path()).expect("run list-agents");

    assert!(list.contains("seeker.web_research"));
    assert!(list.contains("creator.capability_refactorer"));
}

#[test]
fn cli_init_does_not_overwrite_existing_agent_cards() {
    let temp = tempdir().expect("temp dir");

    let init = run_research_cli(&["init".to_string()], temp.path()).expect("run init");
    assert!(init.contains("\"written_files\""));

    let card_path = temp.path().join("agents").join("seeker.web_research.md");
    let custom_contents = "---\nid: seeker.web_research\nname: Custom Seeker\nversion: 9.9.9\ncategory: retrieval\ndescription: customized\nuse_when: []\navoid_when: []\ncapabilities: []\ntools:\n  allowed: []\n  forbidden: []\npermissions:\n  read: []\n  write: []\n  execute: []\n  external_side_effect: []\n  human_approval_required: false\nmemory_scope:\n  read: []\n  write: []\ncontext_policy:\n  max_input_tokens: 1000\n  include: []\n  exclude: []\n  summarization_required: false\ninput_schema: {}\noutput_schema: {}\nhandoff_targets: []\nfailure_modes: []\nsuccess_criteria: []\nevals: []\nenabled: true\n---\n\ncustom body\n";
    std::fs::write(&card_path, custom_contents).expect("write custom card");

    let second_init =
        run_research_cli(&["init".to_string()], temp.path()).expect("run second init");
    assert!(second_init.contains("\"written_files\""));

    let current_contents = std::fs::read_to_string(&card_path).expect("read custom card");
    assert_eq!(current_contents, custom_contents);
}

#[test]
fn run_command_includes_control_plane_report() {
    let temp = tempdir().expect("temp dir");
    let output = run_research_cli(
        &["run".to_string(), "写一个简短总结".to_string()],
        temp.path(),
    )
    .expect("run research");

    assert!(output.contains("\"control_plane_report\""));
    assert!(output.contains("mind_hunter.intake"));
    assert!(output.contains("planner.task_graph"));
}

#[test]
fn list_proposals_returns_saved_proposals() {
    let temp = tempdir().expect("temp dir");
    let proposals_dir = temp.path().join(".research").join("proposals");
    let mut store = JsonFileProposalStore::new(&proposals_dir);
    store
        .save(AgentPatchProposal {
            proposal_id: "proposal-1".to_string(),
            action: AgentPatchAction::Create,
            candidate_agent: None,
            target_agents: Vec::new(),
            reason: "test".to_string(),
            expected_benefit: "test".to_string(),
            required_tools: Vec::new(),
            eval_plan: Vec::new(),
            rollback_plan: Vec::new(),
            approval_status: ApprovalStatus::Pending,
            registry_patch: None,
        })
        .expect("save proposal");

    let output =
        run_research_cli(&["list-proposals".to_string()], temp.path()).expect("list proposals");
    assert!(output.contains("proposal-1"));
}

#[test]
fn approve_proposal_command_returns_registry_patch() {
    let temp = tempdir().expect("temp dir");
    run_research_cli(&["init".to_string()], temp.path()).expect("init");

    let proposals_dir = temp.path().join(".research").join("proposals");
    let mut store = JsonFileProposalStore::new(&proposals_dir);
    store
        .save(AgentPatchProposal {
            proposal_id: "proposal-split".to_string(),
            action: AgentPatchAction::Split,
            candidate_agent: None,
            target_agents: vec!["analyzer.data".to_string()],
            reason: "test split".to_string(),
            expected_benefit: "narrow scope".to_string(),
            required_tools: Vec::new(),
            eval_plan: vec!["replay".to_string()],
            rollback_plan: vec!["restore".to_string()],
            approval_status: ApprovalStatus::Pending,
            registry_patch: None,
        })
        .expect("save split proposal");

    let output = run_research_cli(
        &["approve-proposal".to_string(), "proposal-split".to_string()],
        temp.path(),
    )
    .expect("approve proposal");

    assert!(output.contains("\"approval_status\": \"approved\""));
    assert!(output.contains("\"registry_patch\""));
    assert!(output.contains("\"mode\": \"manual\""));
}
