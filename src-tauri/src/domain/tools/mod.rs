//! Tool system for Omiga
//!
//! Design decision (from eng review):
//! - Use static enum for tools in v0.1 (zero-overhead dispatch)
//! - Each tool implements the Tool trait with associated types for arguments/results
//! - Tool execution produces StreamOutput for unified streaming interface

pub mod bash;
pub mod file_edit;
pub mod file_read;
pub mod file_write;
pub mod glob;
pub mod grep;
pub mod web_fetch;
pub mod web_search;
pub mod todo_write;
pub mod notebook_edit;
pub mod sleep;
pub mod ask_user_question;
pub mod list_mcp_resources;
pub mod read_mcp_resource;
pub mod agent;
pub mod send_user_message;
pub mod exit_plan_mode;
pub mod enter_plan_mode;
pub mod task_stop;
pub mod task_output;
pub mod tool_search;
pub mod skill_invoke;
pub mod list_skills;
pub mod task_create;
pub mod task_get;
pub mod task_list;
pub mod task_update;
pub mod workflow;

use crate::domain::background_shell::BackgroundShellHandle;
use crate::domain::subagent_tool_filter::env_workflow_scripts_enabled;
use crate::domain::session::AgentTask;
use crate::errors::ToolError;
use std::sync::Arc;
use async_trait::async_trait;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

/// Unique identifier for a tool type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    Bash,
    FileEdit,
    FileRead,
    FileWrite,
    Grep,
    Glob,
    WebFetch,
    WebSearch,
    TodoWrite,
    NotebookEdit,
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
}

impl fmt::Display for ToolKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ToolKind::Bash => write!(f, "bash"),
            ToolKind::FileEdit => write!(f, "file_edit"),
            ToolKind::FileRead => write!(f, "file_read"),
            ToolKind::FileWrite => write!(f, "file_write"),
            ToolKind::Grep => write!(f, "grep"),
            ToolKind::Glob => write!(f, "glob"),
            ToolKind::WebFetch => write!(f, "web_fetch"),
            ToolKind::WebSearch => write!(f, "web_search"),
            ToolKind::TodoWrite => write!(f, "todo_write"),
            ToolKind::NotebookEdit => write!(f, "notebook_edit"),
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
    WebFetch(web_fetch::WebFetchArgs),
    WebSearch(web_search::WebSearchArgs),
    TodoWrite(todo_write::TodoWriteArgs),
    NotebookEdit(notebook_edit::NotebookEditArgs),
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
            Tool::WebFetch(_) => ToolKind::WebFetch,
            Tool::WebSearch(_) => ToolKind::WebSearch,
            Tool::TodoWrite(_) => ToolKind::TodoWrite,
            Tool::NotebookEdit(_) => ToolKind::NotebookEdit,
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
        }
    }

    /// Get the tool name for display
    pub fn name(&self) -> &'static str {
        match self {
            Tool::Bash(_) => "Bash",
            Tool::FileEdit(_) => "FileEdit",
            Tool::FileRead(_) => "FileRead",
            Tool::FileWrite(_) => "FileWrite",
            Tool::Grep(_) => "Grep",
            Tool::Glob(_) => "Glob",
            Tool::WebFetch(_) => "WebFetch",
            Tool::WebSearch(_) => "WebSearch",
            Tool::TodoWrite(_) => "TodoWrite",
            Tool::NotebookEdit(_) => "NotebookEdit",
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
            Tool::WebFetch(_) => web_fetch::DESCRIPTION,
            Tool::WebSearch(_) => web_search::DESCRIPTION,
            Tool::TodoWrite(_) => todo_write::DESCRIPTION,
            Tool::NotebookEdit(_) => notebook_edit::DESCRIPTION,
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
            Tool::WebFetch(args) => web_fetch::WebFetchTool::execute(ctx, args).await?,
            Tool::WebSearch(args) => web_search::WebSearchTool::execute(ctx, args).await?,
            Tool::TodoWrite(args) => todo_write::TodoWriteTool::execute(ctx, args).await?,
            Tool::NotebookEdit(args) => notebook_edit::NotebookEditTool::execute(ctx, args).await?,
            Tool::Sleep(args) => sleep::SleepTool::execute(ctx, args).await?,
            Tool::AskUserQuestion(args) => ask_user_question::AskUserQuestionTool::execute(ctx, args).await?,
            Tool::ListMcpResources(args) => list_mcp_resources::ListMcpResourcesTool::execute(ctx, args).await?,
            Tool::ReadMcpResource(args) => read_mcp_resource::ReadMcpResourceTool::execute(ctx, args).await?,
            Tool::ListSkills(_) => {
                return Err(ToolError::ExecutionFailed {
                    message: "ListSkills execution not yet implemented".to_string(),
                });
            }
            Tool::SkillInvoke(_) => {
                return Err(ToolError::ExecutionFailed {
                    message: "Skill invoke execution not yet implemented".to_string(),
                });
            }
            Tool::Agent(args) => agent::AgentTool::execute(ctx, args).await?,
            Tool::SendUserMessage(args) => send_user_message::SendUserMessageTool::execute(ctx, args).await?,
            Tool::ExitPlanMode(args) => exit_plan_mode::ExitPlanModeTool::execute(ctx, args).await?,
            Tool::EnterPlanMode(args) => enter_plan_mode::EnterPlanModeTool::execute(ctx, args).await?,
            Tool::TaskStop(args) => task_stop::TaskStopTool::execute(ctx, args).await?,
            Tool::TaskOutput(args) => task_output::TaskOutputTool::execute(ctx, args).await?,
            Tool::ToolSearch(args) => tool_search::ToolSearchTool::execute(ctx, args).await?,
            Tool::TaskCreate(args) => task_create::TaskCreateTool::execute(ctx, args).await?,
            Tool::TaskGet(args) => task_get::TaskGetTool::execute(ctx, args).await?,
            Tool::TaskList(args) => task_list::TaskListTool::execute(ctx, args).await?,
            Tool::TaskUpdate(args) => task_update::TaskUpdateTool::execute(ctx, args).await?,
            Tool::Workflow(args) => workflow::WorkflowTool::execute(ctx, args).await?,
        };
        Ok(result)
    }

    /// Parse tool from JSON arguments
    pub fn from_json(kind: ToolKind, json: &str) -> Result<Self, ToolError> {
        match kind {
            ToolKind::Bash => {
                let args = serde_json::from_str(json)
                    .map_err(|e| ToolError::InvalidArguments {
                        message: format!("Invalid bash arguments: {}", e),
                    })?;
                Ok(Tool::Bash(args))
            }
            ToolKind::FileEdit => {
                let args = serde_json::from_str(json)
                    .map_err(|e| ToolError::InvalidArguments {
                        message: format!("Invalid file_edit arguments: {}", e),
                    })?;
                Ok(Tool::FileEdit(args))
            }
            ToolKind::FileRead => {
                let args = serde_json::from_str(json)
                    .map_err(|e| ToolError::InvalidArguments {
                        message: format!("Invalid file_read arguments: {}", e),
                    })?;
                Ok(Tool::FileRead(args))
            }
            ToolKind::FileWrite => {
                let args = serde_json::from_str(json)
                    .map_err(|e| ToolError::InvalidArguments {
                        message: format!("Invalid file_write arguments: {}", e),
                    })?;
                Ok(Tool::FileWrite(args))
            }
            ToolKind::Grep => {
                let args = serde_json::from_str(json)
                    .map_err(|e| ToolError::InvalidArguments {
                        message: format!("Invalid grep arguments: {}", e),
                    })?;
                Ok(Tool::Grep(args))
            }
            ToolKind::Glob => {
                let args = serde_json::from_str(json)
                    .map_err(|e| ToolError::InvalidArguments {
                        message: format!("Invalid glob arguments: {}", e),
                    })?;
                Ok(Tool::Glob(args))
            }
            ToolKind::WebFetch => {
                let args = serde_json::from_str(json)
                    .map_err(|e| ToolError::InvalidArguments {
                        message: format!("Invalid web_fetch arguments: {}", e),
                    })?;
                Ok(Tool::WebFetch(args))
            }
            ToolKind::WebSearch => {
                let args = serde_json::from_str(json)
                    .map_err(|e| ToolError::InvalidArguments {
                        message: format!("Invalid web_search arguments: {}", e),
                    })?;
                Ok(Tool::WebSearch(args))
            }
            ToolKind::TodoWrite => {
                let args = serde_json::from_str(json)
                    .map_err(|e| ToolError::InvalidArguments {
                        message: format!("Invalid todo_write arguments: {}", e),
                    })?;
                Ok(Tool::TodoWrite(args))
            }
            ToolKind::NotebookEdit => {
                let args = serde_json::from_str(json)
                    .map_err(|e| ToolError::InvalidArguments {
                        message: format!("Invalid notebook_edit arguments: {}", e),
                    })?;
                Ok(Tool::NotebookEdit(args))
            }
            ToolKind::Sleep => {
                let args = serde_json::from_str(json)
                    .map_err(|e| ToolError::InvalidArguments {
                        message: format!("Invalid sleep arguments: {}", e),
                    })?;
                Ok(Tool::Sleep(args))
            }
            ToolKind::AskUserQuestion => {
                let args = serde_json::from_str(json)
                    .map_err(|e| ToolError::InvalidArguments {
                        message: format!("Invalid ask_user_question arguments: {}", e),
                    })?;
                Ok(Tool::AskUserQuestion(args))
            }
            ToolKind::ListMcpResources => {
                let args = serde_json::from_str(json)
                    .map_err(|e| ToolError::InvalidArguments {
                        message: format!("Invalid list_mcp_resources arguments: {}", e),
                    })?;
                Ok(Tool::ListMcpResources(args))
            }
            ToolKind::ReadMcpResource => {
                let args = serde_json::from_str(json)
                    .map_err(|e| ToolError::InvalidArguments {
                        message: format!("Invalid read_mcp_resource arguments: {}", e),
                    })?;
                Ok(Tool::ReadMcpResource(args))
            }
            ToolKind::ListSkills => {
                let args = serde_json::from_str(json)
                    .map_err(|e| ToolError::InvalidArguments {
                        message: format!("Invalid ListSkills arguments: {}", e),
                    })?;
                Ok(Tool::ListSkills(args))
            }
            ToolKind::SkillInvoke => {
                let args = serde_json::from_str(json)
                    .map_err(|e| ToolError::InvalidArguments {
                        message: format!("Invalid SkillInvoke arguments: {}", e),
                    })?;
                Ok(Tool::SkillInvoke(args))
            }
            ToolKind::Agent => {
                let args = serde_json::from_str(json)
                    .map_err(|e| ToolError::InvalidArguments {
                        message: format!("Invalid Agent arguments: {}", e),
                    })?;
                Ok(Tool::Agent(args))
            }
            ToolKind::SendUserMessage => {
                let args = serde_json::from_str(json)
                    .map_err(|e| ToolError::InvalidArguments {
                        message: format!("Invalid SendUserMessage arguments: {}", e),
                    })?;
                Ok(Tool::SendUserMessage(args))
            }
            ToolKind::ExitPlanMode => {
                let args = serde_json::from_str(json)
                    .map_err(|e| ToolError::InvalidArguments {
                        message: format!("Invalid ExitPlanMode arguments: {}", e),
                    })?;
                Ok(Tool::ExitPlanMode(args))
            }
            ToolKind::EnterPlanMode => {
                let args = serde_json::from_str(json)
                    .map_err(|e| ToolError::InvalidArguments {
                        message: format!("Invalid EnterPlanMode arguments: {}", e),
                    })?;
                Ok(Tool::EnterPlanMode(args))
            }
            ToolKind::TaskStop => {
                let args = serde_json::from_str(json)
                    .map_err(|e| ToolError::InvalidArguments {
                        message: format!("Invalid TaskStop arguments: {}", e),
                    })?;
                Ok(Tool::TaskStop(args))
            }
            ToolKind::TaskOutput => {
                let args = serde_json::from_str(json)
                    .map_err(|e| ToolError::InvalidArguments {
                        message: format!("Invalid TaskOutput arguments: {}", e),
                    })?;
                Ok(Tool::TaskOutput(args))
            }
            ToolKind::ToolSearch => {
                let args = serde_json::from_str(json)
                    .map_err(|e| ToolError::InvalidArguments {
                        message: format!("Invalid ToolSearch arguments: {}", e),
                    })?;
                Ok(Tool::ToolSearch(args))
            }
            ToolKind::TaskCreate => {
                let args = serde_json::from_str(json)
                    .map_err(|e| ToolError::InvalidArguments {
                        message: format!("Invalid TaskCreate arguments: {}", e),
                    })?;
                Ok(Tool::TaskCreate(args))
            }
            ToolKind::TaskGet => {
                let args = serde_json::from_str(json)
                    .map_err(|e| ToolError::InvalidArguments {
                        message: format!("Invalid TaskGet arguments: {}", e),
                    })?;
                Ok(Tool::TaskGet(args))
            }
            ToolKind::TaskList => {
                let args = serde_json::from_str(json)
                    .map_err(|e| ToolError::InvalidArguments {
                        message: format!("Invalid TaskList arguments: {}", e),
                    })?;
                Ok(Tool::TaskList(args))
            }
            ToolKind::TaskUpdate => {
                let args = serde_json::from_str(json)
                    .map_err(|e| ToolError::InvalidArguments {
                        message: format!("Invalid TaskUpdate arguments: {}", e),
                    })?;
                Ok(Tool::TaskUpdate(args))
            }
            ToolKind::Workflow => {
                let args = serde_json::from_str(json)
                    .map_err(|e| ToolError::InvalidArguments {
                        message: format!("Invalid workflow arguments: {}", e),
                    })?;
                Ok(Tool::Workflow(args))
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
            "grep" => ToolKind::Grep,
            "glob" => ToolKind::Glob,
            "web_fetch" => ToolKind::WebFetch,
            "web_search" => ToolKind::WebSearch,
            "todo_write" => ToolKind::TodoWrite,
            "notebook_edit" => ToolKind::NotebookEdit,
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
            _ => {
                return Err(ToolError::UnknownTool {
                    name: tool_name.to_string(),
                })
            }
        };
        Self::from_json(kind, json)
    }
}

/// Execution context passed to all tools
#[derive(Debug, Clone)]
pub struct ToolContext {
    /// Current working directory
    pub cwd: std::path::PathBuf,
    /// Project root directory
    pub project_root: std::path::PathBuf,
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
    /// Brave Search API key from Omiga Settings (non-empty overrides `OMIGA_BRAVE_API_KEY` / `BRAVE_API_KEY`).
    pub brave_search_api_key: Option<String>,
}

impl ToolContext {
    /// Create a new tool context
    pub fn new(project_root: impl Into<std::path::PathBuf>) -> Self {
        let project_root = project_root.into();
        Self {
            cwd: project_root.clone(),
            project_root,
            cancel: tokio_util::sync::CancellationToken::new(),
            timeout_secs: 60,
            todos: None,
            agent_tasks: None,
            background_shell: None,
            background_output_dir: None,
            tool_results_dir: None,
            plan_mode: None,
            brave_search_api_key: None,
        }
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

    /// Brave Search API key from settings (used by `web_search`).
    pub fn with_brave_search_api_key(mut self, key: Option<String>) -> Self {
        self.brave_search_api_key = key.filter(|s| !s.trim().is_empty());
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

/// All tool schemas for LLM. When `include_skill` is true, appends `list_skills` and `skill`
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
        web_fetch::schema(),
        web_search::schema(),
        todo_write::schema(),
        sleep::schema(),
        ask_user_question::schema(),
        list_mcp_resources::schema(),
        read_mcp_resource::schema(),
        agent::schema(),
        send_user_message::schema(),
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
    if env_workflow_scripts_enabled() {
        v.push(workflow::schema());
    }
    if include_skill {
        v.push(list_skills::schema());
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
            | "grep"
            | "glob"
            | "web_fetch"
            | "web_search"
            | "ToolSearch"
            | "tool_search"
            | "list_skills"  // read-only metadata scan
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

        let t = Tool::from_json_str(
            "ReadMcpResourceTool",
            r#"{"server":"s","uri":"u"}"#,
        )
        .unwrap();
        assert!(matches!(t, Tool::ReadMcpResource(_)));

        let t = Tool::from_json_str(
            "Agent",
            r#"{"description":"test","prompt":"do thing"}"#,
        )
        .unwrap();
        assert!(matches!(t, Tool::Agent(_)));

        let t = Tool::from_json_str(
            "SendUserMessage",
            r#"{"message":"Hello"}"#,
        )
        .unwrap();
        assert!(matches!(t, Tool::SendUserMessage(_)));

        let t = Tool::from_json_str(
            "TaskCreate",
            r#"{"subject":"T","description":"D"}"#,
        )
        .unwrap();
        assert!(matches!(t, Tool::TaskCreate(_)));
    }
}
