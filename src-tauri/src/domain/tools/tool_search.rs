//! Search deferred / available tools — aligned with `ToolSearchTool` (wire name `ToolSearch`).
//!
//! Keyword search over `all_tool_schemas` names and descriptions; supports `select:ToolName` prefix.

use super::{all_tool_schemas, ToolContext, ToolError, ToolSchema};
use crate::infrastructure::streaming::{StreamOutput, StreamOutputItem};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::pin::Pin;

pub const DESCRIPTION: &str = r#"Find tools by keyword or direct selection. Use `select:ToolName` (e.g. `select:bash`) to pick one tool by exact name. Otherwise provide keywords matched against tool names and descriptions.

`max_results` defaults to 5 (capped at 25)."#;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSearchArgs {
    pub query: String,
    #[serde(default = "default_max_results")]
    pub max_results: usize,
}

fn default_max_results() -> usize {
    5
}

pub struct ToolSearchTool;

fn normalize(s: &str) -> String {
    s.to_lowercase()
}

#[async_trait]
impl super::ToolImpl for ToolSearchTool {
    type Args = ToolSearchArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        _ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        let q = args.query.trim();
        if q.is_empty() {
            return Err(ToolError::InvalidArguments {
                message: "`query` must be non-empty.".to_string(),
            });
        }

        let max = args.max_results.clamp(1, 25);
        let schemas = all_tool_schemas(true);

        let matches: Vec<String> = if q.to_lowercase().starts_with("select:") {
            let name = q["select:".len()..].trim();
            schemas
                .iter()
                .find(|s| s.name.eq_ignore_ascii_case(name))
                .map(|s| vec![s.name.clone()])
                .unwrap_or_default()
        } else {
            let terms: Vec<String> = normalize(q)
                .split_whitespace()
                .map(String::from)
                .filter(|t| !t.is_empty())
                .collect();

            let mut scored: Vec<(i32, String)> = Vec::new();
            for s in &schemas {
                let n = normalize(&s.name);
                let d = normalize(&s.description);
                let mut score: i32 = 0;
                for t in &terms {
                    if n.contains(t) {
                        score += 3;
                    }
                    if d.contains(t) {
                        score += 1;
                    }
                }
                if score > 0 {
                    scored.push((score, s.name.clone()));
                }
            }
            scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
            scored
                .into_iter()
                .map(|(_, name)| name)
                .take(max)
                .collect()
        };

        #[derive(Serialize)]
        struct Out<'a> {
            matches: Vec<String>,
            query: &'a str,
            total_deferred_tools: usize,
        }

        let out = Out {
            matches,
            query: q,
            total_deferred_tools: schemas.len(),
        };
        let text = serde_json::to_string_pretty(&out).map_err(|e| ToolError::ExecutionFailed {
            message: e.to_string(),
        })?;
        Ok(ToolSearchOutput { text }.into_stream())
    }
}

struct ToolSearchOutput {
    text: String,
}

impl StreamOutput for ToolSearchOutput {
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
        "ToolSearch",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Keywords or select:ToolName" },
                "max_results": { "type": "number", "description": "Max matches (default 5)" }
            },
            "required": ["query"]
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::tools::ToolImpl;

    #[tokio::test]
    async fn select_bash_finds_bash() {
        let ctx = ToolContext::new("/tmp");
        let args = ToolSearchArgs {
            query: "select:bash".to_string(),
            max_results: 5,
        };
        let mut stream = ToolSearchTool::execute(&ctx, args).await.unwrap();
        use futures::StreamExt;
        let mut buf = String::new();
        while let Some(i) = stream.next().await {
            if let StreamOutputItem::Content(c) = i {
                buf.push_str(&c);
            }
        }
        assert!(buf.contains("bash"));
    }
}
