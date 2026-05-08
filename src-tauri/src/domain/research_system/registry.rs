use super::defaults::DEFAULT_AGENT_CARDS;
use super::models::AgentCard;
use serde::de::DeserializeOwned;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct AgentRegistry {
    cards: BTreeMap<String, BTreeMap<String, AgentCard>>,
    active_versions: BTreeMap<String, String>,
    disabled: BTreeSet<String>,
}

impl AgentRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_cards(cards: Vec<AgentCard>) -> Result<Self, String> {
        let mut registry = Self::new();
        for card in cards {
            registry.register(card)?;
        }
        Ok(registry)
    }

    pub fn register(&mut self, card: AgentCard) -> Result<(), String> {
        validate_agent_card(&card)?;
        let id = card.id.clone();
        let version = card.version.clone();
        let versions = self.cards.entry(id.clone()).or_default();
        versions.insert(version.clone(), card);
        let should_promote = self
            .active_versions
            .get(&id)
            .map(|current| compare_versions(&version, current).is_gt())
            .unwrap_or(true);
        if should_promote {
            self.active_versions.insert(id, version);
        }
        Ok(())
    }

    pub fn disable(&mut self, id: &str) {
        self.disabled.insert(id.to_string());
    }

    pub fn enable(&mut self, id: &str) {
        self.disabled.remove(id);
    }

    pub fn get(&self, id: &str) -> Option<&AgentCard> {
        if self.disabled.contains(id) {
            return None;
        }
        let active = self.active_versions.get(id)?;
        let card = self.cards.get(id)?.get(active)?;
        if card.enabled {
            Some(card)
        } else {
            None
        }
    }

    pub fn get_version(&self, id: &str, version: &str) -> Option<&AgentCard> {
        self.cards.get(id)?.get(version)
    }

    pub fn list(&self) -> Vec<&AgentCard> {
        let mut cards = self
            .active_versions
            .keys()
            .filter_map(|id| self.get(id))
            .collect::<Vec<_>>();
        cards.sort_by(|a, b| a.id.cmp(&b.id));
        cards
    }

    pub fn find_by_category(&self, category: &str) -> Vec<&AgentCard> {
        self.list()
            .into_iter()
            .filter(|card| card.category == category)
            .collect()
    }

    pub fn find_by_capability(&self, capability: &str) -> Vec<&AgentCard> {
        self.list()
            .into_iter()
            .filter(|card| card.capabilities.iter().any(|item| item == capability))
            .collect()
    }

    pub fn find_by_use_when(&self, term: &str) -> Vec<&AgentCard> {
        let lowered = term.to_lowercase();
        self.list()
            .into_iter()
            .filter(|card| {
                card.use_when
                    .iter()
                    .any(|item| item.to_lowercase().contains(&lowered))
            })
            .collect()
    }

    pub fn default_registry() -> Result<Self, String> {
        let cards = DEFAULT_AGENT_CARDS
            .iter()
            .map(|(_, content)| parse_agent_card_markdown(content))
            .collect::<Result<Vec<_>, _>>()?;
        Self::from_cards(cards)
    }
}

pub fn load_agent_registry_from_dir(dir: impl AsRef<Path>) -> Result<AgentRegistry, String> {
    let mut cards = Vec::new();
    for entry in fs::read_dir(dir.as_ref()).map_err(|err| err.to_string())? {
        let entry = entry.map_err(|err| err.to_string())?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
            continue;
        }
        let raw = fs::read_to_string(&path).map_err(|err| err.to_string())?;
        cards.push(parse_agent_card_markdown(&raw)?);
    }
    AgentRegistry::from_cards(cards)
}

pub fn parse_agent_card_markdown(markdown: &str) -> Result<AgentCard, String> {
    let (frontmatter, body) = split_frontmatter(markdown)?;
    let mut card: AgentCard = parse_yaml(frontmatter)?;
    card.instructions = body.trim().to_string();
    validate_agent_card(&card)?;
    Ok(card)
}

pub fn render_agent_card_markdown(card: &AgentCard) -> Result<String, String> {
    validate_agent_card(card)?;
    let mut frontmatter = card.clone();
    let body = frontmatter.instructions.trim().to_string();
    frontmatter.instructions.clear();
    let yaml = serde_yaml::to_string(&frontmatter).map_err(|err| err.to_string())?;
    Ok(format!("---\n{}---\n\n{}\n", yaml, body))
}

pub fn write_default_agent_cards(dir: impl AsRef<Path>) -> Result<Vec<PathBuf>, String> {
    let dir = dir.as_ref();
    fs::create_dir_all(dir).map_err(|err| err.to_string())?;
    let mut written = Vec::new();
    for (name, content) in DEFAULT_AGENT_CARDS {
        let path = dir.join(name);
        if path.exists() {
            continue;
        }
        fs::write(&path, content).map_err(|err| err.to_string())?;
        written.push(path);
    }
    Ok(written)
}

pub fn validate_agent_card(card: &AgentCard) -> Result<(), String> {
    if card.id.trim().is_empty() {
        return Err("agent card id is required".to_string());
    }
    if card.name.trim().is_empty() {
        return Err(format!("agent '{}' is missing name", card.id));
    }
    if card.version.trim().is_empty() {
        return Err(format!("agent '{}' is missing version", card.id));
    }
    if card.category.trim().is_empty() {
        return Err(format!("agent '{}' is missing category", card.id));
    }
    if card.description.trim().is_empty() {
        return Err(format!("agent '{}' is missing description", card.id));
    }
    if card.instructions.trim().is_empty() {
        return Err(format!("agent '{}' is missing instructions body", card.id));
    }
    if card.context_policy.max_input_tokens == 0 {
        return Err(format!(
            "agent '{}' must set context_policy.max_input_tokens > 0",
            card.id
        ));
    }
    Ok(())
}

fn split_frontmatter(markdown: &str) -> Result<(&str, &str), String> {
    let rest = markdown
        .strip_prefix("---\n")
        .ok_or_else(|| "agent card is missing YAML front matter".to_string())?;
    let end = rest
        .find("\n---\n")
        .ok_or_else(|| "agent card front matter is not terminated".to_string())?;
    Ok((&rest[..end], &rest[end + 5..]))
}

fn parse_yaml<T: DeserializeOwned>(raw: &str) -> Result<T, String> {
    serde_yaml::from_str(raw).map_err(|err| err.to_string())
}

fn compare_versions(left: &str, right: &str) -> std::cmp::Ordering {
    let left_parts = parse_version_parts(left);
    let right_parts = parse_version_parts(right);
    left_parts.cmp(&right_parts)
}

fn parse_version_parts(version: &str) -> Vec<u32> {
    version
        .split('.')
        .map(|part| part.parse::<u32>().unwrap_or(0))
        .collect()
}
