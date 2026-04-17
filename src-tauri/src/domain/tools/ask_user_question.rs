//! Ask the user multiple-choice questions — aligned with `AskUserQuestionTool` (TypeScript).
//!
//! Interactive UI is wired from the chat path (`commands::chat::execute_ask_user_question_interactive`):
//! the tool blocks until the user submits answers in the Omiga chat UI. The standalone
//! `execute_tool` IPC path still uses immediate stub output when no UI is attached.

use super::{ToolContext, ToolError, ToolSchema};
use crate::infrastructure::streaming::{StreamOutput, StreamOutputItem};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::pin::Pin;

const MAX_QUESTIONS: usize = 4;
const MIN_QUESTIONS: usize = 1;
const MAX_OPTIONS: usize = 4;
const MIN_OPTIONS: usize = 2;
const HEADER_MAX_CHARS: usize = 12;

pub const DESCRIPTION: &str = r#"Ask the user multiple-choice questions to gather information, resolve ambiguity, or offer options.

Provide 1–4 questions; each question has 2–4 options with short labels and longer descriptions. Optional `preview` on an option helps compare concrete artifacts (markdown or HTML fragment per product guidance).

In the Omiga app, the chat UI shows these questions and waits for your selections before the agent continues.

Plan mode: use this for clarifications before planning. Do not use it for plan approval — use the dedicated exit-plan tool when available."#;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionOption {
    pub label: String,
    pub description: String,
    #[serde(default)]
    pub preview: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionItem {
    pub question: String,
    pub header: String,
    pub options: Vec<QuestionOption>,
    #[serde(default, rename = "multiSelect")]
    pub multi_select: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskUserQuestionArgs {
    pub questions: Vec<QuestionItem>,
    #[serde(default)]
    pub answers: Option<serde_json::Value>,
    #[serde(default)]
    pub annotations: Option<serde_json::Value>,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
}

pub struct AskUserQuestionTool;

fn validate(args: &AskUserQuestionArgs) -> Result<(), ToolError> {
    let qs = &args.questions;
    if qs.len() < MIN_QUESTIONS || qs.len() > MAX_QUESTIONS {
        return Err(ToolError::InvalidArguments {
            message: format!(
                "Provide between {} and {} questions.",
                MIN_QUESTIONS, MAX_QUESTIONS
            ),
        });
    }

    let mut seen_q = HashSet::new();
    for q in qs {
        let qt = q.question.trim();
        if qt.is_empty() {
            return Err(ToolError::InvalidArguments {
                message: "Each question must be non-empty.".to_string(),
            });
        }
        if !seen_q.insert(qt.to_string()) {
            return Err(ToolError::InvalidArguments {
                message: "Question texts must be unique.".to_string(),
            });
        }

        let h = q.header.trim();
        if h.is_empty() {
            return Err(ToolError::InvalidArguments {
                message: "Each question needs a non-empty `header` (short chip label).".to_string(),
            });
        }
        if h.chars().count() > HEADER_MAX_CHARS {
            return Err(ToolError::InvalidArguments {
                message: format!(
                    "`header` must be at most {} characters (chip width).",
                    HEADER_MAX_CHARS
                ),
            });
        }

        if q.options.len() < MIN_OPTIONS || q.options.len() > MAX_OPTIONS {
            return Err(ToolError::InvalidArguments {
                message: format!(
                    "Each question needs between {} and {} options.",
                    MIN_OPTIONS, MAX_OPTIONS
                ),
            });
        }

        let mut seen_labels = HashSet::new();
        for o in &q.options {
            let l = o.label.trim();
            let d = o.description.trim();
            if l.is_empty() || d.is_empty() {
                return Err(ToolError::InvalidArguments {
                    message: "Each option needs non-empty `label` and `description`.".to_string(),
                });
            }
            if !seen_labels.insert(l.to_string()) {
                return Err(ToolError::InvalidArguments {
                    message: "Option labels must be unique within a question.".to_string(),
                });
            }
            if q.multi_select && o.preview.is_some() {
                return Err(ToolError::InvalidArguments {
                    message: "`preview` is only supported when multiSelect is false.".to_string(),
                });
            }
        }
    }

    Ok(())
}

/// Exposed for the chat interactive path (same rules as [`validate`]).
pub(crate) fn validate_ask_user_question_args(args: &AskUserQuestionArgs) -> Result<(), ToolError> {
    validate(args)
}

#[async_trait]
impl super::ToolImpl for AskUserQuestionTool {
    type Args = AskUserQuestionArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        _ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        validate(&args)?;

        let mut body = serde_json::Map::new();
        body.insert(
            "questions".to_string(),
            serde_json::to_value(&args.questions).map_err(|e| ToolError::ExecutionFailed {
                message: format!("Failed to serialize questions: {}", e),
            })?,
        );
        body.insert("answers".to_string(), serde_json::json!({}));
        if let Some(a) = args.annotations {
            body.insert("annotations".to_string(), a);
        }
        body.insert(
            "_omiga".to_string(),
            serde_json::Value::String(
                "Interactive multiple-choice UI is not wired in Omiga yet. Continue by asking the user in plain text, or use their next chat message as the answer."
                    .to_string(),
            ),
        );

        let json = serde_json::to_string_pretty(&serde_json::Value::Object(body)).map_err(|e| {
            ToolError::ExecutionFailed {
                message: format!("Failed to serialize result: {}", e),
            }
        })?;

        let preamble = "User has not answered yet (Omiga interactive picker is not connected). Treat the JSON below as the question set; ask the user in natural language or wait for their reply.\n\n";

        Ok(AskUserQuestionOutput {
            text: format!("{}{}", preamble, json),
        }
        .into_stream())
    }
}

struct AskUserQuestionOutput {
    text: String,
}

impl StreamOutput for AskUserQuestionOutput {
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
        "ask_user_question",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "questions": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": 4,
                    "items": {
                        "type": "object",
                        "properties": {
                            "question": { "type": "string", "description": "Full question ending with ?" },
                            "header": { "type": "string", "description": "Short chip label (max 12 chars)" },
                            "multiSelect": { "type": "boolean", "description": "Allow multiple selections" },
                            "options": {
                                "type": "array",
                                "minItems": 2,
                                "maxItems": 4,
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "label": { "type": "string" },
                                        "description": { "type": "string" },
                                        "preview": { "type": "string", "description": "Optional focused preview (single-select only)" }
                                    },
                                    "required": ["label", "description"]
                                }
                            }
                        },
                        "required": ["question", "header", "options"]
                    }
                },
                "answers": { "type": "object", "description": "Filled by UI in Claude Code; omit in Omiga" },
                "annotations": { "type": "object" },
                "metadata": { "type": "object" }
            },
            "required": ["questions"]
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_question() -> QuestionItem {
        QuestionItem {
            question: "Which crate?".to_string(),
            header: "Crate".to_string(),
            multi_select: false,
            options: vec![
                QuestionOption {
                    label: "tokio".to_string(),
                    description: "Async runtime".to_string(),
                    preview: None,
                },
                QuestionOption {
                    label: "async-std".to_string(),
                    description: "Alternative".to_string(),
                    preview: None,
                },
            ],
        }
    }

    #[test]
    fn validate_ok() {
        let args = AskUserQuestionArgs {
            questions: vec![sample_question()],
            answers: None,
            annotations: None,
            metadata: None,
        };
        validate(&args).unwrap();
    }

    #[test]
    fn duplicate_question_rejected() {
        let q = sample_question();
        let args = AskUserQuestionArgs {
            questions: vec![q.clone(), q],
            answers: None,
            annotations: None,
            metadata: None,
        };
        assert!(validate(&args).is_err());
    }

    #[test]
    fn preview_with_multiselect_rejected() {
        let args = AskUserQuestionArgs {
            questions: vec![QuestionItem {
                question: "Pick many?".to_string(),
                header: "Many".to_string(),
                multi_select: true,
                options: vec![
                    QuestionOption {
                        label: "A".to_string(),
                        description: "a".to_string(),
                        preview: Some("x".to_string()),
                    },
                    QuestionOption {
                        label: "B".to_string(),
                        description: "b".to_string(),
                        preview: None,
                    },
                ],
            }],
            answers: None,
            annotations: None,
            metadata: None,
        };
        assert!(validate(&args).is_err());
    }
}
