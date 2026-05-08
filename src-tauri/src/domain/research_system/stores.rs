use super::models::AgentCard;
use super::models::{AgentPatchProposal, ArtifactRecord, EvidenceRecord, TaskGraph, TraceRecord};
use super::registry::{load_agent_registry_from_dir, render_agent_card_markdown, AgentRegistry};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

pub trait TaskGraphStore {
    fn save(&mut self, graph: TaskGraph) -> Result<(), String>;
    fn get(&self, graph_id: &str) -> Option<TaskGraph>;
    fn list(&self) -> Vec<TaskGraph>;
}

pub trait ArtifactStore {
    fn save(&mut self, artifact: ArtifactRecord) -> Result<(), String>;
    fn get(&self, artifact_id: &str) -> Option<ArtifactRecord>;
    fn list(&self) -> Vec<ArtifactRecord>;
}

pub trait EvidenceStore {
    fn save(&mut self, evidence: EvidenceRecord) -> Result<(), String>;
    fn get(&self, evidence_id: &str) -> Option<EvidenceRecord>;
    fn list(&self) -> Vec<EvidenceRecord>;
}

pub trait TraceStore {
    fn append(&mut self, trace: TraceRecord) -> Result<(), String>;
    fn list(&self) -> Vec<TraceRecord>;
    fn list_by_graph(&self, graph_id: &str) -> Vec<TraceRecord>;
}

pub trait ProposalStore {
    fn save(&mut self, proposal: AgentPatchProposal) -> Result<(), String>;
    fn get(&self, proposal_id: &str) -> Option<AgentPatchProposal>;
    fn list(&self) -> Vec<AgentPatchProposal>;
    fn update(&mut self, proposal: AgentPatchProposal) -> Result<(), String>;
}

pub trait AgentRegistryStore {
    fn load(&self) -> Result<AgentRegistry, String>;
    fn save_card(&mut self, card: &AgentCard) -> Result<(), String>;
    fn disable_agent(&mut self, agent_id: &str) -> Result<(), String>;
}

#[derive(Debug, Default)]
pub struct InMemoryTaskGraphStore {
    graphs: HashMap<String, TaskGraph>,
}

impl TaskGraphStore for InMemoryTaskGraphStore {
    fn save(&mut self, graph: TaskGraph) -> Result<(), String> {
        self.graphs.insert(graph.graph_id.clone(), graph);
        Ok(())
    }

    fn get(&self, graph_id: &str) -> Option<TaskGraph> {
        self.graphs.get(graph_id).cloned()
    }

    fn list(&self) -> Vec<TaskGraph> {
        self.graphs.values().cloned().collect()
    }
}

#[derive(Debug, Default)]
pub struct InMemoryArtifactStore {
    artifacts: HashMap<String, ArtifactRecord>,
}

impl ArtifactStore for InMemoryArtifactStore {
    fn save(&mut self, artifact: ArtifactRecord) -> Result<(), String> {
        self.artifacts.insert(artifact.id.clone(), artifact);
        Ok(())
    }

    fn get(&self, artifact_id: &str) -> Option<ArtifactRecord> {
        self.artifacts.get(artifact_id).cloned()
    }

    fn list(&self) -> Vec<ArtifactRecord> {
        self.artifacts.values().cloned().collect()
    }
}

#[derive(Debug, Default)]
pub struct InMemoryEvidenceStore {
    evidence: HashMap<String, EvidenceRecord>,
}

impl EvidenceStore for InMemoryEvidenceStore {
    fn save(&mut self, evidence: EvidenceRecord) -> Result<(), String> {
        self.evidence.insert(evidence.id.clone(), evidence);
        Ok(())
    }

    fn get(&self, evidence_id: &str) -> Option<EvidenceRecord> {
        self.evidence.get(evidence_id).cloned()
    }

    fn list(&self) -> Vec<EvidenceRecord> {
        self.evidence.values().cloned().collect()
    }
}

#[derive(Debug, Default)]
pub struct InMemoryTraceStore {
    traces: Vec<TraceRecord>,
}

impl TraceStore for InMemoryTraceStore {
    fn append(&mut self, trace: TraceRecord) -> Result<(), String> {
        self.traces.push(trace);
        Ok(())
    }

    fn list(&self) -> Vec<TraceRecord> {
        self.traces.clone()
    }

    fn list_by_graph(&self, graph_id: &str) -> Vec<TraceRecord> {
        self.traces
            .iter()
            .filter(|trace| trace.graph_id == graph_id)
            .cloned()
            .collect()
    }
}

#[derive(Debug, Default)]
pub struct InMemoryProposalStore {
    proposals: HashMap<String, AgentPatchProposal>,
}

impl ProposalStore for InMemoryProposalStore {
    fn save(&mut self, proposal: AgentPatchProposal) -> Result<(), String> {
        self.proposals
            .insert(proposal.proposal_id.clone(), proposal);
        Ok(())
    }

    fn get(&self, proposal_id: &str) -> Option<AgentPatchProposal> {
        self.proposals.get(proposal_id).cloned()
    }

    fn list(&self) -> Vec<AgentPatchProposal> {
        self.proposals.values().cloned().collect()
    }

    fn update(&mut self, proposal: AgentPatchProposal) -> Result<(), String> {
        self.save(proposal)
    }
}

#[derive(Debug)]
pub struct InMemoryAgentRegistryStore {
    registry: AgentRegistry,
}

impl InMemoryAgentRegistryStore {
    pub fn new(registry: AgentRegistry) -> Self {
        Self { registry }
    }
}

impl AgentRegistryStore for InMemoryAgentRegistryStore {
    fn load(&self) -> Result<AgentRegistry, String> {
        Ok(self.registry.clone())
    }

    fn save_card(&mut self, card: &AgentCard) -> Result<(), String> {
        self.registry.register(card.clone())
    }

    fn disable_agent(&mut self, agent_id: &str) -> Result<(), String> {
        self.registry.disable(agent_id);
        Ok(())
    }
}

#[derive(Debug)]
pub struct JsonFileTaskGraphStore {
    dir: PathBuf,
}

impl JsonFileTaskGraphStore {
    pub fn new(dir: impl AsRef<Path>) -> Self {
        Self {
            dir: dir.as_ref().to_path_buf(),
        }
    }
}

impl TaskGraphStore for JsonFileTaskGraphStore {
    fn save(&mut self, graph: TaskGraph) -> Result<(), String> {
        write_json_record(&self.dir, &graph.graph_id, &graph)
    }

    fn get(&self, graph_id: &str) -> Option<TaskGraph> {
        read_json_record(self.dir.join(format!("{}.json", graph_id))).ok()
    }

    fn list(&self) -> Vec<TaskGraph> {
        read_json_dir(&self.dir).unwrap_or_default()
    }
}

#[derive(Debug)]
pub struct JsonFileArtifactStore {
    dir: PathBuf,
}

impl JsonFileArtifactStore {
    pub fn new(dir: impl AsRef<Path>) -> Self {
        Self {
            dir: dir.as_ref().to_path_buf(),
        }
    }
}

impl ArtifactStore for JsonFileArtifactStore {
    fn save(&mut self, artifact: ArtifactRecord) -> Result<(), String> {
        write_json_record(&self.dir, &artifact.id, &artifact)
    }

    fn get(&self, artifact_id: &str) -> Option<ArtifactRecord> {
        read_json_record(self.dir.join(format!("{}.json", artifact_id))).ok()
    }

    fn list(&self) -> Vec<ArtifactRecord> {
        read_json_dir(&self.dir).unwrap_or_default()
    }
}

#[derive(Debug)]
pub struct JsonFileEvidenceStore {
    dir: PathBuf,
}

impl JsonFileEvidenceStore {
    pub fn new(dir: impl AsRef<Path>) -> Self {
        Self {
            dir: dir.as_ref().to_path_buf(),
        }
    }
}

impl EvidenceStore for JsonFileEvidenceStore {
    fn save(&mut self, evidence: EvidenceRecord) -> Result<(), String> {
        write_json_record(&self.dir, &evidence.id, &evidence)
    }

    fn get(&self, evidence_id: &str) -> Option<EvidenceRecord> {
        read_json_record(self.dir.join(format!("{}.json", evidence_id))).ok()
    }

    fn list(&self) -> Vec<EvidenceRecord> {
        read_json_dir(&self.dir).unwrap_or_default()
    }
}

#[derive(Debug)]
pub struct JsonFileTraceStore {
    dir: PathBuf,
}

impl JsonFileTraceStore {
    pub fn new(dir: impl AsRef<Path>) -> Self {
        Self {
            dir: dir.as_ref().to_path_buf(),
        }
    }
}

impl TraceStore for JsonFileTraceStore {
    fn append(&mut self, trace: TraceRecord) -> Result<(), String> {
        write_json_record(&self.dir, &trace.id, &trace)
    }

    fn list(&self) -> Vec<TraceRecord> {
        read_json_dir(&self.dir).unwrap_or_default()
    }

    fn list_by_graph(&self, graph_id: &str) -> Vec<TraceRecord> {
        self.list()
            .into_iter()
            .filter(|trace| trace.graph_id == graph_id)
            .collect()
    }
}

#[derive(Debug)]
pub struct JsonFileProposalStore {
    dir: PathBuf,
}

impl JsonFileProposalStore {
    pub fn new(dir: impl AsRef<Path>) -> Self {
        Self {
            dir: dir.as_ref().to_path_buf(),
        }
    }
}

impl ProposalStore for JsonFileProposalStore {
    fn save(&mut self, proposal: AgentPatchProposal) -> Result<(), String> {
        write_json_record(&self.dir, &proposal.proposal_id, &proposal)
    }

    fn get(&self, proposal_id: &str) -> Option<AgentPatchProposal> {
        read_json_record(self.dir.join(format!("{}.json", proposal_id))).ok()
    }

    fn list(&self) -> Vec<AgentPatchProposal> {
        read_json_dir(&self.dir).unwrap_or_default()
    }

    fn update(&mut self, proposal: AgentPatchProposal) -> Result<(), String> {
        self.save(proposal)
    }
}

#[derive(Debug)]
pub struct MarkdownAgentRegistryStore {
    dir: PathBuf,
}

impl MarkdownAgentRegistryStore {
    pub fn new(dir: impl AsRef<Path>) -> Self {
        Self {
            dir: dir.as_ref().to_path_buf(),
        }
    }
}

impl AgentRegistryStore for MarkdownAgentRegistryStore {
    fn load(&self) -> Result<AgentRegistry, String> {
        load_agent_registry_from_dir(&self.dir)
    }

    fn save_card(&mut self, card: &AgentCard) -> Result<(), String> {
        fs::create_dir_all(&self.dir).map_err(|err| err.to_string())?;
        let markdown = render_agent_card_markdown(card)?;
        fs::write(self.dir.join(format!("{}.md", card.id)), markdown).map_err(|err| err.to_string())
    }

    fn disable_agent(&mut self, agent_id: &str) -> Result<(), String> {
        let mut registry = self.load()?;
        let mut card = registry
            .get(agent_id)
            .cloned()
            .ok_or_else(|| format!("agent '{}' not found", agent_id))?;
        card.enabled = false;
        self.save_card(&card)?;
        registry.disable(agent_id);
        Ok(())
    }
}

fn write_json_record<T: Serialize>(dir: &Path, id: &str, value: &T) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|err| err.to_string())?;
    let json = serde_json::to_string_pretty(value).map_err(|err| err.to_string())?;
    fs::write(dir.join(format!("{}.json", id)), json).map_err(|err| err.to_string())
}

fn read_json_record<T: DeserializeOwned>(path: PathBuf) -> Result<T, String> {
    let raw = fs::read_to_string(path).map_err(|err| err.to_string())?;
    serde_json::from_str(&raw).map_err(|err| err.to_string())
}

fn read_json_dir<T: DeserializeOwned>(dir: &Path) -> Result<Vec<T>, String> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut values = Vec::new();
    for entry in fs::read_dir(dir).map_err(|err| err.to_string())? {
        let path = entry.map_err(|err| err.to_string())?.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        values.push(read_json_record(path)?);
    }
    Ok(values)
}
