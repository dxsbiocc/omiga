use super::{ToolContext, ToolError, ToolImpl, ToolSchema};
use crate::domain::execution_records::ExecutionRecord;
use crate::infrastructure::streaming::{stream_single, StreamOutputItem};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

pub const DESCRIPTION: &str =
    "Read one project-scoped Operator/Template ExecutionRecord by id with optional parsed JSON and child records.";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionRecordDetailArgs {
    #[serde(rename = "recordId")]
    pub record_id: String,
    #[serde(default = "default_true", rename = "includeChildren")]
    pub include_children: bool,
    #[serde(default = "default_true", rename = "includeParsedJson")]
    pub include_parsed_json: bool,
}

impl Default for ExecutionRecordDetailArgs {
    fn default() -> Self {
        Self {
            record_id: String::new(),
            include_children: default_true(),
            include_parsed_json: default_true(),
        }
    }
}

pub struct ExecutionRecordDetailTool;

#[async_trait]
impl ToolImpl for ExecutionRecordDetailTool {
    type Args = ExecutionRecordDetailArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        let record_id = args.record_id.trim();
        if record_id.is_empty() {
            return Err(ToolError::InvalidArguments {
                message: "recordId must not be empty".to_string(),
            });
        }
        let record =
            crate::domain::execution_records::get_execution_record(&ctx.project_root, record_id)
                .await
                .map_err(|message| ToolError::ExecutionFailed { message })?;
        let children = if args.include_children {
            crate::domain::execution_records::list_child_execution_records(
                &ctx.project_root,
                record_id,
                200,
            )
            .await
            .map_err(|message| ToolError::ExecutionFailed { message })?
        } else {
            Vec::new()
        };

        let output = if let Some(record) = record {
            let parsed = if args.include_parsed_json {
                parsed_record_json(&record)
            } else {
                JsonValue::Null
            };
            let parent_execution_id = record.parent_execution_id.clone();
            let child_count = children.len();
            let parsed_children = if args.include_parsed_json && args.include_children {
                serde_json::json!(children
                    .iter()
                    .map(|child| serde_json::json!({
                        "id": child.id,
                        "parsed": parsed_record_json(child),
                    }))
                    .collect::<Vec<_>>())
            } else {
                JsonValue::Null
            };
            serde_json::json!({
                "database": crate::domain::execution_records::execution_db_path(&ctx.project_root),
                "recordId": record_id,
                "found": true,
                "record": record,
                "parsed": parsed,
                "children": children,
                "parsedChildren": parsed_children,
                "lineage": {
                    "parentExecutionId": parent_execution_id,
                    "childCount": child_count,
                },
                "note": "Read-only ExecutionRecord detail. This tool does not mutate runs, artifacts, proposals, or archives."
            })
        } else {
            serde_json::json!({
                "database": crate::domain::execution_records::execution_db_path(&ctx.project_root),
                "recordId": record_id,
                "found": false,
                "record": JsonValue::Null,
                "parsed": JsonValue::Null,
                "children": [],
                "parsedChildren": JsonValue::Null,
                "lineage": {
                    "parentExecutionId": JsonValue::Null,
                    "childCount": 0,
                },
                "note": "ExecutionRecord not found in this project store."
            })
        };

        Ok(stream_single(StreamOutputItem::Text(
            serde_json::to_string_pretty(&output).unwrap_or_else(|_| "{}".to_string()),
        )))
    }
}

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "execution_record_detail",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "required": ["recordId"],
            "properties": {
                "recordId": {
                    "type": "string",
                    "description": "ExecutionRecord id, for example execrec_..."
                },
                "includeChildren": {
                    "type": "boolean",
                    "description": "When true, include direct child ExecutionRecords. Defaults to true."
                },
                "includeParsedJson": {
                    "type": "boolean",
                    "description": "When true, parse metadataJson/runtimeJson/outputSummaryJson into a parsed object. Defaults to true."
                }
            }
        }),
    )
}

fn default_true() -> bool {
    true
}

fn parsed_record_json(record: &ExecutionRecord) -> JsonValue {
    serde_json::json!({
        "metadata": parse_json(record.metadata_json.as_deref()),
        "runtime": parse_json(record.runtime_json.as_deref()),
        "outputSummary": parse_json(record.output_summary_json.as_deref()),
    })
}

fn parse_json(raw: Option<&str>) -> JsonValue {
    raw.and_then(|value| serde_json::from_str::<JsonValue>(value).ok())
        .unwrap_or(JsonValue::Null)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::execution_records::{record_execution, ExecutionRecordInput};
    use futures::StreamExt;
    use serde_json::json;

    #[tokio::test]
    async fn reads_record_detail_with_parsed_metadata_and_children() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let parent_id = record_execution(
            tmp.path(),
            ExecutionRecordInput {
                kind: "template".to_string(),
                unit_id: Some("detail_template".to_string()),
                canonical_id: Some("plugin/template/detail_template".to_string()),
                provider_plugin: Some("plugin".to_string()),
                status: "succeeded".to_string(),
                session_id: Some("session-1".to_string()),
                parent_execution_id: None,
                started_at: Some("2026-05-10T00:00:00Z".to_string()),
                ended_at: Some("2026-05-10T00:00:02Z".to_string()),
                input_hash: Some("sha256:input".to_string()),
                param_hash: Some("sha256:param".to_string()),
                output_summary_json: Some(json!({
                    "outputs": [{"path": ".omiga/runs/detail/out/report.md"}]
                })),
                runtime_json: Some(json!({"runDir": ".omiga/runs/detail"})),
                metadata_json: Some(json!({
                    "runDir": ".omiga/runs/detail",
                    "paramSources": {"method": "user_preflight"},
                    "preflight": {"answeredParams": [{"param": "method"}]},
                    "selectedParams": {"method": "limma"}
                })),
            },
        )
        .await
        .expect("parent");
        record_execution(
            tmp.path(),
            ExecutionRecordInput {
                kind: "operator".to_string(),
                unit_id: Some("detail_operator".to_string()),
                canonical_id: Some("plugin/operator/detail_operator".to_string()),
                provider_plugin: Some("plugin".to_string()),
                status: "succeeded".to_string(),
                session_id: Some("session-1".to_string()),
                parent_execution_id: Some(parent_id.clone()),
                started_at: Some("2026-05-10T00:00:01Z".to_string()),
                ended_at: Some("2026-05-10T00:00:02Z".to_string()),
                input_hash: None,
                param_hash: None,
                output_summary_json: None,
                runtime_json: None,
                metadata_json: Some(json!({"runDir": ".omiga/runs/detail-child"})),
            },
        )
        .await
        .expect("child");

        let value = execute_to_json(
            &ToolContext::new(tmp.path()),
            ExecutionRecordDetailArgs {
                record_id: parent_id,
                include_children: true,
                include_parsed_json: true,
            },
        )
        .await;

        assert_eq!(value["found"], true);
        assert_eq!(value["record"]["unitId"], "detail_template");
        assert_eq!(
            value["parsed"]["metadata"]["selectedParams"]["method"],
            "limma"
        );
        assert_eq!(value["lineage"]["childCount"], 1);
        assert_eq!(value["children"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn missing_record_returns_found_false() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let value = execute_to_json(
            &ToolContext::new(tmp.path()),
            ExecutionRecordDetailArgs {
                record_id: "execrec_missing".to_string(),
                include_children: true,
                include_parsed_json: true,
            },
        )
        .await;

        assert_eq!(value["found"], false);
        assert_eq!(value["lineage"]["childCount"], 0);
    }

    async fn execute_to_json(ctx: &ToolContext, args: ExecutionRecordDetailArgs) -> JsonValue {
        let mut stream = ExecutionRecordDetailTool::execute(ctx, args)
            .await
            .expect("execute detail");
        while let Some(item) = stream.next().await {
            if let StreamOutputItem::Text(text) = item {
                return serde_json::from_str(&text).expect("json");
            }
        }
        panic!("execution_record_detail did not return text output");
    }
}
