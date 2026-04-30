//! Tool system for Omiga
//!
//! Design decision (from eng review):
//! - Use static enum for tools in v0.1 (zero-overhead dispatch)
//! - Each tool implements the Tool trait with associated types for arguments/results
//! - Tool execution produces StreamOutput for unified streaming interface

pub mod agent;
pub mod ask_user_question;
pub mod bash;
pub mod enter_plan_mode;
pub mod env_store;
pub mod exit_plan_mode;
pub mod fetch;
pub mod file_edit;
pub mod file_read;
pub mod file_write;
pub mod glob;
pub mod grep;
pub mod list_mcp_resources;
pub mod list_skills;
pub mod notebook_edit;
pub mod query;
pub mod read_mcp_resource;
pub mod recall;
pub mod search;
pub mod send_user_message;
pub mod shell_file_ops;
pub mod skill_config;
pub mod skill_invoke;
pub mod skill_manage;
pub mod skill_view;
pub mod sleep;
pub mod ssh_paths;
pub mod task_create;
pub mod task_get;
pub mod task_list;
pub mod task_output;
pub mod task_stop;
pub mod task_update;
pub mod todo_write;
pub mod tool_search;
pub mod visualization;
pub mod web_safety;
pub mod workflow;

use crate::domain::agents::subagent_tool_filter::env_workflow_scripts_enabled;
use crate::domain::background_shell::BackgroundShellHandle;
use crate::domain::retrieval_registry::{self, RegistryEntryKind};
use crate::domain::session::AgentTask;
use crate::errors::ToolError;
use async_trait::async_trait;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// Unique identifier for a tool type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    Bash,
    FileEdit,
    FileRead,
    FileWrite,
    /// Content search (ripgrep semantics; API name `ripgrep`, see `grep` module).
    #[serde(rename = "ripgrep", alias = "grep")]
    Grep,
    Glob,
    Fetch,
    Query,
    Search,
    TodoWrite,
    NotebookEdit,
    Visualization,
    Sleep,
    AskUserQuestion,
    ListMcpResources,
    ReadMcpResource,
    #[serde(rename = "ListSkills")]
    ListSkills,
    #[serde(rename = "Skill")]
    SkillInvoke,
    #[serde(rename = "Agent")]
    Agent,
    #[serde(rename = "SendUserMessage")]
    SendUserMessage,
    #[serde(rename = "ExitPlanMode")]
    ExitPlanMode,
    #[serde(rename = "EnterPlanMode")]
    EnterPlanMode,
    #[serde(rename = "TaskStop")]
    TaskStop,
    #[serde(rename = "ToolSearch")]
    ToolSearch,
    #[serde(rename = "TaskOutput")]
    TaskOutput,
    #[serde(rename = "TaskCreate")]
    TaskCreate,
    #[serde(rename = "TaskGet")]
    TaskGet,
    #[serde(rename = "TaskList")]
    TaskList,
    #[serde(rename = "TaskUpdate")]
    TaskUpdate,
    Workflow,
    Recall,
}

impl fmt::Display for ToolKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ToolKind::Bash => write!(f, "bash"),
            ToolKind::FileEdit => write!(f, "file_edit"),
            ToolKind::FileRead => write!(f, "file_read"),
            ToolKind::FileWrite => write!(f, "file_write"),
            ToolKind::Grep => write!(f, "ripgrep"),
            ToolKind::Glob => write!(f, "glob"),
            ToolKind::Fetch => write!(f, "fetch"),
            ToolKind::Query => write!(f, "query"),
            ToolKind::Search => write!(f, "search"),
            ToolKind::TodoWrite => write!(f, "todo_write"),
            ToolKind::NotebookEdit => write!(f, "notebook_edit"),
            ToolKind::Visualization => write!(f, "visualization"),
            ToolKind::Sleep => write!(f, "sleep"),
            ToolKind::AskUserQuestion => write!(f, "ask_user_question"),
            ToolKind::ListMcpResources => write!(f, "list_mcp_resources"),
            ToolKind::ReadMcpResource => write!(f, "read_mcp_resource"),
            ToolKind::ListSkills => write!(f, "ListSkills"),
            ToolKind::SkillInvoke => write!(f, "Skill"),
            ToolKind::Agent => write!(f, "Agent"),
            ToolKind::SendUserMessage => write!(f, "SendUserMessage"),
            ToolKind::ExitPlanMode => write!(f, "ExitPlanMode"),
            ToolKind::EnterPlanMode => write!(f, "EnterPlanMode"),
            ToolKind::TaskStop => write!(f, "TaskStop"),
            ToolKind::ToolSearch => write!(f, "ToolSearch"),
            ToolKind::TaskOutput => write!(f, "TaskOutput"),
            ToolKind::TaskCreate => write!(f, "TaskCreate"),
            ToolKind::TaskGet => write!(f, "TaskGet"),
            ToolKind::TaskList => write!(f, "TaskList"),
            ToolKind::TaskUpdate => write!(f, "TaskUpdate"),
            ToolKind::Workflow => write!(f, "workflow"),
            ToolKind::Recall => write!(f, "recall"),
        }
    }
}

/// Static tool dispatch - zero overhead, compile-time validated
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "tool", content = "arguments")]
pub enum Tool {
    Bash(bash::BashArgs),
    FileEdit(file_edit::FileEditArgs),
    FileRead(file_read::FileReadArgs),
    FileWrite(file_write::FileWriteArgs),
    Grep(grep::GrepArgs),
    Glob(glob::GlobArgs),
    Fetch(fetch::FetchArgs),
    Query(query::QueryArgs),
    Search(search::SearchArgs),
    TodoWrite(todo_write::TodoWriteArgs),
    NotebookEdit(notebook_edit::NotebookEditArgs),
    Visualization(visualization::VisualizationArgs),
    Sleep(sleep::SleepArgs),
    AskUserQuestion(ask_user_question::AskUserQuestionArgs),
    ListMcpResources(list_mcp_resources::ListMcpResourcesArgs),
    ReadMcpResource(read_mcp_resource::ReadMcpResourceArgs),
    #[serde(rename = "ListSkills")]
    ListSkills(list_skills::ListSkillsArgs),
    #[serde(rename = "Skill")]
    SkillInvoke(skill_invoke::SkillInvokeArgs),
    Agent(agent::AgentArgs),
    SendUserMessage(send_user_message::SendUserMessageArgs),
    ExitPlanMode(exit_plan_mode::ExitPlanModeArgs),
    EnterPlanMode(enter_plan_mode::EnterPlanModeArgs),
    TaskStop(task_stop::TaskStopArgs),
    TaskOutput(task_output::TaskOutputArgs),
    ToolSearch(tool_search::ToolSearchArgs),
    TaskCreate(task_create::TaskCreateArgs),
    TaskGet(task_get::TaskGetArgs),
    TaskList(task_list::TaskListArgs),
    TaskUpdate(task_update::TaskUpdateArgs),
    #[serde(rename = "workflow")]
    Workflow(workflow::WorkflowArgs),
    Recall(recall::RecallArgs),
}

impl Tool {
    /// Get the tool kind
    pub fn kind(&self) -> ToolKind {
        match self {
            Tool::Bash(_) => ToolKind::Bash,
            Tool::FileEdit(_) => ToolKind::FileEdit,
            Tool::FileRead(_) => ToolKind::FileRead,
            Tool::FileWrite(_) => ToolKind::FileWrite,
            Tool::Grep(_) => ToolKind::Grep,
            Tool::Glob(_) => ToolKind::Glob,
            Tool::Fetch(_) => ToolKind::Fetch,
            Tool::Query(_) => ToolKind::Query,
            Tool::Search(_) => ToolKind::Search,
            Tool::TodoWrite(_) => ToolKind::TodoWrite,
            Tool::NotebookEdit(_) => ToolKind::NotebookEdit,
            Tool::Visualization(_) => ToolKind::Visualization,
            Tool::Sleep(_) => ToolKind::Sleep,
            Tool::AskUserQuestion(_) => ToolKind::AskUserQuestion,
            Tool::ListMcpResources(_) => ToolKind::ListMcpResources,
            Tool::ReadMcpResource(_) => ToolKind::ReadMcpResource,
            Tool::ListSkills(_) => ToolKind::ListSkills,
            Tool::SkillInvoke(_) => ToolKind::SkillInvoke,
            Tool::Agent(_) => ToolKind::Agent,
            Tool::SendUserMessage(_) => ToolKind::SendUserMessage,
            Tool::ExitPlanMode(_) => ToolKind::ExitPlanMode,
            Tool::EnterPlanMode(_) => ToolKind::EnterPlanMode,
            Tool::TaskStop(_) => ToolKind::TaskStop,
            Tool::TaskOutput(_) => ToolKind::TaskOutput,
            Tool::ToolSearch(_) => ToolKind::ToolSearch,
            Tool::TaskCreate(_) => ToolKind::TaskCreate,
            Tool::TaskGet(_) => ToolKind::TaskGet,
            Tool::TaskList(_) => ToolKind::TaskList,
            Tool::TaskUpdate(_) => ToolKind::TaskUpdate,
            Tool::Workflow(_) => ToolKind::Workflow,
            Tool::Recall(_) => ToolKind::Recall,
        }
    }

    /// Get the tool name for display
    pub fn name(&self) -> &'static str {
        match self {
            Tool::Bash(_) => "Bash",
            Tool::FileEdit(_) => "FileEdit",
            Tool::FileRead(_) => "FileRead",
            Tool::FileWrite(_) => "FileWrite",
            Tool::Grep(_) => "Ripgrep",
            Tool::Glob(_) => "Glob",
            Tool::Fetch(_) => "fetch",
            Tool::Query(_) => "query",
            Tool::Search(_) => "search",
            Tool::TodoWrite(_) => "TodoWrite",
            Tool::NotebookEdit(_) => "NotebookEdit",
            Tool::Visualization(_) => "Visualization",
            Tool::Sleep(_) => "Sleep",
            Tool::AskUserQuestion(_) => "AskUserQuestion",
            Tool::ListMcpResources(_) => "ListMcpResources",
            Tool::ReadMcpResource(_) => "ReadMcpResource",
            Tool::ListSkills(_) => "ListSkills",
            Tool::SkillInvoke(_) => "Skill",
            Tool::Agent(_) => "Agent",
            Tool::SendUserMessage(_) => "SendUserMessage",
            Tool::ExitPlanMode(_) => "ExitPlanMode",
            Tool::EnterPlanMode(_) => "EnterPlanMode",
            Tool::TaskStop(_) => "TaskStop",
            Tool::TaskOutput(_) => "TaskOutput",
            Tool::ToolSearch(_) => "ToolSearch",
            Tool::TaskCreate(_) => "TaskCreate",
            Tool::TaskGet(_) => "TaskGet",
            Tool::TaskList(_) => "TaskList",
            Tool::TaskUpdate(_) => "TaskUpdate",
            Tool::Workflow(_) => "workflow",
            Tool::Recall(_) => "recall",
        }
    }

    /// Get tool description for LLM
    pub fn description(&self) -> &'static str {
        match self {
            Tool::Bash(_) => bash::DESCRIPTION,
            Tool::FileEdit(_) => file_edit::DESCRIPTION,
            Tool::FileRead(_) => file_read::DESCRIPTION,
            Tool::FileWrite(_) => file_write::DESCRIPTION,
            Tool::Grep(_) => grep::DESCRIPTION,
            Tool::Glob(_) => glob::DESCRIPTION,
            Tool::Fetch(_) => fetch::DESCRIPTION,
            Tool::Query(_) => query::DESCRIPTION,
            Tool::Search(_) => search::DESCRIPTION,
            Tool::TodoWrite(_) => todo_write::DESCRIPTION,
            Tool::NotebookEdit(_) => notebook_edit::DESCRIPTION,
            Tool::Visualization(_) => visualization::DESCRIPTION,
            Tool::Sleep(_) => sleep::DESCRIPTION,
            Tool::AskUserQuestion(_) => ask_user_question::DESCRIPTION,
            Tool::ListMcpResources(_) => list_mcp_resources::DESCRIPTION,
            Tool::ReadMcpResource(_) => read_mcp_resource::DESCRIPTION,
            Tool::ListSkills(_) => list_skills::DESCRIPTION,
            Tool::SkillInvoke(_) => skill_invoke::DESCRIPTION,
            Tool::Agent(_) => agent::DESCRIPTION,
            Tool::SendUserMessage(_) => send_user_message::DESCRIPTION,
            Tool::ExitPlanMode(_) => exit_plan_mode::DESCRIPTION,
            Tool::EnterPlanMode(_) => enter_plan_mode::DESCRIPTION,
            Tool::TaskStop(_) => task_stop::DESCRIPTION,
            Tool::TaskOutput(_) => task_output::DESCRIPTION,
            Tool::ToolSearch(_) => tool_search::DESCRIPTION,
            Tool::TaskCreate(_) => task_create::DESCRIPTION,
            Tool::TaskGet(_) => task_get::DESCRIPTION,
            Tool::TaskList(_) => task_list::DESCRIPTION,
            Tool::TaskUpdate(_) => task_update::DESCRIPTION,
            Tool::Workflow(_) => workflow::DESCRIPTION,
            Tool::Recall(_) => recall::DESCRIPTION,
        }
    }

    /// Execute the tool and return a stream of outputs
    pub async fn execute(
        self,
        ctx: &ToolContext,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        let result: crate::infrastructure::streaming::StreamOutputBox = match self {
            Tool::Bash(args) => bash::BashTool::execute(ctx, args).await?,
            Tool::FileEdit(args) => file_edit::FileEditTool::execute(ctx, args).await?,
            Tool::FileRead(args) => file_read::FileReadTool::execute(ctx, args).await?,
            Tool::FileWrite(args) => file_write::FileWriteTool::execute(ctx, args).await?,
            Tool::Grep(args) => grep::GrepTool::execute(ctx, args).await?,
            Tool::Glob(args) => glob::GlobTool::execute(ctx, args).await?,
            Tool::Fetch(args) => fetch::FetchTool::execute(ctx, args).await?,
            Tool::Query(args) => query::QueryTool::execute(ctx, args).await?,
            Tool::Search(args) => search::SearchTool::execute(ctx, args).await?,
            Tool::TodoWrite(args) => todo_write::TodoWriteTool::execute(ctx, args).await?,
            Tool::NotebookEdit(args) => notebook_edit::NotebookEditTool::execute(ctx, args).await?,
            Tool::Visualization(args) => {
                visualization::VisualizationTool::execute(ctx, args).await?
            }
            Tool::Sleep(args) => sleep::SleepTool::execute(ctx, args).await?,
            Tool::AskUserQuestion(args) => {
                ask_user_question::AskUserQuestionTool::execute(ctx, args).await?
            }
            Tool::ListMcpResources(args) => {
                list_mcp_resources::ListMcpResourcesTool::execute(ctx, args).await?
            }
            Tool::ReadMcpResource(args) => {
                read_mcp_resource::ReadMcpResourceTool::execute(ctx, args).await?
            }
            Tool::ListSkills(args) => {
                use crate::domain::skills;
                use crate::infrastructure::streaming::{stream_single, StreamOutputItem};
                let cache = ctx.skill_cache.clone().unwrap_or_else(|| {
                    Arc::new(std::sync::Mutex::new(skills::SkillCacheMap::default()))
                });
                let all_skills = skills::load_skills_cached(&ctx.project_root, &cache).await;
                let task_ctx = ctx.skill_task_context.as_deref();
                let json =
                    skills::list_skills_metadata_json(&all_skills, args.query.as_deref(), task_ctx);
                return Ok(stream_single(StreamOutputItem::Text(json)));
            }
            Tool::SkillInvoke(args) => {
                use crate::domain::skills;
                use crate::infrastructure::streaming::{stream_single, StreamOutputItem};
                let skill_args = args.args.clone().or_else(|| args.arguments.clone());
                let cache = ctx.skill_cache.clone().unwrap_or_else(|| {
                    Arc::new(std::sync::Mutex::new(skills::SkillCacheMap::default()))
                });
                let all_skills = skills::load_skills_cached(&ctx.project_root, &cache).await;
                match skills::invoke_skill_with_cache(
                    &ctx.project_root,
                    &args.skill,
                    skill_args.as_deref().unwrap_or(""),
                    &all_skills,
                )
                .await
                {
                    Ok(text) => return Ok(stream_single(StreamOutputItem::Text(text))),
                    Err(e) => {
                        return Err(ToolError::ExecutionFailed {
                            message: e.to_string(),
                        })
                    }
                }
            }
            Tool::Agent(args) => agent::AgentTool::execute(ctx, args).await?,
            Tool::SendUserMessage(args) => {
                send_user_message::SendUserMessageTool::execute(ctx, args).await?
            }
            Tool::ExitPlanMode(args) => {
                exit_plan_mode::ExitPlanModeTool::execute(ctx, args).await?
            }
            Tool::EnterPlanMode(args) => {
                enter_plan_mode::EnterPlanModeTool::execute(ctx, args).await?
            }
            Tool::TaskStop(args) => task_stop::TaskStopTool::execute(ctx, args).await?,
            Tool::TaskOutput(args) => task_output::TaskOutputTool::execute(ctx, args).await?,
            Tool::ToolSearch(args) => tool_search::ToolSearchTool::execute(ctx, args).await?,
            Tool::TaskCreate(args) => task_create::TaskCreateTool::execute(ctx, args).await?,
            Tool::TaskGet(args) => task_get::TaskGetTool::execute(ctx, args).await?,
            Tool::TaskList(args) => task_list::TaskListTool::execute(ctx, args).await?,
            Tool::TaskUpdate(args) => task_update::TaskUpdateTool::execute(ctx, args).await?,
            Tool::Workflow(args) => workflow::WorkflowTool::execute(ctx, args).await?,
            Tool::Recall(args) => recall::RecallTool::execute(ctx, args).await?,
        };
        Ok(result)
    }

    /// Parse tool from JSON arguments
    pub fn from_json(kind: ToolKind, json: &str) -> Result<Self, ToolError> {
        match kind {
            ToolKind::Bash => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid bash arguments: {}", e),
                })?;
                Ok(Tool::Bash(args))
            }
            ToolKind::FileEdit => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid file_edit arguments: {}", e),
                })?;
                Ok(Tool::FileEdit(args))
            }
            ToolKind::FileRead => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid file_read arguments: {}", e),
                })?;
                Ok(Tool::FileRead(args))
            }
            ToolKind::FileWrite => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid file_write arguments: {}", e),
                })?;
                Ok(Tool::FileWrite(args))
            }
            ToolKind::Grep => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid ripgrep arguments: {}", e),
                })?;
                Ok(Tool::Grep(args))
            }
            ToolKind::Glob => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid glob arguments: {}", e),
                })?;
                Ok(Tool::Glob(args))
            }
            ToolKind::Fetch => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid fetch arguments: {}", e),
                })?;
                Ok(Tool::Fetch(args))
            }
            ToolKind::Query => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid query arguments: {}", e),
                })?;
                Ok(Tool::Query(args))
            }
            ToolKind::Search => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid search arguments: {}", e),
                })?;
                Ok(Tool::Search(args))
            }
            ToolKind::TodoWrite => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid todo_write arguments: {}", e),
                })?;
                Ok(Tool::TodoWrite(args))
            }
            ToolKind::NotebookEdit => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid notebook_edit arguments: {}", e),
                })?;
                Ok(Tool::NotebookEdit(args))
            }
            ToolKind::Visualization => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid visualization arguments: {}", e),
                })?;
                Ok(Tool::Visualization(args))
            }
            ToolKind::Sleep => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid sleep arguments: {}", e),
                })?;
                Ok(Tool::Sleep(args))
            }
            ToolKind::AskUserQuestion => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid ask_user_question arguments: {}", e),
                })?;
                Ok(Tool::AskUserQuestion(args))
            }
            ToolKind::ListMcpResources => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid list_mcp_resources arguments: {}", e),
                })?;
                Ok(Tool::ListMcpResources(args))
            }
            ToolKind::ReadMcpResource => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid read_mcp_resource arguments: {}", e),
                })?;
                Ok(Tool::ReadMcpResource(args))
            }
            ToolKind::ListSkills => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid ListSkills arguments: {}", e),
                })?;
                Ok(Tool::ListSkills(args))
            }
            ToolKind::SkillInvoke => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid SkillInvoke arguments: {}", e),
                })?;
                Ok(Tool::SkillInvoke(args))
            }
            ToolKind::Agent => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid Agent arguments: {}", e),
                })?;
                Ok(Tool::Agent(args))
            }
            ToolKind::SendUserMessage => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid SendUserMessage arguments: {}", e),
                })?;
                Ok(Tool::SendUserMessage(args))
            }
            ToolKind::ExitPlanMode => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid ExitPlanMode arguments: {}", e),
                })?;
                Ok(Tool::ExitPlanMode(args))
            }
            ToolKind::EnterPlanMode => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid EnterPlanMode arguments: {}", e),
                })?;
                Ok(Tool::EnterPlanMode(args))
            }
            ToolKind::TaskStop => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid TaskStop arguments: {}", e),
                })?;
                Ok(Tool::TaskStop(args))
            }
            ToolKind::TaskOutput => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid TaskOutput arguments: {}", e),
                })?;
                Ok(Tool::TaskOutput(args))
            }
            ToolKind::ToolSearch => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid ToolSearch arguments: {}", e),
                })?;
                Ok(Tool::ToolSearch(args))
            }
            ToolKind::TaskCreate => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid TaskCreate arguments: {}", e),
                })?;
                Ok(Tool::TaskCreate(args))
            }
            ToolKind::TaskGet => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid TaskGet arguments: {}", e),
                })?;
                Ok(Tool::TaskGet(args))
            }
            ToolKind::TaskList => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid TaskList arguments: {}", e),
                })?;
                Ok(Tool::TaskList(args))
            }
            ToolKind::TaskUpdate => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid TaskUpdate arguments: {}", e),
                })?;
                Ok(Tool::TaskUpdate(args))
            }
            ToolKind::Workflow => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid workflow arguments: {}", e),
                })?;
                Ok(Tool::Workflow(args))
            }
            ToolKind::Recall => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid recall arguments: {}", e),
                })?;
                Ok(Tool::Recall(args))
            }
        }
    }

    /// Parse tool from tool name string and JSON arguments
    pub fn from_json_str(tool_name: &str, json: &str) -> Result<Self, ToolError> {
        let kind = match tool_name {
            "bash" => ToolKind::Bash,
            "file_edit" => ToolKind::FileEdit,
            "file_read" => ToolKind::FileRead,
            "file_write" => ToolKind::FileWrite,
            "ripgrep" | "Ripgrep" | "grep" | "Grep" => ToolKind::Grep,
            "glob" => ToolKind::Glob,
            "fetch" => ToolKind::Fetch,
            "query" => ToolKind::Query,
            "search" => ToolKind::Search,
            "todo_write" => ToolKind::TodoWrite,
            "notebook_edit" => ToolKind::NotebookEdit,
            "visualization" => ToolKind::Visualization,
            "sleep" => ToolKind::Sleep,
            "ask_user_question" | "AskUserQuestion" => ToolKind::AskUserQuestion,
            "list_mcp_resources" | "ListMcpResourcesTool" => ToolKind::ListMcpResources,
            "read_mcp_resource" | "ReadMcpResourceTool" => ToolKind::ReadMcpResource,
            "ListSkills" => ToolKind::ListSkills,
            "Skill" => ToolKind::SkillInvoke,
            "Agent" | "Task" | "agent" => ToolKind::Agent,
            "SendUserMessage" | "Brief" | "send_user_message" => ToolKind::SendUserMessage,
            "ExitPlanMode" | "exit_plan_mode" => ToolKind::ExitPlanMode,
            "EnterPlanMode" | "enter_plan_mode" => ToolKind::EnterPlanMode,
            "TaskStop" | "task_stop" | "KillShell" => ToolKind::TaskStop,
            "TaskOutput" | "task_output" => ToolKind::TaskOutput,
            "ToolSearch" | "tool_search" => ToolKind::ToolSearch,
            "TaskCreate" | "task_create" => ToolKind::TaskCreate,
            "TaskGet" | "task_get" => ToolKind::TaskGet,
            "TaskList" | "task_list" => ToolKind::TaskList,
            "TaskUpdate" | "task_update" => ToolKind::TaskUpdate,
            "workflow" | "Workflow" => ToolKind::Workflow,
            "recall" | "Recall" => ToolKind::Recall,
            _ => {
                return Err(ToolError::UnknownTool {
                    name: tool_name.to_string(),
                })
            }
        };
        Self::from_json(kind, json)
    }
}

/// API keys/settings for built-in search/fetch adapters (Settings override env where supported).
///
/// The struct name is kept for config/state compatibility with existing Settings commands.
#[derive(Debug, Clone, Default)]
pub struct WebSearchApiKeys {
    pub tavily: Option<String>,
    pub exa: Option<String>,
    pub parallel: Option<String>,
    pub firecrawl: Option<String>,
    /// Self-hosted Firecrawl base URL, e.g. `https://api.firecrawl.dev` (no trailing path).
    pub firecrawl_url: Option<String>,
    /// Semantic Scholar is opt-in because the Academic Graph API requires a user key.
    pub semantic_scholar_enabled: bool,
    /// Optional Semantic Scholar Academic Graph API key.
    pub semantic_scholar_api_key: Option<String>,
    /// WeChat public-account search via Sogou is opt-in because it depends on a fragile public endpoint.
    pub wechat_search_enabled: bool,
    /// Optional NCBI E-utilities API key for PubMed.
    pub pubmed_api_key: Option<String>,
    /// NCBI contact email. Defaults to a local virtual mailbox when unset.
    pub pubmed_email: Option<String>,
    /// NCBI tool identifier. Defaults to `omiga` when unset.
    pub pubmed_tool_name: Option<String>,
    /// Enabled dataset subcategories for structured `query(category="dataset")` routing.
    /// `None` means the product default; `Some(vec![])` intentionally disables all.
    pub query_dataset_types: Option<Vec<String>>,
    /// Enabled dataset/data sources. `ena` covers all ENA child source variants.
    /// `None` means the product default; `Some(vec![])` intentionally disables all.
    pub query_dataset_sources: Option<Vec<String>>,
    /// Enabled external structured knowledge sources.
    /// `None` means the product default; `Some(vec![])` intentionally disables all.
    pub query_knowledge_sources: Option<Vec<String>>,
    /// New registry-backed enabled sources grouped by retrieval category.
    /// `None` means derive from legacy fields/defaults; per-category `Some(vec![])`
    /// intentionally disables that category's sources.
    pub enabled_sources_by_category: Option<HashMap<String, Vec<String>>>,
    /// New registry-backed enabled subcategories grouped by retrieval category.
    /// `None` means derive from legacy fields/defaults; per-category `Some(vec![])`
    /// intentionally disables that category's subcategories.
    pub enabled_subcategories_by_category: Option<HashMap<String, Vec<String>>>,
}

pub const QUERY_DATASET_TYPE_IDS: &[&str] = &[
    "expression",
    "sequencing",
    "genomics",
    "sample_metadata",
    "multi_omics",
];
pub const DEFAULT_QUERY_DATASET_TYPE_IDS: &[&str] =
    &["expression", "sequencing", "genomics", "sample_metadata"];

pub const QUERY_DATASET_SOURCE_IDS: &[&str] = &[
    "geo",
    "ena",
    "cbioportal",
    "gtex",
    "arrayexpress",
    "biosample",
];
pub const DEFAULT_QUERY_DATASET_SOURCE_IDS: &[&str] = &["geo", "ena"];

pub const QUERY_KNOWLEDGE_SOURCE_IDS: &[&str] = &["ncbi_gene", "ensembl", "uniprot"];
pub const DEFAULT_QUERY_KNOWLEDGE_SOURCE_IDS: &[&str] = &["ncbi_gene"];

fn normalize_query_setting_id(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace(['-', ' '], "_")
}

fn effective_query_setting(
    configured: Option<&Vec<String>>,
    allowed: &[&str],
    defaults: &[&str],
) -> Vec<String> {
    match configured {
        None => defaults.iter().map(|id| (*id).to_string()).collect(),
        Some(values) => {
            let normalized: Vec<String> = values
                .iter()
                .map(|value| normalize_query_setting_id(value))
                .collect();
            allowed
                .iter()
                .filter(|id| normalized.iter().any(|value| value == **id))
                .map(|id| (*id).to_string())
                .collect()
        }
    }
}

impl WebSearchApiKeys {
    pub fn enabled_sources_for_category(&self, category: &str) -> Vec<String> {
        let category = retrieval_registry::normalize_id(category);
        if let Some(values) = self
            .enabled_sources_by_category
            .as_ref()
            .and_then(|map| map.get(&category))
        {
            return retrieval_registry::normalize_enabled_ids(
                &category,
                values,
                RegistryEntryKind::Source,
                false,
            );
        }

        match category.as_str() {
            "dataset" => self.enabled_query_dataset_sources(),
            "knowledge" => self.enabled_query_knowledge_sources(),
            "literature" => {
                let mut values = retrieval_registry::default_source_ids("literature")
                    .into_iter()
                    .map(str::to_string)
                    .collect::<Vec<_>>();
                if self.semantic_scholar_enabled
                    && !values.iter().any(|id| id == "semantic_scholar")
                {
                    values.push("semantic_scholar".to_string());
                }
                values
            }
            "social" => {
                if self.wechat_search_enabled {
                    vec!["wechat".to_string()]
                } else {
                    retrieval_registry::default_source_ids("social")
                        .into_iter()
                        .map(str::to_string)
                        .collect()
                }
            }
            other => retrieval_registry::default_source_ids(other)
                .into_iter()
                .map(str::to_string)
                .collect(),
        }
    }

    pub fn enabled_subcategories_for_category(&self, category: &str) -> Vec<String> {
        let category = retrieval_registry::normalize_id(category);
        if let Some(values) = self
            .enabled_subcategories_by_category
            .as_ref()
            .and_then(|map| map.get(&category))
        {
            return retrieval_registry::normalize_enabled_ids(
                &category,
                values,
                RegistryEntryKind::Subcategory,
                false,
            );
        }

        match category.as_str() {
            "dataset" => self.enabled_query_dataset_types(),
            other => retrieval_registry::default_subcategory_ids(other)
                .into_iter()
                .map(str::to_string)
                .collect(),
        }
    }

    pub fn enabled_sources_by_category(&self) -> HashMap<String, Vec<String>> {
        let mut out = retrieval_registry::defaults_by_category(RegistryEntryKind::Source);
        for category in retrieval_registry::category_ids() {
            out.insert(
                category.to_string(),
                self.enabled_sources_for_category(category),
            );
        }
        out
    }

    pub fn enabled_subcategories_by_category(&self) -> HashMap<String, Vec<String>> {
        let mut out = retrieval_registry::defaults_by_category(RegistryEntryKind::Subcategory);
        for category in retrieval_registry::category_ids() {
            out.insert(
                category.to_string(),
                self.enabled_subcategories_for_category(category),
            );
        }
        out
    }

    pub fn enabled_query_dataset_types(&self) -> Vec<String> {
        if let Some(values) = self
            .enabled_subcategories_by_category
            .as_ref()
            .and_then(|map| map.get("dataset"))
        {
            return retrieval_registry::normalize_enabled_ids(
                "dataset",
                values,
                RegistryEntryKind::Subcategory,
                false,
            );
        }
        effective_query_setting(
            self.query_dataset_types.as_ref(),
            QUERY_DATASET_TYPE_IDS,
            DEFAULT_QUERY_DATASET_TYPE_IDS,
        )
    }

    pub fn enabled_query_dataset_sources(&self) -> Vec<String> {
        if let Some(values) = self
            .enabled_sources_by_category
            .as_ref()
            .and_then(|map| map.get("dataset"))
        {
            return retrieval_registry::normalize_enabled_ids(
                "dataset",
                values,
                RegistryEntryKind::Source,
                false,
            );
        }
        effective_query_setting(
            self.query_dataset_sources.as_ref(),
            QUERY_DATASET_SOURCE_IDS,
            DEFAULT_QUERY_DATASET_SOURCE_IDS,
        )
    }

    pub fn enabled_query_knowledge_sources(&self) -> Vec<String> {
        if let Some(values) = self
            .enabled_sources_by_category
            .as_ref()
            .and_then(|map| map.get("knowledge"))
        {
            let query_sources = values
                .iter()
                .filter_map(|source| {
                    let def = retrieval_registry::find_source("knowledge", source)?;
                    def.supports(retrieval_registry::RetrievalCapability::Query)
                        .then_some(def.id.to_string())
                })
                .collect::<Vec<_>>();
            return retrieval_registry::normalize_enabled_ids(
                "knowledge",
                &query_sources,
                RegistryEntryKind::Source,
                false,
            );
        }
        effective_query_setting(
            self.query_knowledge_sources.as_ref(),
            QUERY_KNOWLEDGE_SOURCE_IDS,
            DEFAULT_QUERY_KNOWLEDGE_SOURCE_IDS,
        )
    }

    pub fn is_query_dataset_type_enabled(&self, type_id: &str) -> bool {
        let normalized = normalize_query_setting_id(type_id);
        self.enabled_query_dataset_types()
            .iter()
            .any(|id| id == &normalized)
    }

    pub fn is_query_dataset_source_enabled(&self, source_id: &str) -> bool {
        let group =
            retrieval_registry::canonical_source_id("dataset", source_id).unwrap_or(source_id);
        self.enabled_query_dataset_sources()
            .iter()
            .any(|id| id == group)
    }

    pub fn is_query_knowledge_source_enabled(&self, source_id: &str) -> bool {
        let source =
            retrieval_registry::canonical_source_id("knowledge", source_id).unwrap_or(source_id);
        self.enabled_query_knowledge_sources()
            .iter()
            .any(|id| id == source)
    }
}

/// Execution context passed to all tools
#[derive(Clone)]
pub struct ToolContext {
    /// Active chat session id when available.
    pub session_id: Option<String>,
    /// Rendered session scratchpad excerpt for the current task, when available.
    pub working_memory_context: Option<String>,
    /// Current working directory
    pub cwd: std::path::PathBuf,
    /// Project root directory
    pub project_root: std::path::PathBuf,
    /// `local` | `ssh` | `sandbox` — from chat composer; tools may branch on this for execution surface.
    pub execution_environment: String,
    /// Selected SSH server name; used when `execution_environment == "ssh"`.
    pub ssh_server: Option<String>,
    /// `modal` | `daytona` | `docker` | `singularity` — sandbox backend; used when `execution_environment == "sandbox"`.
    pub sandbox_backend: String,
    /// Virtual env type for local execution: `"none"` | `"conda"` | `"venv"` | `"pyenv"`.
    pub local_venv_type: String,
    /// Virtual env name/path (conda env name, venv dir path, pyenv version string).
    pub local_venv_name: String,
    /// Session-scoped environment cache (hermes-agent `_active_environments` pattern).
    /// Shared across all tool calls in a session so `init_session()` runs once per backend.
    /// `None` in test / sub-agent contexts where no UI session is attached.
    pub env_store: Option<env_store::EnvStore>,
    /// Cancellation token
    pub cancel: tokio_util::sync::CancellationToken,
    /// Timeout duration (default: 60s)
    pub timeout_secs: u64,
    /// Session todo list (only set during chat tool execution)
    pub todos: Option<Arc<tokio::sync::Mutex<Vec<crate::domain::session::TodoItem>>>>,
    /// V2 task list (`TaskCreate` / `TaskGet` / `TaskUpdate` / `TaskList`), session-scoped.
    pub agent_tasks: Option<Arc<tokio::sync::Mutex<Vec<AgentTask>>>>,
    /// When set (chat UI), `bash` may use `run_in_background` and write output under `background_output_dir`.
    pub background_shell: Option<BackgroundShellHandle>,
    pub background_output_dir: Option<std::path::PathBuf>,
    /// Session `tool-results` directory (chat). Used by `read_mcp_resource` for binary blob persistence (TS parity).
    pub tool_results_dir: Option<std::path::PathBuf>,
    /// When set, `EnterPlanMode` / `ExitPlanMode` toggle this (TS `permissionMode === 'plan'`).
    pub plan_mode: Option<Arc<tokio::sync::Mutex<bool>>>,
    /// Search/fetch adapter API keys from Omiga Settings.
    pub web_search_api_keys: WebSearchApiKeys,
    /// Whether web tools should honor system/env proxy settings.
    pub web_use_proxy: bool,
    /// Preferred public search engine for `search(category="web")`: ddg, bing, or google.
    pub web_search_engine: String,
    /// Ordered web search methods selected in Settings, e.g. tavily → google → ddg.
    pub web_search_methods: Vec<String>,
    /// Skill metadata cache shared across tool calls in a session.
    #[allow(dead_code)]
    pub skill_cache: Option<Arc<std::sync::Mutex<crate::domain::skills::SkillCacheMap>>>,
    /// Task context string for `list_skills` relevance ranking (the user's current goal).
    pub skill_task_context: Option<String>,
}

impl fmt::Debug for ToolContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ToolContext")
            .field("session_id", &self.session_id)
            .field(
                "working_memory_context",
                &self
                    .working_memory_context
                    .as_ref()
                    .map(|_| "<working-memory>"),
            )
            .field("cwd", &self.cwd)
            .field("project_root", &self.project_root)
            .field("execution_environment", &self.execution_environment)
            .field("ssh_server", &self.ssh_server)
            .field("sandbox_backend", &self.sandbox_backend)
            .field("local_venv_type", &self.local_venv_type)
            .field("local_venv_name", &self.local_venv_name)
            .field("timeout_secs", &self.timeout_secs)
            .field("web_use_proxy", &self.web_use_proxy)
            .field("web_search_engine", &self.web_search_engine)
            .field("web_search_methods", &self.web_search_methods)
            .field("skill_cache", &self.skill_cache.as_ref().map(|_| "<cache>"))
            .field("skill_task_context", &self.skill_task_context)
            .finish_non_exhaustive()
    }
}

impl ToolContext {
    /// Create a new tool context
    pub fn new(project_root: impl Into<std::path::PathBuf>) -> Self {
        let project_root = project_root.into();
        Self {
            session_id: None,
            working_memory_context: None,
            cwd: project_root.clone(),
            project_root,
            execution_environment: "local".to_string(),
            ssh_server: None,
            sandbox_backend: String::new(),
            local_venv_type: String::new(),
            local_venv_name: String::new(),
            cancel: tokio_util::sync::CancellationToken::new(),
            timeout_secs: 60,
            todos: None,
            agent_tasks: None,
            background_shell: None,
            background_output_dir: None,
            tool_results_dir: None,
            plan_mode: None,
            web_search_api_keys: WebSearchApiKeys::default(),
            web_use_proxy: true,
            web_search_engine: "ddg".to_string(),
            web_search_methods: vec!["ddg".to_string(), "google".to_string(), "bing".to_string()],
            env_store: None,
            skill_cache: None,
            skill_task_context: None,
        }
    }

    pub fn with_skill_cache(
        mut self,
        cache: Arc<std::sync::Mutex<crate::domain::skills::SkillCacheMap>>,
    ) -> Self {
        self.skill_cache = Some(cache);
        self
    }

    pub fn with_skill_task_context(mut self, ctx: impl Into<String>) -> Self {
        self.skill_task_context = Some(ctx.into());
        self
    }

    pub fn with_session_id(mut self, session_id: impl Into<Option<String>>) -> Self {
        self.session_id = session_id.into();
        self
    }

    pub fn with_working_memory_context(
        mut self,
        working_memory_context: impl Into<Option<String>>,
    ) -> Self {
        self.working_memory_context = working_memory_context.into();
        self
    }

    /// Attach session todo storage (for `todo_write`)
    pub fn with_todos(
        mut self,
        todos: Option<Arc<tokio::sync::Mutex<Vec<crate::domain::session::TodoItem>>>>,
    ) -> Self {
        self.todos = todos;
        self
    }

    /// Attach session V2 task store (for `TaskCreate` / `TaskGet` / `TaskUpdate` / `TaskList`).
    pub fn with_agent_tasks(
        mut self,
        agent_tasks: Option<Arc<tokio::sync::Mutex<Vec<AgentTask>>>>,
    ) -> Self {
        self.agent_tasks = agent_tasks;
        self
    }

    /// Set a custom working directory
    pub fn with_cwd(mut self, cwd: impl Into<std::path::PathBuf>) -> Self {
        self.cwd = cwd.into();
        self
    }

    /// Set custom timeout
    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }

    /// Enable background bash (`run_in_background`) with per-session output directory.
    pub fn with_background_shell(
        mut self,
        handle: BackgroundShellHandle,
        output_dir: std::path::PathBuf,
    ) -> Self {
        self.background_shell = Some(handle);
        self.background_output_dir = Some(output_dir);
        self
    }

    /// Session tool-results directory (same path as chat `tool_results_dir_for_session`).
    pub fn with_tool_results_dir(mut self, dir: std::path::PathBuf) -> Self {
        self.tool_results_dir = Some(dir);
        self
    }

    /// Session plan-mode flag (`EnterPlanMode` / `ExitPlanMode`).
    pub fn with_plan_mode(mut self, flag: Option<Arc<tokio::sync::Mutex<bool>>>) -> Self {
        self.plan_mode = flag;
        self
    }

    /// Search/fetch adapter API keys from settings.
    pub fn with_web_search_api_keys(mut self, keys: WebSearchApiKeys) -> Self {
        self.web_search_api_keys = keys;
        self
    }

    pub fn with_web_use_proxy(mut self, enabled: bool) -> Self {
        self.web_use_proxy = enabled;
        self
    }

    pub fn with_web_search_engine(mut self, engine: impl Into<String>) -> Self {
        self.web_search_engine = engine.into();
        self
    }

    pub fn with_web_search_methods(mut self, methods: Vec<String>) -> Self {
        self.web_search_methods = methods;
        self
    }

    /// `local` | `ssh` | `sandbox` — set from chat composer for this round / session.
    pub fn with_execution_environment(mut self, env: impl Into<String>) -> Self {
        self.execution_environment = env.into();
        self
    }

    /// Selected SSH server name (used when `execution_environment == "ssh"`).
    pub fn with_ssh_server(mut self, server: impl Into<Option<String>>) -> Self {
        self.ssh_server = server.into();
        self
    }

    /// Remote sandbox backend from composer (used when `execution_environment == "sandbox"`).
    pub fn with_sandbox_backend(mut self, backend: impl Into<String>) -> Self {
        self.sandbox_backend = backend.into();
        self
    }

    /// Local virtual environment (conda env name, venv path, pyenv version).
    pub fn with_local_venv(
        mut self,
        venv_type: impl Into<String>,
        venv_name: impl Into<String>,
    ) -> Self {
        self.local_venv_type = venv_type.into();
        self.local_venv_name = venv_name.into();
        self
    }

    /// Attach the session-scoped environment cache (hermes-agent `_active_environments` pattern).
    /// Must be set from the chat session to enable remote file operations and env reuse.
    pub fn with_env_store(mut self, store: Option<env_store::EnvStore>) -> Self {
        self.env_store = store;
        self
    }

    /// Tavily only (convenience; merges into `web_search_api_keys`).
    pub fn with_tavily_search_api_key(mut self, key: Option<String>) -> Self {
        self.web_search_api_keys.tavily = key.filter(|s| !s.trim().is_empty());
        self
    }

    /// Use the round cancellation token (chat UI). Drives `bash` kill-on-stop and matches `cancel_stream`.
    pub fn with_cancel_token(mut self, token: tokio_util::sync::CancellationToken) -> Self {
        self.cancel = token;
        self
    }
}

/// Trait for individual tool implementations
#[async_trait]
pub trait ToolImpl: Send + Sync {
    /// Tool argument type
    type Args: DeserializeOwned + Send + 'static;

    /// Tool description for LLM system prompt
    const DESCRIPTION: &'static str;

    /// Execute the tool with given arguments, returning a boxed stream
    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError>;
}

/// Tool result types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolResult {
    Success { output: String },
    Error { message: String },
    Cancelled,
    Timeout,
}

/// All tool schemas for LLM. Always appends `skill_manage` (project skills CRUD).
/// When `include_skill` is true, also appends `list_skills`, `skill_view`, and `skill`
/// (only when the project has at least one `SKILL.md` — see `domain::skills`).
pub fn all_tool_schemas(include_skill: bool) -> Vec<ToolSchema> {
    let mut v = vec![
        bash::schema(),
        file_read::schema(),
        file_write::schema(),
        file_edit::schema(),
        notebook_edit::schema(),
        grep::schema(),
        glob::schema(),
        fetch::schema(),
        query::schema(),
        search::schema(),
        todo_write::schema(),
        visualization::schema(),
        sleep::schema(),
        ask_user_question::schema(),
        list_mcp_resources::schema(),
        read_mcp_resource::schema(),
        agent::schema(),
        exit_plan_mode::schema(),
        enter_plan_mode::schema(),
        task_stop::schema(),
        task_output::schema(),
        tool_search::schema(),
        task_create::schema(),
        task_get::schema(),
        task_list::schema(),
        task_update::schema(),
    ];
    v.push(recall::schema());
    if env_workflow_scripts_enabled() {
        v.push(workflow::schema());
    }
    // Procedural memory: available even when no skills exist yet (bootstrap creates first skill).
    v.push(skill_manage::schema());
    v.push(skill_config::schema());
    if include_skill {
        v.push(list_skills::schema());
        v.push(skill_view::schema());
        v.push(skill_invoke::schema());
    }
    v
}

/// JSON schema for a tool (for LLM function calling)
#[derive(Debug, Clone, Serialize)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

impl ToolSchema {
    /// Create a new tool schema
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: serde_json::Value,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            parameters,
        }
    }
}

use std::fmt;

/// Returns true when the named tool can safely run concurrently with other such tools.
///
/// Mirrors TS `isConcurrencySafe` on each tool class (see `toolOrchestration.ts`).
/// Only pure read-only tools that never modify shared state are safe to run in parallel.
pub fn is_concurrency_safe_by_name(name: &str) -> bool {
    matches!(
        name,
        "file_read"
            | "ripgrep"
            | "grep"
            | "glob"
            | "fetch"
            | "query"
            | "search"
            | "ToolSearch"
            | "tool_search"
            | "visualization"
            | "list_skills"  // read-only metadata scan
            | "skill_view"
            | "recall"
    )
}

#[cfg(test)]
mod tool_enum_tests {
    use super::*;

    #[test]
    fn from_json_str_notebook_edit_and_sleep() {
        let t = Tool::from_json_str(
            "notebook_edit",
            r#"{"notebook_path":"n.ipynb","new_source":"x","cell_id":"cell-0"}"#,
        )
        .unwrap();
        assert!(matches!(t, Tool::NotebookEdit(_)));

        let t = Tool::from_json_str("sleep", r#"{"duration":1.5}"#).unwrap();
        assert!(matches!(t, Tool::Sleep(_)));

        let t = Tool::from_json_str(
            "AskUserQuestion",
            r#"{"questions":[{"question":"A?","header":"H","options":[{"label":"x","description":"xd"},{"label":"y","description":"yd"}]}]}"#,
        )
        .unwrap();
        assert!(matches!(t, Tool::AskUserQuestion(_)));

        let t = Tool::from_json_str("ListMcpResourcesTool", "{}").unwrap();
        assert!(matches!(t, Tool::ListMcpResources(_)));

        let t = Tool::from_json_str("ReadMcpResourceTool", r#"{"server":"s","uri":"u"}"#).unwrap();
        assert!(matches!(t, Tool::ReadMcpResource(_)));

        let t =
            Tool::from_json_str("Agent", r#"{"description":"test","prompt":"do thing"}"#).unwrap();
        assert!(matches!(t, Tool::Agent(_)));

        let t = Tool::from_json_str("SendUserMessage", r#"{"message":"Hello"}"#).unwrap();
        assert!(matches!(t, Tool::SendUserMessage(_)));

        let t = Tool::from_json_str("TaskCreate", r#"{"subject":"T","description":"D"}"#).unwrap();
        assert!(matches!(t, Tool::TaskCreate(_)));

        let t = Tool::from_json_str("ripgrep", r#"{"pattern":"fn main"}"#).unwrap();
        assert!(matches!(t, Tool::Grep(_)));

        let t = Tool::from_json_str("grep", r#"{"pattern":"x"}"#).unwrap();
        assert!(matches!(t, Tool::Grep(_)));
    }
}
