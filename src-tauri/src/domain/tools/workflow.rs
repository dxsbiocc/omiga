//! Workflow tool — parity with `WORKFLOW_TOOL_NAME` / `src/tools/WorkflowTool/constants.ts`.
//!
//! In Claude Code, workflows are gated by `feature('WORKFLOW_SCRIPTS')`. Omiga mirrors that with
//! [`crate::domain::agents::subagent_tool_filter::env_workflow_scripts_enabled`].
//!
//! Full workflow execution uses the bundled TS runtime (not shipped in this repo). This
//! implementation discovers workflow files under `.claude/workflows` or `workflows` and returns
//! a structured summary so the model can follow steps manually or via other tools.

use super::{ToolContext, ToolError, ToolImpl, ToolSchema};
use crate::infrastructure::streaming::{StreamOutput, StreamOutputItem};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::Path;
use std::pin::Pin;

/// Wire name (`src/tools/WorkflowTool/constants.ts`).
pub const WORKFLOW_TOOL_NAME: &str = "workflow";

pub const DESCRIPTION: &str = "Run or inspect a named workflow definition from the project.\n\
\n\
Workflows are optional markdown/YAML files under `.claude/workflows/` or `workflows/`.\n\
Omiga lists matches and previews the file; it does not run the full Claude Code workflow engine.\n\
Use this to coordinate multi-step work — then use `bash`, `file_read`, or `Agent` as needed.\n\
\n\
Enable the tool by setting `OMIGA_WORKFLOW_SCRIPTS=1` (or `WORKFLOW_SCRIPTS=1`), matching TS `WORKFLOW_SCRIPTS`.";

/// Arguments for the workflow tool (flexible field names for TS parity).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowArgs {
    /// Workflow name (file stem) or path fragment to match.
    #[serde(alias = "workflow", alias = "workflow_id")]
    pub name: String,
    #[serde(default)]
    pub input: Option<serde_json::Value>,
}

pub struct WorkflowTool;

#[async_trait]
impl ToolImpl for WorkflowTool {
    type Args = WorkflowArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        let name = args.name.trim();
        if name.is_empty() {
            return Err(ToolError::InvalidArguments {
                message: "`name` (or `workflow`) is required".to_string(),
            });
        }

        let discovered = discover_workflow_files(&ctx.project_root);
        let mut lines: Vec<String> = Vec::new();
        lines.push(format!("Workflow request: `{name}`"));
        if let Some(input) = args.input.as_ref() {
            lines.push(format!("Input: {}", input));
        }
        lines.push(String::new());
        if discovered.is_empty() {
            lines.push(
                "No workflow files found under `.claude/workflows/` or `workflows/` (`.md`, `.yaml`, `.yml`)."
                    .to_string(),
            );
        } else {
            lines.push("Discovered workflows:".to_string());
            for w in &discovered {
                lines.push(format!("- {} — {}", w.stem, w.path.display()));
            }
            if let Some(hit) = discovered.iter().find(|w| w.stem == name) {
                lines.push(String::new());
                lines.push(format!("--- Preview: {} ---", hit.path.display()));
                match std::fs::read_to_string(&hit.path) {
                    Ok(body) => {
                        let preview = truncate_chars(&body, 4000);
                        lines.push(preview);
                    }
                    Err(e) => lines.push(format!("(Could not read file: {e})")),
                }
            } else {
                lines.push(String::new());
                lines.push(format!(
                    "No exact file-stem match for `{name}`. Use one of the names above."
                ));
            }
        }

        let text = lines.join("\n");
        Ok(WorkflowOutput { text }.into_stream())
    }
}

struct WorkflowFile {
    stem: String,
    path: std::path::PathBuf,
}

fn discover_workflow_files(project_root: &Path) -> Vec<WorkflowFile> {
    let mut set: BTreeSet<String> = BTreeSet::new();
    let mut out: Vec<WorkflowFile> = Vec::new();
    for rel in [".claude/workflows", "workflows"] {
        let dir = project_root.join(rel);
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for ent in rd.flatten() {
            let path = ent.path();
            let ext = path.extension().and_then(|e| e.to_str());
            if !matches!(ext, Some("md" | "yaml" | "yml")) {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if set.insert(stem.to_string()) {
                out.push(WorkflowFile {
                    stem: stem.to_string(),
                    path,
                });
            }
        }
    }
    out.sort_by(|a, b| a.stem.cmp(&b.stem));
    out
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let prefix: String = s.chars().take(max).collect();
    format!("{prefix}\n\n[Truncated… {} total chars]", s.chars().count())
}

pub struct WorkflowOutput {
    pub text: String,
}

impl StreamOutput for WorkflowOutput {
    fn into_stream(self) -> Pin<Box<dyn futures::Stream<Item = StreamOutputItem> + Send>> {
        use futures::stream;
        Box::pin(stream::iter(vec![
            StreamOutputItem::Start,
            StreamOutputItem::Content(self.text),
            StreamOutputItem::Complete,
        ]))
    }
}

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        WORKFLOW_TOOL_NAME,
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Workflow name (file stem) or relative path to match under .claude/workflows or workflows."
                },
                "workflow": {
                    "type": "string",
                    "description": "Alias for `name` (Claude Code parity)."
                },
                "input": {
                    "description": "Optional structured input for the workflow (recorded in the output; full execution is not implemented)."
                }
            },
            "required": ["name"]
        }),
    )
}
