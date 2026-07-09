//! Atomic multi-file apply_patch tool.

mod apply;
mod parser;

use super::{ToolContext, ToolError, ToolSchema};
use crate::infrastructure::streaming::{StreamOutput, StreamOutputItem};
use apply::{apply_patch_atomic, ChangeKind, ChangeSummary};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::pin::Pin;

pub const DESCRIPTION: &str = r#"Apply a Codex-style multi-file patch atomically.

The `patch` argument must be one string with this format:
*** Begin Patch
*** Add File: path
+new line
*** Update File: path
@@ optional unique context line
 context line
-old line
+new line
*** Delete File: path
*** End Patch

All target files are parsed and matched before anything is written. If any Add/Update/Delete hunk fails, no file is changed. Update context matching accepts unique matches with exact text, trailing-whitespace differences, or leading/trailing whitespace drift; ambiguous or missing context fails."#;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyPatchArgs {
    pub patch: String,
}

pub struct ApplyPatchTool;

#[async_trait]
impl super::ToolImpl for ApplyPatchTool {
    type Args = ApplyPatchArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        if ctx.execution_environment != "local" {
            return Err(ToolError::ExecutionFailed {
                message: format!(
                    "apply_patch currently supports local filesystem execution only, got '{}'",
                    ctx.execution_environment
                ),
            });
        }

        let patch = parser::parse_patch(&args.patch).map_err(|e| ToolError::InvalidArguments {
            message: format!("Invalid apply_patch patch: {e}"),
        })?;
        let summaries = apply_patch_atomic(&ctx.project_root, &ctx.cwd, patch).await?;
        Ok(ApplyPatchOutput { summaries }.into_stream())
    }
}

#[derive(Debug, Clone)]
pub struct ApplyPatchOutput {
    summaries: Vec<ChangeSummary>,
}

impl StreamOutput for ApplyPatchOutput {
    fn into_stream(self) -> Pin<Box<dyn futures::Stream<Item = StreamOutputItem> + Send>> {
        use futures::stream;

        let mut text = String::from("Patch applied successfully.\n");
        for summary in &self.summaries {
            let kind = match summary.kind {
                ChangeKind::Add => "A",
                ChangeKind::Modify => "M",
                ChangeKind::Delete => "D",
            };
            text.push_str(&format!(
                "{} {} ({} -> {} lines)\n",
                kind, summary.path, summary.old_lines, summary.new_lines
            ));
        }

        let items = vec![
            StreamOutputItem::Metadata {
                key: "files_changed".to_string(),
                value: self.summaries.len().to_string(),
            },
            StreamOutputItem::Start,
            StreamOutputItem::Content(text),
            StreamOutputItem::Complete,
        ];
        Box::pin(stream::iter(items))
    }
}

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "apply_patch",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "patch": {
                    "type": "string",
                    "description": "Codex-style patch text wrapped by *** Begin Patch and *** End Patch. Supports *** Add File, *** Update File with @@ hunks using space/-/+ lines, and *** Delete File."
                }
            },
            "required": ["patch"]
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::tools::{ToolContext, ToolImpl};
    use std::fs;

    fn wrap(body: &str) -> String {
        format!("*** Begin Patch\n{body}\n*** End Patch")
    }

    async fn run(root: &std::path::Path, body: &str) -> Result<Vec<ChangeSummary>, ToolError> {
        let patch = parser::parse_patch(&wrap(body)).unwrap();
        apply_patch_atomic(root, root, patch).await
    }

    #[tokio::test]
    async fn apply_patch_add_new_file() {
        let tmp = tempfile::tempdir().unwrap();
        run(tmp.path(), "*** Add File: a.txt\n+one\n+two")
            .await
            .unwrap();
        assert_eq!(
            fs::read_to_string(tmp.path().join("a.txt")).unwrap(),
            "one\ntwo\n"
        );
    }

    #[tokio::test]
    async fn apply_patch_update_single_hunk_hits() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("a.txt"), "one\ntwo\nthree\n").unwrap();
        run(
            tmp.path(),
            "*** Update File: a.txt\n@@\n one\n-two\n+TWO\n three",
        )
        .await
        .unwrap();
        assert_eq!(
            fs::read_to_string(tmp.path().join("a.txt")).unwrap(),
            "one\nTWO\nthree\n"
        );
    }

    #[tokio::test]
    async fn apply_patch_update_allows_context_whitespace_drift() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(
            tmp.path().join("a.txt"),
            "fn main() {\n    value();   \n}\n",
        )
        .unwrap();
        run(
            tmp.path(),
            "*** Update File: a.txt\n@@\n fn main() {\n value();\n-}\n+    done();\n+}",
        )
        .await
        .unwrap();
        assert_eq!(
            fs::read_to_string(tmp.path().join("a.txt")).unwrap(),
            "fn main() {\n    value();   \n    done();\n}\n"
        );
    }

    #[tokio::test]
    async fn apply_patch_delete_file() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("a.txt"), "gone\n").unwrap();
        run(tmp.path(), "*** Delete File: a.txt").await.unwrap();
        assert!(!tmp.path().join("a.txt").exists());
    }

    #[tokio::test]
    async fn apply_patch_multi_file_success() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("a.txt"), "old\n").unwrap();
        fs::write(tmp.path().join("delete.txt"), "bye\n").unwrap();
        run(
            tmp.path(),
            "*** Add File: add.txt\n+new\n*** Update File: a.txt\n@@\n-old\n+updated\n*** Delete File: delete.txt",
        )
        .await
        .unwrap();
        assert_eq!(
            fs::read_to_string(tmp.path().join("add.txt")).unwrap(),
            "new\n"
        );
        assert_eq!(
            fs::read_to_string(tmp.path().join("a.txt")).unwrap(),
            "updated\n"
        );
        assert!(!tmp.path().join("delete.txt").exists());
    }

    #[tokio::test]
    async fn apply_patch_failure_rolls_back_all_files() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("existing.txt"), "keep\n").unwrap();
        let err = run(
            tmp.path(),
            "*** Add File: new.txt\n+created\n*** Update File: existing.txt\n@@\n-missing\n+changed",
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("expected context not found"));
        assert!(!tmp.path().join("new.txt").exists());
        assert_eq!(
            fs::read_to_string(tmp.path().join("existing.txt")).unwrap(),
            "keep\n"
        );
    }

    #[tokio::test]
    async fn apply_patch_rejects_path_traversal() {
        let tmp = tempfile::tempdir().unwrap();
        let err = run(tmp.path(), "*** Add File: ../escape.txt\n+nope")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("Path traversal"));
        assert!(!tmp.path().parent().unwrap().join("escape.txt").exists());
    }

    #[tokio::test]
    async fn apply_patch_tool_from_context_smoke() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = ToolContext::new(tmp.path());
        let _stream = ApplyPatchTool::execute(
            &ctx,
            ApplyPatchArgs {
                patch: wrap("*** Add File: a.txt\n+ok"),
            },
        )
        .await
        .unwrap();
        assert_eq!(
            fs::read_to_string(tmp.path().join("a.txt")).unwrap(),
            "ok\n"
        );
    }
}
