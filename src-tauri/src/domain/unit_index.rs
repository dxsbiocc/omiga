//! Read-only Unit Index over installed Operator / Template / Skill entries.
//!
//! The Unit Index is a routing/catalog view. It deliberately does not create
//! new execution tools or change existing `operator__*`, `skill`, or retrieval
//! runtime behavior.

use crate::domain::operators::OperatorCandidateSummary;
use crate::domain::skills::{SkillEntry, SkillSource};
use crate::domain::templates::{TemplateCandidateSummary, TemplateSpecWithSource};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};

const STAGE_INFERENCE_READ_LIMIT_BYTES: u64 = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnitKind {
    Operator,
    Template,
    Skill,
}

impl UnitKind {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "operator" | "operators" => Some(Self::Operator),
            "template" | "templates" => Some(Self::Template),
            "skill" | "skills" => Some(Self::Skill),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Operator => "operator",
            Self::Template => "template",
            Self::Skill => "skill",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnitClassification {
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, rename = "stageInput")]
    pub stage_input: Vec<String>,
    #[serde(default, rename = "stageOutput")]
    pub stage_output: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnitExposure {
    #[serde(default, rename = "exposeToAgent")]
    pub expose_to_agent: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnitIndexEntry {
    pub canonical_id: String,
    pub id: String,
    pub kind: UnitKind,
    pub provider_plugin: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub classification: UnitClassification,
    pub exposure: UnitExposure,
    pub source_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub migration_target: Option<String>,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UnitFilter {
    pub kind: Option<UnitKind>,
    pub query: Option<String>,
    pub category: Option<String>,
    pub tag: Option<String>,
    pub stage: Option<String>,
    pub stages: Vec<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StageInference {
    pub path: String,
    pub stages: Vec<String>,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnitDescription {
    pub unit: UnitIndexEntry,
    pub details: serde_json::Value,
}

pub fn build_unit_index(skills: &[SkillEntry]) -> Vec<UnitIndexEntry> {
    let mut units = Vec::new();
    units.extend(operator_units_from_summaries(
        crate::domain::operators::list_operator_summaries(),
    ));
    units.extend(template_units_from_summaries(
        crate::domain::templates::list_template_summaries(),
    ));
    units.extend(skill_units_from_entries(skills));
    sort_units(&mut units);
    units
}

pub fn filter_units(units: &[UnitIndexEntry], filter: &UnitFilter) -> Vec<UnitIndexEntry> {
    let query = normalize_optional(&filter.query);
    let category = normalize_optional(&filter.category);
    let tag = normalize_optional(&filter.tag);
    let stage = normalize_optional(&filter.stage);
    let inferred_stages = filter
        .stages
        .iter()
        .map(|value| normalize(value))
        .filter(|value| !value.is_empty())
        .collect::<BTreeSet<_>>();

    let mut out = units
        .iter()
        .filter(|unit| filter.kind.map(|kind| unit.kind == kind).unwrap_or(true))
        .filter(|unit| {
            query
                .as_deref()
                .map(|q| unit_matches_query(unit, q))
                .unwrap_or(true)
        })
        .filter(|unit| {
            category
                .as_deref()
                .map(|cat| {
                    unit.classification
                        .category
                        .as_deref()
                        .map(normalize)
                        .is_some_and(|value| value.contains(cat))
                })
                .unwrap_or(true)
        })
        .filter(|unit| {
            tag.as_deref()
                .map(|needle| {
                    unit.classification
                        .tags
                        .iter()
                        .any(|candidate| normalize(candidate).contains(needle))
                })
                .unwrap_or(true)
        })
        .filter(|unit| {
            stage
                .as_deref()
                .map(|needle| {
                    unit.classification
                        .stage_input
                        .iter()
                        .chain(unit.classification.stage_output.iter())
                        .any(|candidate| normalize(candidate).contains(needle))
                })
                .unwrap_or(true)
        })
        .filter(|unit| {
            if inferred_stages.is_empty() || stage.is_some() {
                return true;
            }
            unit.classification
                .stage_input
                .iter()
                .chain(unit.classification.stage_output.iter())
                .map(|candidate| normalize(candidate))
                .any(|candidate| {
                    inferred_stages
                        .iter()
                        .any(|needle| candidate.contains(needle))
                })
        })
        .cloned()
        .collect::<Vec<_>>();

    sort_units(&mut out);
    if let Some(limit) = filter.limit.filter(|limit| *limit > 0) {
        out.truncate(limit);
    }
    out
}

pub fn infer_stages_from_paths(paths: &[String]) -> Vec<StageInference> {
    infer_stages_from_paths_with_root(None, paths)
}

pub fn infer_stages_from_paths_with_root(
    project_root: Option<&Path>,
    paths: &[String],
) -> Vec<StageInference> {
    paths
        .iter()
        .filter_map(|path| {
            let trimmed = path.trim();
            if trimmed.is_empty() {
                return None;
            }
            let lower = trimmed.to_ascii_lowercase();
            let filename = std::path::Path::new(trimmed)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(trimmed)
                .to_ascii_lowercase();
            let extension = std::path::Path::new(trimmed)
                .extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let has_de_token = filename
                .split(|ch: char| !ch.is_ascii_alphanumeric())
                .any(|part| part == "de");
            let mut stages = Vec::new();
            let mut reasons = Vec::new();

            if matches!(extension.as_str(), "tsv" | "csv" | "txt") {
                push_stage_reason(&mut stages, &mut reasons, "table", "tabular file extension");
            }
            if matches!(extension.as_str(), "rnk") {
                push_stage_reason(
                    &mut stages,
                    &mut reasons,
                    "ranked_gene_table",
                    ".rnk ranked-list file",
                );
                push_stage_reason(
                    &mut stages,
                    &mut reasons,
                    "gene_list",
                    ".rnk gene-list compatible file",
                );
            }
            if matches!(extension.as_str(), "gmt" | "gmx") {
                push_stage_reason(
                    &mut stages,
                    &mut reasons,
                    "gene_sets",
                    "gene-set file extension",
                );
            }
            if matches!(extension.as_str(), "gct" | "loom" | "h5ad") {
                push_stage_reason(
                    &mut stages,
                    &mut reasons,
                    "expression_matrix",
                    "expression matrix extension",
                );
            }
            if matches!(
                extension.as_str(),
                "png" | "jpg" | "jpeg" | "svg" | "pdf" | "webp"
            ) {
                push_stage_reason(
                    &mut stages,
                    &mut reasons,
                    "static_figure",
                    "static figure extension",
                );
            }

            if filename.contains("count") || filename.contains("counts") {
                push_stage_reason(
                    &mut stages,
                    &mut reasons,
                    "count_matrix",
                    "filename mentions counts",
                );
            }
            if filename.contains("expr")
                || filename.contains("expression")
                || filename.contains("matrix")
            {
                push_stage_reason(
                    &mut stages,
                    &mut reasons,
                    "expression_matrix",
                    "filename mentions expression/matrix",
                );
            }
            if filename.contains("sample")
                || filename.contains("metadata")
                || filename.contains("coldata")
                || filename.contains("phenodata")
            {
                push_stage_reason(
                    &mut stages,
                    &mut reasons,
                    "sample_metadata",
                    "filename suggests sample metadata",
                );
            }
            if has_de_token
                || filename.contains("diff")
                || filename.contains("differential")
                || filename.contains("limma")
                || filename.contains("deseq")
                || filename.contains("marker")
            {
                push_stage_reason(
                    &mut stages,
                    &mut reasons,
                    "diff_results",
                    "filename suggests differential results",
                );
                push_stage_reason(
                    &mut stages,
                    &mut reasons,
                    "ranked_gene_table",
                    "differential results can seed ranked-gene workflows",
                );
            }
            if filename.contains("rank")
                || filename.contains("gene_list")
                || filename.contains("genelist")
            {
                push_stage_reason(
                    &mut stages,
                    &mut reasons,
                    "ranked_gene_table",
                    "filename suggests ranked gene table",
                );
                push_stage_reason(
                    &mut stages,
                    &mut reasons,
                    "gene_list",
                    "filename suggests gene list",
                );
            } else if filename.contains("gene") {
                push_stage_reason(
                    &mut stages,
                    &mut reasons,
                    "gene_list",
                    "filename mentions genes",
                );
            }
            if lower.contains("enrichment")
                || filename.contains("pathway")
                || filename.contains("kegg")
                || filename.contains("go_")
                || filename.starts_with("go")
            {
                push_stage_reason(
                    &mut stages,
                    &mut reasons,
                    "enrichment_results",
                    "filename suggests enrichment results",
                );
            }

            if let Some(root) = project_root {
                append_stage_inference_from_file(root, trimmed, &mut stages, &mut reasons);
            }
            dedup_strings(&mut stages);
            dedup_strings(&mut reasons);
            Some(StageInference {
                path: trimmed.to_string(),
                stages,
                reasons,
            })
        })
        .collect()
}

fn push_stage_reason(
    stages: &mut Vec<String>,
    reasons: &mut Vec<String>,
    stage: &str,
    reason: &str,
) {
    push_unique(stages, stage);
    push_unique(reasons, reason);
}

fn append_stage_inference_from_file(
    project_root: &Path,
    raw_path: &str,
    stages: &mut Vec<String>,
    reasons: &mut Vec<String>,
) {
    let Some(path) = resolve_project_file_for_stage_inference(project_root, raw_path) else {
        return;
    };
    let Ok(mut file) = std::fs::File::open(path) else {
        return;
    };
    let mut bytes = Vec::new();
    if file
        .by_ref()
        .take(STAGE_INFERENCE_READ_LIMIT_BYTES)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return;
    }
    let text = String::from_utf8_lossy(&bytes);
    infer_stages_from_table_preview(&text, stages, reasons);
}

fn resolve_project_file_for_stage_inference(
    project_root: &Path,
    raw_path: &str,
) -> Option<PathBuf> {
    let root = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf());
    let raw = Path::new(raw_path);
    let candidate = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        root.join(raw)
    };
    let candidate = candidate.canonicalize().ok()?;
    if candidate.is_file() && candidate.starts_with(&root) {
        Some(candidate)
    } else {
        None
    }
}

fn infer_stages_from_table_preview(
    text: &str,
    stages: &mut Vec<String>,
    reasons: &mut Vec<String>,
) {
    let rows = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .take(3)
        .collect::<Vec<_>>();
    let Some(header_line) = rows.first() else {
        return;
    };
    let delimiter = if header_line.matches('\t').count() >= header_line.matches(',').count() {
        '\t'
    } else {
        ','
    };
    let header = split_table_line(header_line, delimiter)
        .into_iter()
        .map(|field| normalize_header_token(&field))
        .collect::<Vec<_>>();
    let data = rows
        .get(1)
        .map(|line| split_table_line(line, delimiter))
        .unwrap_or_default();

    let has = |needles: &[&str]| -> bool {
        header
            .iter()
            .any(|field| needles.iter().any(|needle| field.contains(needle)))
    };
    if has(&[
        "sample",
        "condition",
        "group",
        "treatment",
        "batch",
        "phenotype",
    ]) {
        push_stage_reason(
            stages,
            reasons,
            "sample_metadata",
            "table header suggests sample metadata",
        );
    }
    if has(&[
        "log2fc",
        "logfc",
        "foldchange",
        "pvalue",
        "p_val",
        "padj",
        "adjp",
        "fdr",
    ]) {
        push_stage_reason(
            stages,
            reasons,
            "diff_results",
            "table header contains differential-statistics columns",
        );
        push_stage_reason(
            stages,
            reasons,
            "ranked_gene_table",
            "differential statistics can seed ranked-gene workflows",
        );
    }
    if has(&[
        "term",
        "pathway",
        "description",
        "generatio",
        "bgratio",
        "qvalue",
        "ontology",
    ]) {
        push_stage_reason(
            stages,
            reasons,
            "enrichment_results",
            "table header suggests enrichment results",
        );
    }
    if has(&["score", "stat", "rank"]) && has(&["gene", "symbol", "feature", "id"]) {
        push_stage_reason(
            stages,
            reasons,
            "ranked_gene_table",
            "table header suggests ranked gene scores",
        );
        push_stage_reason(
            stages,
            reasons,
            "gene_list",
            "ranked gene scores can be consumed as a gene list",
        );
    }

    let numeric_data_columns = data
        .iter()
        .skip(1)
        .filter(|value| value.trim().parse::<f64>().is_ok())
        .count();
    if data.len() >= 3 && numeric_data_columns >= 2 {
        if has(&["count", "counts"]) {
            push_stage_reason(
                stages,
                reasons,
                "count_matrix",
                "numeric table header/data suggests count matrix",
            );
        } else if has(&["gene", "symbol", "feature", "id"]) || header.len() >= 3 {
            push_stage_reason(
                stages,
                reasons,
                "expression_matrix",
                "numeric table preview suggests expression matrix",
            );
        }
    }
}

fn split_table_line(line: &str, delimiter: char) -> Vec<String> {
    line.split(delimiter)
        .map(|field| {
            field
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string()
        })
        .collect()
}

fn normalize_header_token(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
        .collect()
}

pub fn inferred_stage_terms(inferences: &[StageInference]) -> Vec<String> {
    let mut stages = inferences
        .iter()
        .flat_map(|inference| inference.stages.iter().cloned())
        .collect::<Vec<_>>();
    dedup_strings(&mut stages);
    stages
}

pub fn find_unit_matches(units: &[UnitIndexEntry], raw_id: &str) -> Vec<UnitIndexEntry> {
    let needle = normalize(raw_id);
    if needle.is_empty() {
        return Vec::new();
    }
    let mut matches = units
        .iter()
        .filter(|unit| {
            normalize(&unit.canonical_id) == needle
                || normalize(&unit.id) == needle
                || unit.aliases.iter().any(|alias| normalize(alias) == needle)
        })
        .cloned()
        .collect::<Vec<_>>();
    sort_units(&mut matches);
    matches
}

pub fn describe_unit_by_entry(
    unit: UnitIndexEntry,
    skills: &[SkillEntry],
) -> Option<UnitDescription> {
    match unit.kind {
        UnitKind::Operator => describe_operator_unit(unit),
        UnitKind::Template => describe_template_unit(unit),
        UnitKind::Skill => describe_skill_unit(unit, skills),
    }
}

pub fn operator_units_from_summaries(
    summaries: Vec<OperatorCandidateSummary>,
) -> Vec<UnitIndexEntry> {
    summaries
        .into_iter()
        .map(|summary| {
            let mut tags = summary.tags.clone();
            push_unique(&mut tags, "operator");
            let stage_input = summary
                .interface
                .inputs
                .keys()
                .cloned()
                .collect::<Vec<String>>();
            let stage_output = summary
                .interface
                .outputs
                .keys()
                .cloned()
                .collect::<Vec<String>>();
            UnitIndexEntry {
                canonical_id: canonical_unit_id(
                    &summary.source_plugin,
                    UnitKind::Operator,
                    &summary.id,
                ),
                id: summary.id.clone(),
                kind: UnitKind::Operator,
                provider_plugin: summary.source_plugin,
                aliases: summary.enabled_aliases,
                classification: UnitClassification {
                    category: infer_operator_category(&summary.tags),
                    tags,
                    stage_input,
                    stage_output,
                },
                exposure: UnitExposure {
                    expose_to_agent: summary.exposed,
                },
                source_path: summary.manifest_path,
                migration_target: None,
                status: if summary.exposed {
                    "available".to_string()
                } else {
                    "installed".to_string()
                },
                name: summary.name,
                version: Some(summary.version),
                description: summary.description,
            }
        })
        .collect()
}

pub fn template_units_from_summaries(
    summaries: Vec<TemplateCandidateSummary>,
) -> Vec<UnitIndexEntry> {
    summaries
        .into_iter()
        .map(|summary| {
            let mut tags = summary.tags.clone();
            extend_unique(&mut tags, summary.classification.tags.clone());
            push_unique(&mut tags, "template");
            let mut aliases = summary.aliases.clone();
            aliases.extend(
                summary
                    .migration_target
                    .iter()
                    .filter(|target| target.as_str() != summary.id.as_str())
                    .cloned(),
            );
            dedup_strings(&mut aliases);
            UnitIndexEntry {
                canonical_id: canonical_unit_id(
                    &summary.source_plugin,
                    UnitKind::Template,
                    &summary.id,
                ),
                id: summary.id,
                kind: UnitKind::Template,
                provider_plugin: summary.source_plugin,
                aliases,
                classification: UnitClassification {
                    category: summary.classification.category,
                    tags,
                    stage_input: summary.classification.stage_input,
                    stage_output: summary.classification.stage_output,
                },
                exposure: UnitExposure {
                    expose_to_agent: summary.exposure.expose_to_agent,
                },
                source_path: summary.manifest_path,
                migration_target: summary.migration_target,
                status: "available".to_string(),
                name: summary.name,
                version: Some(summary.version),
                description: summary.description,
            }
        })
        .collect()
}

pub fn template_units_from_specs(specs: Vec<TemplateSpecWithSource>) -> Vec<UnitIndexEntry> {
    let summaries = specs
        .into_iter()
        .map(|candidate| TemplateCandidateSummary {
            id: candidate.spec.metadata.id,
            version: candidate.spec.metadata.version,
            name: candidate.spec.metadata.name,
            description: candidate.spec.metadata.description,
            tags: candidate.spec.metadata.tags,
            source_plugin: candidate.source.source_plugin,
            manifest_path: candidate
                .source
                .manifest_path
                .to_string_lossy()
                .into_owned(),
            classification: candidate.spec.classification,
            exposure: candidate.spec.exposure,
            runtime: candidate.spec.runtime,
            template: candidate.spec.template,
            aliases: candidate.spec.aliases,
            execution: candidate.spec.execution,
            migration_target: candidate.spec.migration_target,
        })
        .collect();
    template_units_from_summaries(summaries)
}

pub fn skill_units_from_entries(skills: &[SkillEntry]) -> Vec<UnitIndexEntry> {
    let providers = skill_plugin_providers();
    skills
        .iter()
        .map(|skill| {
            let provider = provider_for_skill(skill, &providers);
            let mut tags = skill.tags.clone();
            push_unique(&mut tags, "skill");
            UnitIndexEntry {
                canonical_id: canonical_unit_id(&provider, UnitKind::Skill, &skill.name),
                id: skill.name.clone(),
                kind: UnitKind::Skill,
                provider_plugin: provider,
                aliases: Vec::new(),
                classification: UnitClassification {
                    category: Some("workflow/skill".to_string()),
                    tags,
                    stage_input: Vec::new(),
                    stage_output: Vec::new(),
                },
                exposure: UnitExposure {
                    expose_to_agent: true,
                },
                source_path: skill
                    .skill_dir
                    .join("SKILL.md")
                    .to_string_lossy()
                    .into_owned(),
                migration_target: None,
                status: "available".to_string(),
                name: Some(skill.name.clone()),
                version: None,
                description: Some(skill.description.clone()),
            }
        })
        .collect()
}

fn describe_operator_unit(unit: UnitIndexEntry) -> Option<UnitDescription> {
    let summary = crate::domain::operators::list_operator_summaries()
        .into_iter()
        .find(|summary| {
            canonical_unit_id(&summary.source_plugin, UnitKind::Operator, &summary.id)
                == unit.canonical_id
        })?;
    Some(UnitDescription {
        unit,
        details: serde_json::json!({
            "schemaKind": "OperatorCandidateSummary",
            "operator": summary,
            "note": "Read-only Unit Index description. Execute atomic operators with existing operator__* tools; prefer template_execute when a Template wraps this operator as a higher-level workflow."
        }),
    })
}

fn describe_template_unit(unit: UnitIndexEntry) -> Option<UnitDescription> {
    let template = crate::domain::templates::discover_template_candidates()
        .into_iter()
        .find(|candidate| {
            canonical_unit_id(
                &candidate.source.source_plugin,
                UnitKind::Template,
                &candidate.spec.metadata.id,
            ) == unit.canonical_id
        })?;
    let execute = crate::domain::templates::template_execute_example(&template, &unit.canonical_id);
    Some(UnitDescription {
        unit,
        details: serde_json::json!({
            "schemaKind": "TemplateSpec",
            "template": template,
            "execute": execute,
            "note": "Execute this Template with template_execute."
        }),
    })
}

fn describe_skill_unit(unit: UnitIndexEntry, skills: &[SkillEntry]) -> Option<UnitDescription> {
    let skill = skills.iter().find(|skill| skill.name == unit.id)?;
    Some(UnitDescription {
        unit,
        details: serde_json::json!({
            "schemaKind": "SkillReference",
            "skill": {
                "name": skill.name,
                "description": skill.description,
                "whenToUse": skill.when_to_use,
                "tags": skill.tags,
                "source": skill.source,
                "path": skill.skill_dir.join("SKILL.md"),
                "allowedTools": skill.allowed_tools,
                "conditions": skill.conditions,
                "configVars": skill.config_vars,
            },
            "note": "Skill runtime remains the existing skill loader/invoker; Unit Index stores only a reference."
        }),
    })
}

fn canonical_unit_id(provider: &str, kind: UnitKind, id: &str) -> String {
    format!("{provider}/{}/{id}", kind.as_str())
}

fn infer_operator_category(tags: &[String]) -> Option<String> {
    let normalized = tags.iter().map(|tag| normalize(tag)).collect::<Vec<_>>();
    if normalized
        .iter()
        .any(|tag| tag.contains("differential") || tag.contains("rnaseq") || tag.contains("rna"))
    {
        Some("omics/transcriptomics/differential".to_string())
    } else if normalized
        .iter()
        .any(|tag| tag.contains("enrichment") || tag.contains("gsea") || tag.contains("pathway"))
    {
        Some("omics/enrichment".to_string())
    } else if normalized
        .iter()
        .any(|tag| tag.contains("pca") || tag.contains("dimension"))
    {
        Some("omics/dimensionality_reduction".to_string())
    } else if normalized
        .iter()
        .any(|tag| tag.contains("retrieval") || tag.contains("pubmed") || tag.contains("uniprot"))
    {
        Some("utility/data_retrieval".to_string())
    } else {
        Some("operator".to_string())
    }
}

fn skill_plugin_providers() -> Vec<(std::path::PathBuf, String)> {
    let outcome = crate::domain::plugins::plugin_load_outcome();
    outcome
        .plugins()
        .iter()
        .filter(|plugin| plugin.is_active())
        .flat_map(|plugin| {
            plugin
                .skill_roots
                .iter()
                .cloned()
                .map(|root| (root, plugin.id.clone()))
                .collect::<Vec<_>>()
        })
        .collect()
}

fn provider_for_skill(skill: &SkillEntry, providers: &[(std::path::PathBuf, String)]) -> String {
    for (root, provider) in providers {
        if skill.skill_dir.starts_with(root) {
            return provider.clone();
        }
    }
    match skill.source {
        SkillSource::OmigaPlugin => "plugin-skills".to_string(),
        SkillSource::OmigaProject => "project".to_string(),
        SkillSource::OmigaUser => "user".to_string(),
        SkillSource::ClaudeUser => "claude-user".to_string(),
    }
}

fn unit_matches_query(unit: &UnitIndexEntry, query: &str) -> bool {
    let mut haystack = vec![
        unit.canonical_id.as_str(),
        unit.id.as_str(),
        unit.provider_plugin.as_str(),
        unit.source_path.as_str(),
        unit.status.as_str(),
    ];
    if let Some(name) = unit.name.as_deref() {
        haystack.push(name);
    }
    if let Some(description) = unit.description.as_deref() {
        haystack.push(description);
    }
    if let Some(category) = unit.classification.category.as_deref() {
        haystack.push(category);
    }
    unit.aliases
        .iter()
        .chain(unit.classification.tags.iter())
        .chain(unit.classification.stage_input.iter())
        .chain(unit.classification.stage_output.iter())
        .any(|value| normalize(value).contains(query))
        || haystack
            .iter()
            .any(|value| normalize(value).contains(query))
}

fn normalize_optional(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(normalize)
}

fn normalize(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn sort_units(units: &mut [UnitIndexEntry]) {
    units.sort_by(|left, right| {
        unit_kind_order(left.kind)
            .cmp(&unit_kind_order(right.kind))
            .then_with(|| left.provider_plugin.cmp(&right.provider_plugin))
            .then_with(|| left.id.cmp(&right.id))
            .then_with(|| left.canonical_id.cmp(&right.canonical_id))
    });
}

fn unit_kind_order(kind: UnitKind) -> u8 {
    match kind {
        UnitKind::Operator => 0,
        UnitKind::Template => 1,
        UnitKind::Skill => 2,
    }
}

fn push_unique(values: &mut Vec<String>, value: impl Into<String>) {
    extend_unique(values, [value.into()]);
}

fn extend_unique(values: &mut Vec<String>, new_values: impl IntoIterator<Item = String>) {
    let mut seen = values
        .iter()
        .map(|value| normalize(value))
        .collect::<BTreeSet<_>>();
    for value in new_values {
        let trimmed = value.trim();
        if !trimmed.is_empty() && seen.insert(normalize(trimmed)) {
            values.push(trimmed.to_string());
        }
    }
}

fn dedup_strings(values: &mut Vec<String>) {
    let mut seen = BTreeSet::new();
    values.retain(|value| {
        let trimmed = value.trim().to_string();
        !trimmed.is_empty() && seen.insert(normalize(&trimmed))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::skills::{SkillConditions, SkillEntry};

    fn unit(
        id: &str,
        kind: UnitKind,
        category: &str,
        tags: &[&str],
        stage_input: &[&str],
        stage_output: &[&str],
    ) -> UnitIndexEntry {
        UnitIndexEntry {
            canonical_id: canonical_unit_id("provider@market", kind, id),
            id: id.to_string(),
            kind,
            provider_plugin: "provider@market".to_string(),
            aliases: vec![format!("{id}_alias")],
            classification: UnitClassification {
                category: Some(category.to_string()),
                tags: tags.iter().map(|tag| tag.to_string()).collect(),
                stage_input: stage_input.iter().map(|stage| stage.to_string()).collect(),
                stage_output: stage_output.iter().map(|stage| stage.to_string()).collect(),
            },
            exposure: UnitExposure {
                expose_to_agent: true,
            },
            source_path: "/tmp/source".to_string(),
            migration_target: None,
            status: "available".to_string(),
            name: Some(id.to_string()),
            version: Some("0.1.0".to_string()),
            description: Some(format!("{id} description")),
        }
    }

    #[test]
    fn filters_units_by_kind_category_tag_stage_and_query() {
        let units = vec![
            unit(
                "bulk_de",
                UnitKind::Template,
                "omics/transcriptomics/differential",
                &["rna", "differential"],
                &["count_matrix"],
                &["diff_results"],
            ),
            unit(
                "seqtk_sample_reads",
                UnitKind::Operator,
                "operator",
                &["fastq"],
                &["reads"],
                &["sampled_reads"],
            ),
        ];

        let matches = filter_units(
            &units,
            &UnitFilter {
                kind: Some(UnitKind::Template),
                query: Some("bulk".to_string()),
                category: Some("transcriptomics".to_string()),
                tag: Some("rna".to_string()),
                stage: Some("diff_results".to_string()),
                stages: Vec::new(),
                limit: None,
            },
        );

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].id, "bulk_de");
    }

    #[test]
    fn infers_stage_filters_from_common_omics_file_names() {
        let inferences = infer_stages_from_paths(&[
            "data/raw_counts.tsv".to_string(),
            "sample_metadata.csv".to_string(),
            "results/de_results.tsv".to_string(),
            "msigdb.gmt".to_string(),
        ]);
        let stages = inferred_stage_terms(&inferences);

        assert!(stages.iter().any(|stage| stage == "count_matrix"));
        assert!(stages.iter().any(|stage| stage == "sample_metadata"));
        assert!(stages.iter().any(|stage| stage == "diff_results"));
        assert!(stages.iter().any(|stage| stage == "ranked_gene_table"));
        assert!(stages.iter().any(|stage| stage == "gene_sets"));
        assert!(
            !inferences[1]
                .stages
                .iter()
                .any(|stage| stage == "diff_results"),
            "metadata should not be treated as a DE-result token"
        );
    }

    #[test]
    fn inferred_stage_filters_narrow_unit_candidates_when_explicit_stage_is_absent() {
        let units = vec![
            unit(
                "bulk_de",
                UnitKind::Template,
                "omics/transcriptomics/differential",
                &["rna", "differential"],
                &["count_matrix", "sample_metadata"],
                &["diff_results"],
            ),
            unit(
                "enrich",
                UnitKind::Template,
                "omics/enrichment",
                &["enrichment"],
                &["gene_list"],
                &["enrichment_results"],
            ),
        ];

        let matches = filter_units(
            &units,
            &UnitFilter {
                kind: Some(UnitKind::Template),
                stages: vec!["count_matrix".to_string()],
                ..Default::default()
            },
        );

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].id, "bulk_de");
    }

    #[test]
    fn infers_stages_from_project_scoped_table_headers() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("mystery.tsv"),
            "gene\tlog2FoldChange\tpadj\nA\t1.2\t0.01\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("samples.csv"),
            "sample_id,condition,batch\nS1,treated,B1\n",
        )
        .unwrap();

        let inferences = infer_stages_from_paths_with_root(
            Some(tmp.path()),
            &["mystery.tsv".to_string(), "samples.csv".to_string()],
        );
        let stages = inferred_stage_terms(&inferences);

        assert!(stages.iter().any(|stage| stage == "diff_results"));
        assert!(stages.iter().any(|stage| stage == "ranked_gene_table"));
        assert!(stages.iter().any(|stage| stage == "sample_metadata"));
        assert!(inferences[0]
            .reasons
            .iter()
            .any(|reason| reason.contains("differential-statistics")));
    }

    #[test]
    fn find_unit_matches_accepts_canonical_id_short_id_and_alias() {
        let units = vec![unit(
            "bulk_de",
            UnitKind::Template,
            "omics/transcriptomics/differential",
            &["rna"],
            &["count_matrix"],
            &["diff_results"],
        )];

        assert_eq!(
            find_unit_matches(&units, "provider@market/template/bulk_de").len(),
            1
        );
        assert_eq!(find_unit_matches(&units, "bulk_de").len(), 1);
        assert_eq!(find_unit_matches(&units, "bulk_de_alias").len(), 1);
    }

    #[test]
    fn indexes_skills_as_references_without_runtime_reimplementation() {
        let skills = vec![SkillEntry {
            name: "omics-helper".to_string(),
            description: "Helps with omics tasks".to_string(),
            when_to_use: Some("Use for omics".to_string()),
            tags: vec!["omics".to_string()],
            skill_dir: std::path::PathBuf::from("/tmp/project/.omiga/skills/omics-helper"),
            source: SkillSource::OmigaProject,
            allowed_tools: vec!["file_read".to_string()],
            conditions: SkillConditions::default(),
            config_vars: vec![],
        }];

        let units = skill_units_from_entries(&skills);

        assert_eq!(units.len(), 1);
        assert_eq!(units[0].kind, UnitKind::Skill);
        assert_eq!(units[0].provider_plugin, "project");
        assert_eq!(
            units[0].classification.category.as_deref(),
            Some("workflow/skill")
        );
    }

    #[test]
    fn indexes_visualization_r_templates_as_template_units() {
        let plugin_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("bundled_plugins/plugins/visualization-r");
        let template = crate::domain::templates::load_template_manifest(
            &plugin_root
                .join("templates")
                .join("scatter")
                .join("basic")
                .join("template.yaml"),
            "visualization-r@omiga-curated",
            plugin_root,
        )
        .expect("visualization-r template");

        let units = template_units_from_specs(vec![template]);

        assert_eq!(units.len(), 1);
        assert_eq!(units[0].id, "viz_scatter_basic");
        assert_eq!(
            units[0].canonical_id,
            "visualization-r@omiga-curated/template/viz_scatter_basic"
        );
        assert_eq!(
            units[0].classification.category.as_deref(),
            Some("visualization/scatter")
        );
        assert!(units[0]
            .classification
            .tags
            .iter()
            .any(|tag| tag == "ggplot2"));
    }

    #[test]
    fn template_descriptions_include_executable_argument_skeleton() {
        let plugin_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("bundled_plugins/plugins/visualization-r");
        let template = crate::domain::templates::load_template_manifest(
            &plugin_root
                .join("templates")
                .join("scatter")
                .join("basic")
                .join("template.yaml"),
            "visualization-r@omiga-curated",
            plugin_root,
        )
        .expect("visualization-r template");

        let execute = crate::domain::templates::template_execute_example(
            &template,
            "visualization-r@omiga-curated/template/viz_scatter_basic",
        );

        assert_eq!(execute["tool"], "template_execute");
        assert_eq!(
            execute["arguments"]["id"],
            "visualization-r@omiga-curated/template/viz_scatter_basic"
        );
        assert!(
            execute["arguments"]["inputs"]["table"]
                .as_str()
                .is_some_and(|path| path.ends_with("templates/scatter/basic/example.tsv")),
            "{execute}"
        );
        assert_eq!(execute["arguments"]["params"]["x_column"], "x_value");
        assert_eq!(
            execute["arguments"]["params"]["title"],
            "Basic scatter plot"
        );
    }
}
