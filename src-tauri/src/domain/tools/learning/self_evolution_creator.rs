use super::{ToolContext, ToolError, ToolImpl, ToolSchema};
use crate::infrastructure::streaming::{stream_single, StreamOutputItem};
use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::Path;
use uuid::Uuid;

pub const DESCRIPTION: &str = "Create or refresh the dedicated project skill that helps self-evolution author review-first Operator and Template drafts.";

const CREATOR_SKILL_NAME: &str = "self-evolution-unit-creator";
const CREATOR_SKILL_DIR_RELATIVE: &str = ".omiga/skills/self-evolution-unit-creator";
const CREATOR_DRAFT_ROOT_RELATIVE: &str = ".omiga/learning/self-evolution-drafts";
const SAFETY_NOTE: &str = "Self-evolution creator bootstrap and draft-package generation only. This tool writes or refreshes one project Skill under .omiga/skills and optional inert draft packages under .omiga/learning/self-evolution-drafts; it does not write active Operator/Template targets, register units, change defaults, or mutate archives.";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LearningSelfEvolutionCreatorArgs {
    /// When true, replace the generated creator skill if it already exists.
    #[serde(default)]
    pub refresh: bool,
    /// Optional package kind to create immediately: operator/operator_candidate or template/template_candidate.
    #[serde(default)]
    pub unit_kind: Option<String>,
    /// Optional stable draft unit id. Defaults to a slug from title/kind.
    #[serde(default)]
    pub unit_id: Option<String>,
    /// Optional human-facing title for the generated draft package.
    #[serde(default)]
    pub title: Option<String>,
    /// Optional rationale/description copied into candidate.json and draft manifests.
    #[serde(default)]
    pub description: Option<String>,
    /// Optional source ExecutionRecord ids copied into review metadata.
    #[serde(default)]
    pub source_record_ids: Vec<String>,
    /// Optional note copied into the draft README.
    #[serde(default)]
    pub review_note: Option<String>,
}

pub struct LearningSelfEvolutionCreatorTool;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CreatorUnitKind {
    Operator,
    Template,
}

impl CreatorUnitKind {
    fn parse(raw: &str) -> Result<Self, ToolError> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "operator" | "operator_candidate" => Ok(Self::Operator),
            "template" | "template_candidate" => Ok(Self::Template),
            other => Err(ToolError::InvalidArguments {
                message: format!(
                    "Invalid unitKind `{other}`; expected operator, operator_candidate, template, or template_candidate."
                ),
            }),
        }
    }

    fn candidate_kind(self) -> &'static str {
        match self {
            Self::Operator => "operator_candidate",
            Self::Template => "template_candidate",
        }
    }

    fn unit_kind(self) -> &'static str {
        match self {
            Self::Operator => "operator",
            Self::Template => "template",
        }
    }

    fn title_fallback(self) -> &'static str {
        match self {
            Self::Operator => "Draft Operator",
            Self::Template => "Draft Template",
        }
    }

    fn target_hint(self, slug: &str) -> String {
        match self {
            Self::Operator => format!("plugins/<plugin-id>/operators/{slug}/operator.yaml"),
            Self::Template => format!("plugins/<plugin-id>/templates/{slug}/template.yaml"),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CreatorDraftPackage {
    status: String,
    candidate_id: String,
    kind: String,
    unit_id: String,
    title: String,
    draft_dir: String,
    files: Vec<String>,
    proposed_target_hint: String,
    safety_note: String,
}

#[async_trait]
impl ToolImpl for LearningSelfEvolutionCreatorTool {
    type Args = LearningSelfEvolutionCreatorArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        let skill_dir = ctx.project_root.join(CREATOR_SKILL_DIR_RELATIVE);
        let skill_path = skill_dir.join("SKILL.md");
        let existed = tokio::fs::metadata(&skill_path).await.is_ok();

        let status = if existed && !args.refresh {
            "already_exists"
        } else {
            tokio::fs::create_dir_all(&skill_dir).await.map_err(|err| {
                ToolError::ExecutionFailed {
                    message: format!("create self-evolution creator skill dir: {err}"),
                }
            })?;
            tokio::fs::write(&skill_path, creator_skill_markdown())
                .await
                .map_err(|err| ToolError::ExecutionFailed {
                    message: format!("write self-evolution creator SKILL.md: {err}"),
                })?;
            if let Some(cache) = &ctx.skill_cache {
                crate::domain::skills::invalidate_skill_cache(&ctx.project_root, cache);
            }
            if existed {
                "refreshed"
            } else {
                "created"
            }
        };
        let draft_package = if args
            .unit_kind
            .as_deref()
            .is_some_and(|kind| !kind.trim().is_empty())
        {
            Some(write_creator_draft_package(ctx, &args).await?)
        } else {
            None
        };

        let skills = crate::domain::skills::load_skills_for_project(&ctx.project_root).await;
        let skill = skills.iter().find(|skill| skill.name == CREATOR_SKILL_NAME);
        let units = crate::domain::unit_index::build_unit_index(&skills);
        let unit = units.iter().find(|unit| unit.id == CREATOR_SKILL_NAME);
        let unit_index_kind = unit.map(|entry| match entry.kind {
            crate::domain::unit_index::UnitKind::Operator => "operator",
            crate::domain::unit_index::UnitKind::Template => "template",
            crate::domain::unit_index::UnitKind::Skill => "skill",
        });
        let output = serde_json::json!({
            "status": status,
            "skillName": CREATOR_SKILL_NAME,
            "skillDir": project_relative_path(&ctx.project_root, &skill_dir),
            "skillPath": project_relative_path(&ctx.project_root, &skill_path),
            "skillDiscovered": skill.is_some(),
            "unitIndexKind": unit_index_kind,
            "unitCanonicalId": unit.map(|entry| entry.canonical_id.clone()),
            "creatorScope": ["operator", "template"],
            "draftPackage": draft_package,
            "nextSteps": [
                "Invoke Skill/self-evolution-unit-creator with a candidate id or draft directory.",
                "Or call learning_self_evolution_creator with unitKind=operator/template to create an inert review draft package.",
                "Let the skill author review-only Operator/Template draft files and validation notes.",
                "Use the existing promotion preview/artifact/readiness/apply gates for any active target write."
            ],
            "safetyNote": SAFETY_NOTE,
        });
        Ok(stream_single(StreamOutputItem::Text(
            serde_json::to_string_pretty(&output).unwrap_or_else(|_| "{}".to_string()),
        )))
    }
}

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "learning_self_evolution_creator",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "refresh": {
                    "type": "boolean",
                    "description": "When true, overwrite the generated project Skill if it already exists. Defaults to false."
                },
                "unitKind": {
                    "type": "string",
                    "enum": ["operator", "operator_candidate", "template", "template_candidate"],
                    "description": "Optional: when set, also create an inert Operator/Template draft package under .omiga/learning/self-evolution-drafts."
                },
                "unitId": {
                    "type": "string",
                    "description": "Optional stable id for the generated Operator/Template draft."
                },
                "title": {
                    "type": "string",
                    "description": "Optional human-facing title for the generated draft package."
                },
                "description": {
                    "type": "string",
                    "description": "Optional rationale copied into candidate.json and review metadata."
                },
                "sourceRecordIds": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Optional ExecutionRecord ids copied into the draft review metadata."
                },
                "reviewNote": {
                    "type": "string",
                    "description": "Optional note copied into the draft README."
                }
            }
        }),
    )
}

async fn write_creator_draft_package(
    ctx: &ToolContext,
    args: &LearningSelfEvolutionCreatorArgs,
) -> Result<CreatorDraftPackage, ToolError> {
    let kind = CreatorUnitKind::parse(args.unit_kind.as_deref().unwrap_or(""))?;
    let title = args
        .title
        .as_deref()
        .map(inline)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            args.unit_id
                .as_deref()
                .map(inline)
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| kind.title_fallback().to_string())
        });
    let unit_id = args
        .unit_id
        .as_deref()
        .map(safe_slug)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| safe_slug(&title));
    let candidate_id = format!("creator-{}-{unit_id}", kind.candidate_kind());
    let generated_at = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let batch_dir = ctx
        .project_root
        .join(CREATOR_DRAFT_ROOT_RELATIVE)
        .join(format!(
            "creator-batch-{}-{}",
            Utc::now().format("%Y%m%dT%H%M%SZ"),
            Uuid::new_v4().simple()
        ));
    let draft_dir = batch_dir.join(format!(
        "01-{}-{}",
        kind.candidate_kind().replace('_', "-"),
        unit_id
    ));
    tokio::fs::create_dir_all(&draft_dir)
        .await
        .map_err(|err| ToolError::ExecutionFailed {
            message: format!("create creator draft package dir: {err}"),
        })?;

    let candidate = serde_json::json!({
        "id": &candidate_id,
        "kind": kind.candidate_kind(),
        "priority": "medium",
        "title": &title,
        "rationale": args.description.as_deref().map(inline).unwrap_or_else(|| {
            format!("Review-first {} draft created by the self-evolution unit creator.", kind.unit_kind())
        }),
        "proposedNextStep": "Review generated draft files, add deterministic fixtures/tests, then use promotion preview/artifact/readiness/apply gates for any active target write.",
        "sourceRecordIds": &args.source_record_ids,
        "evidence": {
            "createdBy": "learning_self_evolution_creator",
            "unitKind": kind.unit_kind(),
            "unitId": &unit_id,
            "proposedTargetHint": kind.target_hint(&unit_id),
        },
        "safetyNote": SAFETY_NOTE,
    });

    let mut files = Vec::new();
    let readme_path = draft_dir.join("DRAFT.md");
    write_text_file(
        &readme_path,
        render_creator_draft_readme(
            kind,
            &generated_at,
            &candidate_id,
            &unit_id,
            &title,
            args.description.as_deref(),
            args.review_note.as_deref(),
        ),
    )
    .await?;
    files.push(project_relative_path(&ctx.project_root, &readme_path));

    let candidate_path = draft_dir.join("candidate.json");
    let candidate_text =
        serde_json::to_string_pretty(&candidate).map_err(|err| ToolError::ExecutionFailed {
            message: format!("serialize creator candidate JSON: {err}"),
        })?;
    write_text_file(&candidate_path, candidate_text).await?;
    files.push(project_relative_path(&ctx.project_root, &candidate_path));

    for (filename, contents) in creator_package_files(kind, &unit_id, &title, args) {
        let path = draft_dir.join(filename);
        write_text_file(&path, contents).await?;
        files.push(project_relative_path(&ctx.project_root, &path));
    }

    Ok(CreatorDraftPackage {
        status: "draft_package_created".to_string(),
        candidate_id,
        kind: kind.candidate_kind().to_string(),
        unit_id: unit_id.clone(),
        title,
        draft_dir: project_relative_path(&ctx.project_root, &draft_dir),
        files,
        proposed_target_hint: kind.target_hint(&unit_id),
        safety_note: SAFETY_NOTE.to_string(),
    })
}

async fn write_text_file(path: &Path, contents: String) -> Result<(), ToolError> {
    tokio::fs::write(path, contents)
        .await
        .map_err(|err| ToolError::ExecutionFailed {
            message: format!("write creator draft file {}: {err}", path.display()),
        })
}

fn creator_package_files(
    kind: CreatorUnitKind,
    unit_id: &str,
    title: &str,
    args: &LearningSelfEvolutionCreatorArgs,
) -> Vec<(&'static str, String)> {
    match kind {
        CreatorUnitKind::Operator => vec![
            (
                "operator.yaml.draft",
                operator_yaml_draft(unit_id, title, args),
            ),
            ("operator.py.draft", operator_script_draft(unit_id, title)),
            ("fixture.json.draft", operator_fixture_draft(unit_id)),
        ],
        CreatorUnitKind::Template => vec![
            (
                "template.yaml.draft",
                template_yaml_draft(unit_id, title, args),
            ),
            ("template.sh.j2.draft", template_entry_draft(unit_id, title)),
            ("example-input.tsv.draft", template_example_input_draft()),
        ],
    }
}

fn render_creator_draft_readme(
    kind: CreatorUnitKind,
    generated_at: &str,
    candidate_id: &str,
    unit_id: &str,
    title: &str,
    description: Option<&str>,
    review_note: Option<&str>,
) -> String {
    let mut out = String::new();
    out.push_str("# Self-Evolution Unit Creator Draft\n\n");
    out.push_str(&format!("- Generated at: `{generated_at}`\n"));
    out.push_str(&format!("- Safety: {SAFETY_NOTE}\n"));
    out.push_str(&format!("- Candidate id: `{candidate_id}`\n"));
    out.push_str(&format!("- Kind: `{}`\n", kind.candidate_kind()));
    out.push_str(&format!("- Unit id: `{unit_id}`\n"));
    out.push_str(&format!("- Title: {}\n", inline(title)));
    out.push_str(&format!(
        "- Proposed target hint: `{}`\n",
        kind.target_hint(unit_id)
    ));
    if let Some(description) = description.map(inline).filter(|value| !value.is_empty()) {
        out.push_str(&format!("- Description: {description}\n"));
    }
    if let Some(note) = review_note.map(inline).filter(|value| !value.is_empty()) {
        out.push_str(&format!("- Review note: {note}\n"));
    }
    out.push_str("\n## Creator checklist\n\n");
    out.push_str("- [ ] Confirm the reusable behavior should be an Operator or Template.\n");
    out.push_str("- [ ] Inspect `candidate.json` and source ExecutionRecord ids.\n");
    out.push_str("- [ ] Complete TODOs in companion `.draft` files.\n");
    out.push_str("- [ ] Add deterministic fixture/smoke validation before promotion.\n");
    out.push_str(
        "- [ ] Use promotion preview/artifact/readiness/apply gates for any active target write.\n",
    );
    out
}

fn operator_yaml_draft(
    unit_id: &str,
    title: &str,
    args: &LearningSelfEvolutionCreatorArgs,
) -> String {
    let description = args
        .description
        .as_deref()
        .map(inline)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            "Review-first Operator draft generated by self-evolution creator.".to_string()
        });
    format!(
        r#"# REVIEW DRAFT ONLY — not loaded until promoted into a real plugin operator.yaml.
apiVersion: omiga.ai/operator/v1alpha1
kind: Operator
metadata:
  id: {unit_id}
  version: 0.1.0
  name: {title:?}
  description: {description:?}
  tags: [self-evolution-draft, creator-draft]
interface:
  inputs: {{}}
  params:
    mode:
      kind: enum
      enum: [offline_fixture, live]
      default: offline_fixture
      description: Review gate: keep offline_fixture deterministic before enabling live mode.
    fixture_json:
      kind: string
      default: ./examples/fixture.json
      description: Deterministic fixture file for smoke validation.
  outputs:
    outputs_json:
      kind: file
      glob: outputs.json
      required: true
      description: Structured draft output summary.
runtime:
  placement:
    supported: [local, ssh]
  container:
    supported: [none]
cache:
  enabled: true
  policyVersion: self-evolution-draft/v1
  fixtureParam: fixture_json
  modeParam: mode
  offlineMode: offline_fixture
permissions:
  sideEffects: []
execution:
  argv:
    - python3
    - ./scripts/operator.py
    - ${{outdir}}
    - ${{params.mode}}
    - ${{params.fixture_json}}
review:
  createdBy: learning_self_evolution_creator
  sourceRecordIds: {source_records:?}
  targetHint: plugins/<plugin-id>/operators/{unit_id}/operator.yaml
  safetyNote: {SAFETY_NOTE:?}
  todos:
    - Replace placeholder script logic with reviewed implementation.
    - Keep offline_fixture behavior deterministic and covered by tests.
    - Run unit_authoring_validate after promotion into an active plugin path.
"#,
        source_records = &args.source_record_ids
    )
}

fn operator_script_draft(unit_id: &str, title: &str) -> String {
    format!(
        r#"#!/usr/bin/env python3
"""Review-only draft script for Operator `{unit_id}` ({title})."""

import argparse
import json
from pathlib import Path


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("outdir")
    parser.add_argument("mode", nargs="?", default="offline_fixture")
    parser.add_argument("fixture_json", nargs="?", default="./examples/fixture.json")
    args = parser.parse_args()

    fixture = {{}}
    fixture_path = Path(args.fixture_json)
    if args.mode == "offline_fixture" and fixture_path.exists():
        fixture = json.loads(fixture_path.read_text())

    outdir = Path(args.outdir)
    outdir.mkdir(parents=True, exist_ok=True)
    (outdir / "outputs.json").write_text(json.dumps({{
        "status": "draft",
        "operatorId": {unit_id:?},
        "mode": args.mode,
        "fixture": fixture,
        "reviewNote": "Replace this placeholder before promotion."
    }}, indent=2))


if __name__ == "__main__":
    main()
"#
    )
}

fn operator_fixture_draft(unit_id: &str) -> String {
    serde_json::to_string_pretty(&serde_json::json!({
        "status": "draft_fixture",
        "operatorId": unit_id,
        "exampleInput": {},
        "expectedSignals": ["outputs.json is written", "status remains draft until reviewed"],
        "safetyNote": SAFETY_NOTE,
    }))
    .unwrap_or_else(|_| "{}".to_string())
}

fn template_yaml_draft(
    unit_id: &str,
    title: &str,
    args: &LearningSelfEvolutionCreatorArgs,
) -> String {
    let description = args
        .description
        .as_deref()
        .map(inline)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            "Review-first Template draft generated by self-evolution creator.".to_string()
        });
    format!(
        r#"# REVIEW DRAFT ONLY — not loaded until promoted into a real plugin template.yaml.
apiVersion: omiga.ai/unit/v1alpha1
kind: Template
metadata:
  id: {unit_id}
  version: 0.1.0
  name: {title:?}
  description: {description:?}
  tags: [self-evolution-draft, creator-draft]
classification:
  category: draft/self-evolution
  stageInput: [review-input]
  stageOutput: [review-output]
exposure:
  exposeToAgent: false
interface:
  inputs:
    table:
      kind: file
      required: true
      description: Review fixture input; replace with real contract before promotion.
  params:
    title:
      kind: string
      default: {title:?}
      description: Human-facing title for the rendered draft output.
  outputs:
    outputs_json:
      kind: file
      glob: outputs.json
      required: true
      description: Structured draft output summary.
runtime:
  envRef: local
template:
  engine: jinja2
  entry: ./template.sh.j2
execution:
  interpreter: bash
  argv:
    - ${{inputs.table}}
    - ${{outdir}}
    - ${{params.title}}
review:
  createdBy: learning_self_evolution_creator
  sourceRecordIds: {source_records:?}
  targetHint: plugins/<plugin-id>/templates/{unit_id}/template.yaml
  safetyNote: {SAFETY_NOTE:?}
  todos:
    - Replace placeholder rendered script with reviewed workflow logic.
    - Add deterministic fixture execution before promotion.
    - Run unit_authoring_validate after promotion into an active plugin path.
"#,
        source_records = &args.source_record_ids
    )
}

fn template_entry_draft(unit_id: &str, title: &str) -> String {
    format!(
        r#"#!/usr/bin/env bash
set -euo pipefail

# Review-only rendered Template draft for `{unit_id}` ({title}).
# Replace placeholder logic before promotion.

INPUT_TSV="{{{{ input_tsv | default(value='example-input.tsv') }}}}"
INPUT_TSV="${{1:-${{INPUT_TSV}}}}"
OUTDIR="${{2:-.}}"
TITLE="${{3:-{title}}}"

mkdir -p "${{OUTDIR}}"

cat > "${{OUTDIR}}/outputs.json" <<JSON
{{
  "status": "draft",
  "templateId": "{unit_id}",
  "inputTsv": "${{INPUT_TSV}}",
  "title": "${{TITLE}}",
  "reviewNote": "Replace this placeholder before promotion."
}}
JSON
"#
    )
}

fn template_example_input_draft() -> String {
    "sample\tvalue\nexample\t1\n".to_string()
}

fn safe_slug(value: &str) -> String {
    let slug = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() {
        "draft".to_string()
    } else {
        slug.chars().take(80).collect()
    }
}

fn inline(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn creator_skill_markdown() -> &'static str {
    r#"---
name: self-evolution-unit-creator
description: Create review-first Operator and Template drafts from self-evolution evidence without applying or registering them.
when_to_use: Use when a self-evolution report candidate or draft should become a higher-quality Operator or Template draft for human review.
tags: [self-evolution, creator, operator, template, unit-authoring]
allowed-tools: [learning_self_evolution_creator, learning_self_evolution_report, learning_self_evolution_draft_write, unit_list, unit_search, unit_describe, unit_authoring_validate, file_read, file_write, file_edit, ripgrep, glob]
---

# Self-Evolution Unit Creator

You are the dedicated creator for turning self-evolution evidence into
review-first **Operator** and **Template** draft artifacts.

## Hard boundaries

- Default output is inert draft material under
  `.omiga/learning/self-evolution-drafts/` or review-only notes beside an
  existing draft.
- Do **not** register units, update defaults, mutate archives, edit plugin
  registries, or silently overwrite active Operator/Template files.
- If an active target write is requested, use the separate promotion flow:
  preview → saved artifact → readiness/request gate → explicit apply.
- Preserve explicit user parameters above learned defaults.
- Every draft needs deterministic validation guidance before promotion.

## Inputs to inspect

Start from any of:

1. a candidate id from `learning_self_evolution_report`;
2. a draft directory from `learning_self_evolution_draft_write`;
3. ExecutionRecord evidence named in `candidate.json`;
4. an explicit user request for an Operator or Template candidate.

If no draft exists yet, run `learning_self_evolution_draft_write` with a narrow
`candidateKinds` filter (`operator_candidate` or `template_candidate`) before
authoring additional files.

For a new manual seed, call `learning_self_evolution_creator` with
`unitKind=operator` or `unitKind=template` plus `unitId`, `title`, and any
`sourceRecordIds`. That creates an inert draft package with candidate metadata,
manifest draft, and deterministic fixture/script placeholders.

## Choose the right unit kind

Create an **Operator** draft when the reusable behavior is atomic:

- one command/script or API wrapper;
- stable input/output contract;
- deterministic offline fixture or smoke path;
- suitable for `operator__*` exposure after promotion.

Create a **Template** draft when the reusable behavior is workflow-shaped:

- orchestrates one or more operators or rendered scripts;
- captures analysis stages, presets, or output bundles;
- suitable for `template_execute` after promotion;
- keeps implementation editable in a template source file such as
  `template.R.j2`, `template.py.j2`, or `template.sh.j2`.

## Operator draft checklist

When refining or creating `operator.yaml.draft`, include:

- `apiVersion`, `kind: Operator`, stable `metadata.id`, name, description,
  version, and tags;
- runtime command/interpreter and argv shape;
- parameter schema with defaults marked as learned/reviewed rather than
  forced;
- input/output contract and artifact names;
- deterministic `mode=offline_fixture` or equivalent smoke path;
- source ExecutionRecord ids and provenance references in a `review` block;
- TODOs for any missing implementation script or fixture.

Prefer adding companion review-only files such as `operator-script.py.draft` or
`fixture.json.draft` instead of writing active plugin files directly.

## Template draft checklist

When refining or creating `template.yaml.draft`, include:

- `apiVersion`, `kind: Template`, stable `metadata.id`, name, description,
  version, and tags;
- `classification` stage input/output metadata;
- template engine and entry file name;
- render/execution mode and migration target when parity still depends on an
  existing Operator;
- parameter defaults and provenance of learned choices;
- outputs, run artifacts, and rerun instructions;
- source ExecutionRecord ids and review safety notes.

Prefer adding companion review-only files such as `template.R.j2.draft`,
`example.tsv.draft`, or `README.review.md` instead of writing active template
files directly.

## Validation gate before promotion

Before recommending promotion, collect:

1. candidate id and draft directory;
2. proposed project-relative target path;
3. deterministic tests/fixtures to run after apply;
4. `unit_authoring_validate` expectation for the target kind;
5. known gaps and rejected alternatives;
6. confirmation that promotion will use the saved review artifact flow.

## Response contract

End with:

- `candidateKind`: `operator_candidate` or `template_candidate`;
- `draftFiles`: paths created or refined;
- `proposedTarget`: project-relative path or `none`;
- `validation`: commands/checks to run;
- `promotionGate`: whether the existing promotion flow is ready or blocked;
- `safety`: confirm no active unit/default/archive mutation occurred.
"#
}

fn project_relative_path(project_root: &Path, path: &Path) -> String {
    path.strip_prefix(project_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::tools::{ToolContext, ToolImpl};
    use futures::StreamExt;
    use serde_json::Value as JsonValue;

    #[tokio::test]
    async fn creates_dedicated_creator_skill_for_operator_and_template_drafts() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let value = execute_to_json(
            &ToolContext::new(tmp.path()),
            LearningSelfEvolutionCreatorArgs {
                refresh: false,
                ..Default::default()
            },
        )
        .await;

        assert_eq!(value["status"], "created");
        assert_eq!(value["skillName"], CREATOR_SKILL_NAME);
        assert_eq!(value["skillDiscovered"], true);
        assert_eq!(value["unitIndexKind"], "skill");
        assert_eq!(
            value["creatorScope"],
            serde_json::json!(["operator", "template"])
        );
        let skill_path = tmp.path().join(value["skillPath"].as_str().unwrap());
        let skill = std::fs::read_to_string(skill_path).expect("skill");
        assert!(skill.contains("operator_candidate"));
        assert!(skill.contains("template_candidate"));
        assert!(skill.contains("promotion flow"));

        let skills = crate::domain::skills::load_skills_for_project(tmp.path()).await;
        assert!(skills.iter().any(|skill| skill.name == CREATOR_SKILL_NAME));
        let units = crate::domain::unit_index::build_unit_index(&skills);
        assert!(units.iter().any(|unit| {
            unit.id == CREATOR_SKILL_NAME && unit.kind == crate::domain::unit_index::UnitKind::Skill
        }));
    }

    #[tokio::test]
    async fn is_idempotent_unless_refresh_is_requested() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path());
        let first = execute_to_json(
            &ctx,
            LearningSelfEvolutionCreatorArgs {
                refresh: false,
                ..Default::default()
            },
        )
        .await;
        assert_eq!(first["status"], "created");
        let skill_path = tmp.path().join(first["skillPath"].as_str().unwrap());
        std::fs::write(&skill_path, "custom creator skill").expect("customize");

        let second = execute_to_json(
            &ctx,
            LearningSelfEvolutionCreatorArgs {
                refresh: false,
                ..Default::default()
            },
        )
        .await;
        assert_eq!(second["status"], "already_exists");
        assert_eq!(
            std::fs::read_to_string(&skill_path).expect("custom remains"),
            "custom creator skill"
        );

        let refreshed = execute_to_json(
            &ctx,
            LearningSelfEvolutionCreatorArgs {
                refresh: true,
                ..Default::default()
            },
        )
        .await;
        assert_eq!(refreshed["status"], "refreshed");
        assert!(std::fs::read_to_string(&skill_path)
            .expect("refreshed skill")
            .contains("Self-Evolution Unit Creator"));
    }

    #[tokio::test]
    async fn creates_review_only_operator_draft_package_when_requested() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let value = execute_to_json(
            &ToolContext::new(tmp.path()),
            LearningSelfEvolutionCreatorArgs {
                refresh: false,
                unit_kind: Some("operator".to_string()),
                unit_id: Some("Reusable Search".to_string()),
                title: Some("Reusable Search Operator".to_string()),
                description: Some("Wrap repeated search behavior.".to_string()),
                source_record_ids: vec!["execrec_operator_1".to_string()],
                review_note: Some("phase creator".to_string()),
            },
        )
        .await;

        assert_eq!(value["status"], "created");
        let package = &value["draftPackage"];
        assert_eq!(package["status"], "draft_package_created");
        assert_eq!(package["kind"], "operator_candidate");
        assert_eq!(package["unitId"], "reusable-search");
        assert!(package["proposedTargetHint"]
            .as_str()
            .unwrap()
            .ends_with("operators/reusable-search/operator.yaml"));
        let draft_dir = tmp.path().join(package["draftDir"].as_str().unwrap());
        assert!(draft_dir.join("DRAFT.md").exists());
        assert!(draft_dir.join("candidate.json").exists());
        assert!(draft_dir.join("operator.yaml.draft").exists());
        assert!(draft_dir.join("operator.py.draft").exists());
        assert!(draft_dir.join("fixture.json.draft").exists());
        let manifest =
            std::fs::read_to_string(draft_dir.join("operator.yaml.draft")).expect("operator draft");
        assert!(manifest.contains("kind: Operator"));
        assert!(manifest.contains("interface:"));
        assert!(manifest.contains("execution:"));
        assert!(manifest.contains("${params.mode}"));
        assert!(manifest.contains("default: offline_fixture"));
        assert!(manifest.contains("execrec_operator_1"));
        let candidate =
            std::fs::read_to_string(draft_dir.join("candidate.json")).expect("candidate");
        assert!(candidate.contains("\"operator_candidate\""));
        assert!(candidate.contains("learning_self_evolution_creator"));
    }

    #[tokio::test]
    async fn creates_review_only_template_draft_package_when_requested() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let value = execute_to_json(
            &ToolContext::new(tmp.path()),
            LearningSelfEvolutionCreatorArgs {
                refresh: false,
                unit_kind: Some("template_candidate".to_string()),
                unit_id: Some("Bulk DE Workflow".to_string()),
                title: Some("Bulk DE Workflow Template".to_string()),
                description: Some("Capture repeated DE workflow choices.".to_string()),
                source_record_ids: vec!["execrec_template_1".to_string()],
                review_note: None,
            },
        )
        .await;

        let package = &value["draftPackage"];
        assert_eq!(package["status"], "draft_package_created");
        assert_eq!(package["kind"], "template_candidate");
        assert_eq!(package["unitId"], "bulk-de-workflow");
        assert!(package["proposedTargetHint"]
            .as_str()
            .unwrap()
            .ends_with("templates/bulk-de-workflow/template.yaml"));
        let draft_dir = tmp.path().join(package["draftDir"].as_str().unwrap());
        assert!(draft_dir.join("template.yaml.draft").exists());
        assert!(draft_dir.join("template.sh.j2.draft").exists());
        assert!(draft_dir.join("example-input.tsv.draft").exists());
        let manifest =
            std::fs::read_to_string(draft_dir.join("template.yaml.draft")).expect("template draft");
        assert!(manifest.contains("kind: Template"));
        assert!(manifest.contains("apiVersion: omiga.ai/unit/v1alpha1"));
        assert!(manifest.contains("interface:"));
        assert!(manifest.contains("execution:"));
        assert!(manifest.contains("${inputs.table}"));
        assert!(manifest.contains("template.sh.j2"));
        assert!(manifest.contains("execrec_template_1"));
        let entry = std::fs::read_to_string(draft_dir.join("template.sh.j2.draft")).expect("entry");
        assert!(entry.contains("outputs.json"));
        assert!(entry.contains("bulk-de-workflow"));
    }

    async fn execute_to_json(
        ctx: &ToolContext,
        args: LearningSelfEvolutionCreatorArgs,
    ) -> JsonValue {
        let mut stream = LearningSelfEvolutionCreatorTool::execute(ctx, args)
            .await
            .expect("execute creator");
        while let Some(item) = stream.next().await {
            if let StreamOutputItem::Text(text) = item {
                return serde_json::from_str(&text).expect("json");
            }
        }
        panic!("learning_self_evolution_creator did not return text output");
    }
}
