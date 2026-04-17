//! Omiga visualization tool - generates omiga:viz markdown blocks for the frontend.

use super::{ToolContext, ToolError, ToolSchema};
use crate::infrastructure::streaming::{StreamOutput, StreamOutputItem};
use serde::{Deserialize, Serialize};
use std::pin::Pin;

pub const DESCRIPTION: &str = r#"Generate an omiga:viz interactive visualization block.

Use this tool when you need to present data visually (charts, graphs, flowcharts, protein structures, maps, 3D scenes, formulas, or custom HTML).
Supported viz_type values:
- "echarts": Standard charts (bar, line, pie, scatter, etc.). Config should be an ECharts option object under `config.option`.
- "plotly": Scientific/3D plots. Config should be Plotly data+layout under `config.data` and `config.layout`.
- "mermaid": Flowcharts, sequence diagrams, gantt charts. Config should have `config.source` as the mermaid syntax string.
- "graphviz": Graphviz/DOT directed graphs. Config should have `config.dot` as the DOT source string.
- "pdb": Protein structure viewer (Mol*). Config should have `config.url` pointing to a PDB file URL.
- "three": Three.js 3D scene. Config should have `config.code` as JavaScript code string using the global `THREE` object.
- "map": Leaflet map with markers and optional GeoJSON. Config should have `config.config` with `center`, `zoom`, `markers`, and optionally `geojson`.
- "katex": Large math formula card. Config should have `config.source` as the LaTeX string, and optionally `config.displayMode` (default true).
- "iframe": Embed an external URL. Config should have `config.url`.
- "html": Render custom HTML in a sandboxed iframe. Config should have `config.html`.

The tool returns a markdown code block that the Omiga frontend will render interactively.
"#;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OmigaVizArgs {
    /// Visualization type: echarts, plotly, mermaid, pdb, iframe, html
    pub viz_type: String,
    /// Human-readable title/caption for the visualization
    pub title: Option<String>,
    /// Description or context about what this visualization shows
    pub description: Option<String>,
    /// The visualization configuration object (type-specific)
    pub config: serde_json::Value,
}

pub struct OmigaVizTool;

#[async_trait::async_trait]
impl super::ToolImpl for OmigaVizTool {
    type Args = OmigaVizArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        _ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<Pin<Box<dyn futures::Stream<Item = StreamOutputItem> + Send>>, ToolError> {
        let mut config = args.config.clone();
        match config {
            serde_json::Value::Object(ref mut map) => {
                map.insert(
                    "type".to_string(),
                    serde_json::Value::String(args.viz_type.clone()),
                );
            }
            _ => {
                return Err(ToolError::InvalidArguments {
                    message: format!(
                        "omiga_viz: `config` must be a JSON object, got {}",
                        config.to_string().chars().take(80).collect::<String>()
                    ),
                });
            }
        }

        let mut parts = Vec::new();
        if let Some(title) = args.title {
            parts.push(format!("**{}**", title));
        }
        if let Some(desc) = args.description {
            parts.push(desc);
        }
        parts.push(format!(
            "```omiga:viz\n{}\n```",
            serde_json::to_string_pretty(&config).unwrap_or_else(|_| "{}".to_string())
        ));

        let output = parts.join("\n\n");
        Ok(OmigaVizOutput { content: output }.into_stream())
    }
}

pub struct OmigaVizOutput {
    pub content: String,
}

impl StreamOutput for OmigaVizOutput {
    fn into_stream(self) -> Pin<Box<dyn futures::Stream<Item = StreamOutputItem> + Send>> {
        use futures::stream;
        Box::pin(stream::iter(vec![
            StreamOutputItem::Start,
            StreamOutputItem::Content(self.content),
            StreamOutputItem::Complete,
        ]))
    }
}

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "omiga_viz",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "viz_type": {
                    "type": "string",
                    "enum": ["echarts", "plotly", "mermaid", "graphviz", "pdb", "three", "map", "katex", "iframe", "html"],
                    "description": "The visualization type"
                },
                "title": {
                    "type": "string",
                    "description": "Optional title/caption"
                },
                "description": {
                    "type": "string",
                    "description": "Optional description of what the visualization shows"
                },
                "config": {
                    "type": "object",
                    "description": "Type-specific configuration object"
                }
            },
            "required": ["viz_type", "config"]
        }),
    )
}
