use super::context::ContextAssembler;
use super::creator::Creator;
use super::director::ResearchDirector;
use super::executor::Executor;
use super::permissions::PermissionManager;
use super::registry::{write_default_agent_cards, AgentRegistry};
use super::reviewer::Reviewer;
use super::runner::MockAgentRunner;
use super::stores::{
    JsonFileArtifactStore, JsonFileEvidenceStore, JsonFileProposalStore, JsonFileTaskGraphStore,
    JsonFileTraceStore, MarkdownAgentRegistryStore, ProposalStore,
};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};

pub fn run_research_cli(args: &[String], cwd: &Path) -> Result<String, String> {
    let Some(command) = args.first().map(|item| item.as_str()) else {
        return Ok(help_text());
    };
    let layout = WorkspaceLayout::new(cwd);

    match command {
        "init" => init_workspace(&layout),
        "list-agents" => list_agents(&layout),
        "list-proposals" => list_proposals(&layout),
        "plan" => {
            let request = collect_request(&args[1..])?;
            let registry = load_registry(&layout)?;
            let director = ResearchDirector::new(registry, MockAgentRunner::new());
            let (plan, _) = director.prepare(&request)?;
            serde_json::to_string_pretty(&plan).map_err(|err| err.to_string())
        }
        "run" => run_flow(&layout, &collect_request(&args[1..])?),
        "review-traces" => review_traces(&layout),
        "approve-proposal" => approve_proposal(&layout, &collect_identifier(&args[1..])?),
        "help" | "--help" | "-h" => Ok(help_text()),
        other => Err(format!("unknown command '{}'\n\n{}", other, help_text())),
    }
}

fn init_workspace(layout: &WorkspaceLayout) -> Result<String, String> {
    layout.ensure_dirs()?;
    let written = write_default_agent_cards(&layout.agents_dir)?;
    Ok(serde_json::to_string_pretty(&json!({
        "workspace_root": layout.root,
        "agents_dir": layout.agents_dir,
        "state_dir": layout.state_dir,
        "written_files": written,
    }))
    .map_err(|err| err.to_string())?)
}

fn list_agents(layout: &WorkspaceLayout) -> Result<String, String> {
    let registry = load_registry(layout)?;
    let agents = registry
        .list()
        .into_iter()
        .map(|card| {
            json!({
                "id": card.id,
                "name": card.name,
                "version": card.version,
                "category": card.category,
                "capabilities": card.capabilities,
                "enabled": card.enabled,
            })
        })
        .collect::<Vec<_>>();
    serde_json::to_string_pretty(&agents).map_err(|err| err.to_string())
}

fn run_flow(layout: &WorkspaceLayout, request: &str) -> Result<String, String> {
    layout.ensure_dirs()?;
    if !layout.agents_dir.exists() {
        write_default_agent_cards(&layout.agents_dir)?;
    }
    let registry = load_registry(layout)?;
    let director = ResearchDirector::new(registry.clone(), MockAgentRunner::new());
    let (plan, control_plane_report) = director.prepare(request)?;
    let mut executor = Executor::new(
        registry,
        MockAgentRunner::new(),
        Reviewer::new(),
        PermissionManager::new(),
        ContextAssembler::new(),
        Box::new(JsonFileTaskGraphStore::new(&layout.graphs_dir)),
        Box::new(JsonFileArtifactStore::new(&layout.artifacts_dir)),
        Box::new(JsonFileEvidenceStore::new(&layout.evidence_dir)),
        Box::new(JsonFileTraceStore::new(&layout.traces_dir)),
    );
    let mut result = executor.execute(plan)?;
    result.control_plane_report = Some(control_plane_report);
    serde_json::to_string_pretty(&result).map_err(|err| err.to_string())
}

fn review_traces(layout: &WorkspaceLayout) -> Result<String, String> {
    layout.ensure_dirs()?;
    let registry = load_registry(layout)?;
    let trace_store = JsonFileTraceStore::new(&layout.traces_dir);
    let mut proposal_store = JsonFileProposalStore::new(&layout.proposals_dir);
    let creator = Creator::new();
    let proposals = creator.review_traces(&registry, &trace_store, &mut proposal_store)?;
    serde_json::to_string_pretty(&proposals).map_err(|err| err.to_string())
}

fn list_proposals(layout: &WorkspaceLayout) -> Result<String, String> {
    layout.ensure_dirs()?;
    let proposal_store = JsonFileProposalStore::new(&layout.proposals_dir);
    let proposals = proposal_store.list();
    serde_json::to_string_pretty(&proposals).map_err(|err| err.to_string())
}

fn approve_proposal(layout: &WorkspaceLayout, proposal_id: &str) -> Result<String, String> {
    layout.ensure_dirs()?;
    let creator = Creator::new();
    let mut proposal_store = JsonFileProposalStore::new(&layout.proposals_dir);
    let mut registry_store = MarkdownAgentRegistryStore::new(&layout.agents_dir);
    let approved =
        creator.approve_proposal(proposal_id, &mut proposal_store, Some(&mut registry_store))?;
    serde_json::to_string_pretty(&approved).map_err(|err| err.to_string())
}

fn load_registry(layout: &WorkspaceLayout) -> Result<AgentRegistry, String> {
    if layout.agents_dir.exists()
        && fs::read_dir(&layout.agents_dir)
            .map_err(|err| err.to_string())?
            .next()
            .is_some()
    {
        super::registry::load_agent_registry_from_dir(&layout.agents_dir)
    } else {
        AgentRegistry::default_registry()
    }
}

fn collect_request(parts: &[String]) -> Result<String, String> {
    let request = parts.join(" ").trim().to_string();
    if request.is_empty() {
        Err("a user request string is required".to_string())
    } else {
        Ok(request)
    }
}

fn collect_identifier(parts: &[String]) -> Result<String, String> {
    let value = parts.join(" ").trim().to_string();
    if value.is_empty() {
        Err("a proposal id is required".to_string())
    } else {
        Ok(value)
    }
}

fn help_text() -> String {
    [
        "research <command>",
        "",
        "Commands:",
        "  init               Create agents/ and .research/ state directories",
        "  list-agents        List active agent cards",
        "  list-proposals     List saved creator proposals",
        "  plan <request>     Print a TaskGraph for the request",
        "  run <request>      Execute the request with MockAgentRunner",
        "  review-traces      Generate creator proposals from saved traces",
        "  approve-proposal <proposal_id>",
        "                     Approve a proposal and apply or emit its registry patch",
    ]
    .join("\n")
}

struct WorkspaceLayout {
    root: PathBuf,
    agents_dir: PathBuf,
    state_dir: PathBuf,
    graphs_dir: PathBuf,
    artifacts_dir: PathBuf,
    evidence_dir: PathBuf,
    traces_dir: PathBuf,
    proposals_dir: PathBuf,
}

impl WorkspaceLayout {
    fn new(root: &Path) -> Self {
        let state_dir = root.join(".research");
        Self {
            root: root.to_path_buf(),
            agents_dir: root.join("agents"),
            graphs_dir: state_dir.join("graphs"),
            artifacts_dir: state_dir.join("artifacts"),
            evidence_dir: state_dir.join("evidence"),
            traces_dir: state_dir.join("traces"),
            proposals_dir: state_dir.join("proposals"),
            state_dir,
        }
    }

    fn ensure_dirs(&self) -> Result<(), String> {
        fs::create_dir_all(&self.agents_dir).map_err(|err| err.to_string())?;
        fs::create_dir_all(&self.graphs_dir).map_err(|err| err.to_string())?;
        fs::create_dir_all(&self.artifacts_dir).map_err(|err| err.to_string())?;
        fs::create_dir_all(&self.evidence_dir).map_err(|err| err.to_string())?;
        fs::create_dir_all(&self.traces_dir).map_err(|err| err.to_string())?;
        fs::create_dir_all(&self.proposals_dir).map_err(|err| err.to_string())?;
        Ok(())
    }
}
