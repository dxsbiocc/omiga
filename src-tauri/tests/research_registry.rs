use omiga_lib::domain::research_system::{parse_agent_card_markdown, AgentRegistry};

const SEEKER_CARD: &str =
    include_str!("../src/domain/research_system/defaults/agents/seeker.web_research.md");

#[test]
fn loads_agent_card_from_yaml_frontmatter_markdown() {
    let card = parse_agent_card_markdown(SEEKER_CARD).expect("default seeker card should parse");
    assert_eq!(card.id, "seeker.web_research");
    assert_eq!(card.category, "retrieval");
    assert!(card.instructions.contains("你负责检索"));
}

#[test]
fn registry_can_get_agent_and_search_metadata() {
    let registry = AgentRegistry::default_registry().expect("default registry");
    let seeker = registry.get("seeker.web_research").expect("seeker present");
    assert_eq!(seeker.name, "Seeker");

    let retrieval = registry.find_by_category("retrieval");
    assert!(retrieval
        .iter()
        .any(|card| card.id == "seeker.web_research"));

    let capability = registry.find_by_capability("evidence_extraction");
    assert!(capability
        .iter()
        .any(|card| card.id == "seeker.web_research"));

    let use_when = registry.find_by_use_when("可视化");
    assert!(use_when
        .iter()
        .any(|card| card.id == "painter.visualization"));
}
