//! Tool system for Omiga
//!
//! Design decision (from eng review):
//! - Use static enum for tools in v0.1 (zero-overhead dispatch)
//! - Each tool implements the Tool trait with associated types for arguments/results
//! - Tool execution produces StreamOutput for unified streaming interface

// Keep public module names flat for compatibility while storing same-family
// tool implementations in subdirectories.
pub mod agent;
#[path = "interaction/ask_user_question.rs"]
pub mod ask_user_question;
pub mod bash;
pub mod connector;
pub mod cron_create;
pub mod cron_delete;
pub mod cron_list;
#[path = "plan/enter_mode.rs"]
pub mod enter_plan_mode;
pub mod enter_worktree;
pub mod env_store;
#[path = "environment/profile_check.rs"]
pub mod environment_profile_check;
#[path = "environment/profile_prepare_plan.rs"]
pub mod environment_profile_prepare_plan;
#[path = "execution/archive_advisor.rs"]
pub mod execution_archive_advisor;
#[path = "execution/archive_suggestion_write.rs"]
pub mod execution_archive_suggestion_write;
#[path = "execution/lineage_report.rs"]
pub mod execution_lineage_report;
#[path = "execution/record_detail.rs"]
pub mod execution_record_detail;
#[path = "execution/record_list.rs"]
pub mod execution_record_list;
#[path = "plan/exit_mode.rs"]
pub mod exit_plan_mode;
pub mod exit_worktree;
pub mod fetch;
#[path = "file/edit.rs"]
pub mod file_edit;
#[path = "file/read.rs"]
pub mod file_read;
#[path = "file/write.rs"]
pub mod file_write;
#[path = "file/glob.rs"]
pub mod glob;
#[path = "file/grep.rs"]
pub mod grep;
#[path = "learning/preference_candidate_list.rs"]
pub mod learning_preference_candidate_list;
#[path = "learning/preference_candidate_promote.rs"]
pub mod learning_preference_candidate_promote;
#[path = "learning/proposal_apply.rs"]
pub mod learning_proposal_apply;
#[path = "learning/proposal_decide.rs"]
pub mod learning_proposal_decide;
#[path = "learning/proposal_list.rs"]
pub mod learning_proposal_list;
#[path = "learning/self_evolution_creator.rs"]
pub mod learning_self_evolution_creator;
#[path = "learning/self_evolution_draft_write.rs"]
pub mod learning_self_evolution_draft_write;
#[path = "learning/self_evolution_report.rs"]
pub mod learning_self_evolution_report;
#[path = "mcp/list_resources.rs"]
pub mod list_mcp_resources;
#[path = "skill/list.rs"]
pub mod list_skills;
pub mod monitor;
#[path = "notebook/edit.rs"]
pub mod notebook_edit;
#[path = "operator/describe.rs"]
pub mod operator_describe;
#[path = "operator/list.rs"]
pub mod operator_list;
pub mod push_notification;
pub mod query;
#[path = "mcp/read_resource.rs"]
pub mod read_mcp_resource;
pub mod recall;
pub mod search;
#[path = "interaction/send_user_message.rs"]
pub mod send_user_message;
#[path = "file/shell_ops.rs"]
pub mod shell_file_ops;
#[path = "skill/config.rs"]
pub mod skill_config;
#[path = "skill/invoke.rs"]
pub mod skill_invoke;
#[path = "skill/manage.rs"]
pub mod skill_manage;
#[path = "skill/view.rs"]
pub mod skill_view;
pub mod sleep;
#[path = "file/ssh_paths.rs"]
pub mod ssh_paths;
#[path = "task/create.rs"]
pub mod task_create;
#[path = "task/get.rs"]
pub mod task_get;
#[path = "task/list.rs"]
pub mod task_list;
#[path = "task/output.rs"]
pub mod task_output;
#[path = "task/stop.rs"]
pub mod task_stop;
#[path = "task/update.rs"]
pub mod task_update;
pub mod template_execute;
pub mod todo_write;
pub mod tool_search;
#[path = "unit/authoring_validate.rs"]
pub mod unit_authoring_validate;
#[path = "unit/describe.rs"]
pub mod unit_describe;
#[path = "unit/list.rs"]
pub mod unit_list;
#[path = "unit/search.rs"]
pub mod unit_search;
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
    Connector,
    TodoWrite,
    NotebookEdit,
    OperatorList,
    OperatorDescribe,
    UnitList,
    UnitSearch,
    UnitDescribe,
    UnitAuthoringValidate,
    TemplateExecute,
    EnvironmentProfileCheck,
    EnvironmentProfilePreparePlan,
    ExecutionArchiveAdvisor,
    ExecutionArchiveSuggestionWrite,
    LearningProposalList,
    LearningProposalDecide,
    LearningProposalApply,
    LearningPreferenceCandidateList,
    LearningPreferenceCandidatePromote,
    LearningSelfEvolutionCreator,
    LearningSelfEvolutionDraftWrite,
    LearningSelfEvolutionReport,
    ExecutionLineageReport,
    ExecutionRecordDetail,
    ExecutionRecordList,
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
    #[serde(rename = "EnterWorktree")]
    EnterWorktree,
    #[serde(rename = "ExitWorktree")]
    ExitWorktree,
    #[serde(rename = "Monitor")]
    Monitor,
    #[serde(rename = "PushNotification")]
    PushNotification,
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
    #[serde(rename = "CronCreate")]
    CronCreate,
    #[serde(rename = "CronList")]
    CronList,
    #[serde(rename = "CronDelete")]
    CronDelete,
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
            ToolKind::Connector => write!(f, "connector"),
            ToolKind::TodoWrite => write!(f, "todo_write"),
            ToolKind::NotebookEdit => write!(f, "notebook_edit"),
            ToolKind::OperatorList => write!(f, "operator_list"),
            ToolKind::OperatorDescribe => write!(f, "operator_describe"),
            ToolKind::UnitList => write!(f, "unit_list"),
            ToolKind::UnitSearch => write!(f, "unit_search"),
            ToolKind::UnitDescribe => write!(f, "unit_describe"),
            ToolKind::UnitAuthoringValidate => write!(f, "unit_authoring_validate"),
            ToolKind::TemplateExecute => write!(f, "template_execute"),
            ToolKind::EnvironmentProfileCheck => write!(f, "environment_profile_check"),
            ToolKind::EnvironmentProfilePreparePlan => {
                write!(f, "environment_profile_prepare_plan")
            }
            ToolKind::ExecutionArchiveAdvisor => write!(f, "execution_archive_advisor"),
            ToolKind::ExecutionArchiveSuggestionWrite => {
                write!(f, "execution_archive_suggestion_write")
            }
            ToolKind::LearningProposalList => write!(f, "learning_proposal_list"),
            ToolKind::LearningProposalDecide => write!(f, "learning_proposal_decide"),
            ToolKind::LearningProposalApply => write!(f, "learning_proposal_apply"),
            ToolKind::LearningPreferenceCandidateList => {
                write!(f, "learning_preference_candidate_list")
            }
            ToolKind::LearningPreferenceCandidatePromote => {
                write!(f, "learning_preference_candidate_promote")
            }
            ToolKind::LearningSelfEvolutionCreator => {
                write!(f, "learning_self_evolution_creator")
            }
            ToolKind::LearningSelfEvolutionDraftWrite => {
                write!(f, "learning_self_evolution_draft_write")
            }
            ToolKind::LearningSelfEvolutionReport => write!(f, "learning_self_evolution_report"),
            ToolKind::ExecutionLineageReport => write!(f, "execution_lineage_report"),
            ToolKind::ExecutionRecordDetail => write!(f, "execution_record_detail"),
            ToolKind::ExecutionRecordList => write!(f, "execution_record_list"),
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
            ToolKind::EnterWorktree => write!(f, "EnterWorktree"),
            ToolKind::ExitWorktree => write!(f, "ExitWorktree"),
            ToolKind::Monitor => write!(f, "Monitor"),
            ToolKind::PushNotification => write!(f, "PushNotification"),
            ToolKind::TaskStop => write!(f, "TaskStop"),
            ToolKind::ToolSearch => write!(f, "ToolSearch"),
            ToolKind::TaskOutput => write!(f, "TaskOutput"),
            ToolKind::TaskCreate => write!(f, "TaskCreate"),
            ToolKind::TaskGet => write!(f, "TaskGet"),
            ToolKind::TaskList => write!(f, "TaskList"),
            ToolKind::TaskUpdate => write!(f, "TaskUpdate"),
            ToolKind::Workflow => write!(f, "workflow"),
            ToolKind::Recall => write!(f, "recall"),
            ToolKind::CronCreate => write!(f, "CronCreate"),
            ToolKind::CronList => write!(f, "CronList"),
            ToolKind::CronDelete => write!(f, "CronDelete"),
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
    Connector(connector::ConnectorArgs),
    TodoWrite(todo_write::TodoWriteArgs),
    NotebookEdit(notebook_edit::NotebookEditArgs),
    OperatorList(operator_list::OperatorListArgs),
    OperatorDescribe(operator_describe::OperatorDescribeArgs),
    UnitList(unit_list::UnitListArgs),
    UnitSearch(unit_search::UnitSearchArgs),
    UnitDescribe(unit_describe::UnitDescribeArgs),
    UnitAuthoringValidate(unit_authoring_validate::UnitAuthoringValidateArgs),
    TemplateExecute(template_execute::TemplateExecuteArgs),
    EnvironmentProfileCheck(environment_profile_check::EnvironmentProfileCheckArgs),
    EnvironmentProfilePreparePlan(
        environment_profile_prepare_plan::EnvironmentProfilePreparePlanArgs,
    ),
    ExecutionArchiveAdvisor(execution_archive_advisor::ExecutionArchiveAdvisorArgs),
    ExecutionArchiveSuggestionWrite(
        execution_archive_suggestion_write::ExecutionArchiveSuggestionWriteArgs,
    ),
    LearningProposalList(learning_proposal_list::LearningProposalListArgs),
    LearningProposalDecide(learning_proposal_decide::LearningProposalDecideArgs),
    LearningProposalApply(learning_proposal_apply::LearningProposalApplyArgs),
    LearningPreferenceCandidateList(
        learning_preference_candidate_list::LearningPreferenceCandidateListArgs,
    ),
    LearningPreferenceCandidatePromote(
        learning_preference_candidate_promote::LearningPreferenceCandidatePromoteArgs,
    ),
    LearningSelfEvolutionCreator(learning_self_evolution_creator::LearningSelfEvolutionCreatorArgs),
    LearningSelfEvolutionDraftWrite(
        learning_self_evolution_draft_write::LearningSelfEvolutionDraftWriteArgs,
    ),
    LearningSelfEvolutionReport(learning_self_evolution_report::LearningSelfEvolutionReportArgs),
    ExecutionLineageReport(execution_lineage_report::ExecutionLineageReportArgs),
    ExecutionRecordDetail(execution_record_detail::ExecutionRecordDetailArgs),
    ExecutionRecordList(execution_record_list::ExecutionRecordListArgs),
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
    EnterWorktree(enter_worktree::EnterWorktreeArgs),
    ExitWorktree(exit_worktree::ExitWorktreeArgs),
    Monitor(monitor::MonitorArgs),
    PushNotification(push_notification::PushNotificationArgs),
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
    CronCreate(cron_create::CronCreateArgs),
    CronList(cron_list::CronListArgs),
    CronDelete(cron_delete::CronDeleteArgs),
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
            Tool::Connector(_) => ToolKind::Connector,
            Tool::TodoWrite(_) => ToolKind::TodoWrite,
            Tool::NotebookEdit(_) => ToolKind::NotebookEdit,
            Tool::OperatorList(_) => ToolKind::OperatorList,
            Tool::OperatorDescribe(_) => ToolKind::OperatorDescribe,
            Tool::UnitList(_) => ToolKind::UnitList,
            Tool::UnitSearch(_) => ToolKind::UnitSearch,
            Tool::UnitDescribe(_) => ToolKind::UnitDescribe,
            Tool::UnitAuthoringValidate(_) => ToolKind::UnitAuthoringValidate,
            Tool::TemplateExecute(_) => ToolKind::TemplateExecute,
            Tool::EnvironmentProfileCheck(_) => ToolKind::EnvironmentProfileCheck,
            Tool::EnvironmentProfilePreparePlan(_) => ToolKind::EnvironmentProfilePreparePlan,
            Tool::ExecutionArchiveAdvisor(_) => ToolKind::ExecutionArchiveAdvisor,
            Tool::ExecutionArchiveSuggestionWrite(_) => ToolKind::ExecutionArchiveSuggestionWrite,
            Tool::LearningProposalList(_) => ToolKind::LearningProposalList,
            Tool::LearningProposalDecide(_) => ToolKind::LearningProposalDecide,
            Tool::LearningProposalApply(_) => ToolKind::LearningProposalApply,
            Tool::LearningPreferenceCandidateList(_) => ToolKind::LearningPreferenceCandidateList,
            Tool::LearningPreferenceCandidatePromote(_) => {
                ToolKind::LearningPreferenceCandidatePromote
            }
            Tool::LearningSelfEvolutionCreator(_) => ToolKind::LearningSelfEvolutionCreator,
            Tool::LearningSelfEvolutionDraftWrite(_) => ToolKind::LearningSelfEvolutionDraftWrite,
            Tool::LearningSelfEvolutionReport(_) => ToolKind::LearningSelfEvolutionReport,
            Tool::ExecutionLineageReport(_) => ToolKind::ExecutionLineageReport,
            Tool::ExecutionRecordDetail(_) => ToolKind::ExecutionRecordDetail,
            Tool::ExecutionRecordList(_) => ToolKind::ExecutionRecordList,
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
            Tool::EnterWorktree(_) => ToolKind::EnterWorktree,
            Tool::ExitWorktree(_) => ToolKind::ExitWorktree,
            Tool::Monitor(_) => ToolKind::Monitor,
            Tool::PushNotification(_) => ToolKind::PushNotification,
            Tool::TaskStop(_) => ToolKind::TaskStop,
            Tool::TaskOutput(_) => ToolKind::TaskOutput,
            Tool::ToolSearch(_) => ToolKind::ToolSearch,
            Tool::TaskCreate(_) => ToolKind::TaskCreate,
            Tool::TaskGet(_) => ToolKind::TaskGet,
            Tool::TaskList(_) => ToolKind::TaskList,
            Tool::TaskUpdate(_) => ToolKind::TaskUpdate,
            Tool::Workflow(_) => ToolKind::Workflow,
            Tool::Recall(_) => ToolKind::Recall,
            Tool::CronCreate(_) => ToolKind::CronCreate,
            Tool::CronList(_) => ToolKind::CronList,
            Tool::CronDelete(_) => ToolKind::CronDelete,
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
            Tool::Connector(_) => "connector",
            Tool::TodoWrite(_) => "TodoWrite",
            Tool::NotebookEdit(_) => "NotebookEdit",
            Tool::OperatorList(_) => "operator_list",
            Tool::OperatorDescribe(_) => "operator_describe",
            Tool::UnitList(_) => "unit_list",
            Tool::UnitSearch(_) => "unit_search",
            Tool::UnitDescribe(_) => "unit_describe",
            Tool::UnitAuthoringValidate(_) => "unit_authoring_validate",
            Tool::TemplateExecute(_) => "template_execute",
            Tool::EnvironmentProfileCheck(_) => "environment_profile_check",
            Tool::EnvironmentProfilePreparePlan(_) => "environment_profile_prepare_plan",
            Tool::ExecutionArchiveAdvisor(_) => "execution_archive_advisor",
            Tool::ExecutionArchiveSuggestionWrite(_) => "execution_archive_suggestion_write",
            Tool::LearningProposalList(_) => "learning_proposal_list",
            Tool::LearningProposalDecide(_) => "learning_proposal_decide",
            Tool::LearningProposalApply(_) => "learning_proposal_apply",
            Tool::LearningPreferenceCandidateList(_) => "learning_preference_candidate_list",
            Tool::LearningPreferenceCandidatePromote(_) => "learning_preference_candidate_promote",
            Tool::LearningSelfEvolutionCreator(_) => "learning_self_evolution_creator",
            Tool::LearningSelfEvolutionDraftWrite(_) => "learning_self_evolution_draft_write",
            Tool::LearningSelfEvolutionReport(_) => "learning_self_evolution_report",
            Tool::ExecutionLineageReport(_) => "execution_lineage_report",
            Tool::ExecutionRecordDetail(_) => "execution_record_detail",
            Tool::ExecutionRecordList(_) => "execution_record_list",
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
            Tool::EnterWorktree(_) => "EnterWorktree",
            Tool::ExitWorktree(_) => "ExitWorktree",
            Tool::Monitor(_) => "Monitor",
            Tool::PushNotification(_) => "PushNotification",
            Tool::TaskStop(_) => "TaskStop",
            Tool::TaskOutput(_) => "TaskOutput",
            Tool::ToolSearch(_) => "ToolSearch",
            Tool::TaskCreate(_) => "TaskCreate",
            Tool::TaskGet(_) => "TaskGet",
            Tool::TaskList(_) => "TaskList",
            Tool::TaskUpdate(_) => "TaskUpdate",
            Tool::Workflow(_) => "workflow",
            Tool::Recall(_) => "recall",
            Tool::CronCreate(_) => "CronCreate",
            Tool::CronList(_) => "CronList",
            Tool::CronDelete(_) => "CronDelete",
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
            Tool::Connector(_) => connector::DESCRIPTION,
            Tool::TodoWrite(_) => todo_write::DESCRIPTION,
            Tool::NotebookEdit(_) => notebook_edit::DESCRIPTION,
            Tool::OperatorList(_) => operator_list::DESCRIPTION,
            Tool::OperatorDescribe(_) => operator_describe::DESCRIPTION,
            Tool::UnitList(_) => unit_list::DESCRIPTION,
            Tool::UnitSearch(_) => unit_search::DESCRIPTION,
            Tool::UnitDescribe(_) => unit_describe::DESCRIPTION,
            Tool::UnitAuthoringValidate(_) => unit_authoring_validate::DESCRIPTION,
            Tool::TemplateExecute(_) => template_execute::DESCRIPTION,
            Tool::EnvironmentProfileCheck(_) => environment_profile_check::DESCRIPTION,
            Tool::EnvironmentProfilePreparePlan(_) => environment_profile_prepare_plan::DESCRIPTION,
            Tool::ExecutionArchiveAdvisor(_) => execution_archive_advisor::DESCRIPTION,
            Tool::ExecutionArchiveSuggestionWrite(_) => {
                execution_archive_suggestion_write::DESCRIPTION
            }
            Tool::LearningProposalList(_) => learning_proposal_list::DESCRIPTION,
            Tool::LearningProposalDecide(_) => learning_proposal_decide::DESCRIPTION,
            Tool::LearningProposalApply(_) => learning_proposal_apply::DESCRIPTION,
            Tool::LearningPreferenceCandidateList(_) => {
                learning_preference_candidate_list::DESCRIPTION
            }
            Tool::LearningPreferenceCandidatePromote(_) => {
                learning_preference_candidate_promote::DESCRIPTION
            }
            Tool::LearningSelfEvolutionCreator(_) => learning_self_evolution_creator::DESCRIPTION,
            Tool::LearningSelfEvolutionDraftWrite(_) => {
                learning_self_evolution_draft_write::DESCRIPTION
            }
            Tool::LearningSelfEvolutionReport(_) => learning_self_evolution_report::DESCRIPTION,
            Tool::ExecutionLineageReport(_) => execution_lineage_report::DESCRIPTION,
            Tool::ExecutionRecordDetail(_) => execution_record_detail::DESCRIPTION,
            Tool::ExecutionRecordList(_) => execution_record_list::DESCRIPTION,
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
            Tool::EnterWorktree(_) => enter_worktree::DESCRIPTION,
            Tool::ExitWorktree(_) => exit_worktree::DESCRIPTION,
            Tool::Monitor(_) => monitor::DESCRIPTION,
            Tool::PushNotification(_) => push_notification::DESCRIPTION,
            Tool::TaskStop(_) => task_stop::DESCRIPTION,
            Tool::TaskOutput(_) => task_output::DESCRIPTION,
            Tool::ToolSearch(_) => tool_search::DESCRIPTION,
            Tool::TaskCreate(_) => task_create::DESCRIPTION,
            Tool::TaskGet(_) => task_get::DESCRIPTION,
            Tool::TaskList(_) => task_list::DESCRIPTION,
            Tool::TaskUpdate(_) => task_update::DESCRIPTION,
            Tool::Workflow(_) => workflow::DESCRIPTION,
            Tool::Recall(_) => recall::DESCRIPTION,
            Tool::CronCreate(_) => cron_create::DESCRIPTION,
            Tool::CronList(_) => cron_list::DESCRIPTION,
            Tool::CronDelete(_) => cron_delete::DESCRIPTION,
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
            Tool::Connector(args) => connector::ConnectorTool::execute(ctx, args).await?,
            Tool::TodoWrite(args) => todo_write::TodoWriteTool::execute(ctx, args).await?,
            Tool::NotebookEdit(args) => notebook_edit::NotebookEditTool::execute(ctx, args).await?,
            Tool::OperatorList(args) => operator_list::OperatorListTool::execute(ctx, args).await?,
            Tool::OperatorDescribe(args) => {
                operator_describe::OperatorDescribeTool::execute(ctx, args).await?
            }
            Tool::UnitList(args) => unit_list::UnitListTool::execute(ctx, args).await?,
            Tool::UnitSearch(args) => unit_search::UnitSearchTool::execute(ctx, args).await?,
            Tool::UnitDescribe(args) => unit_describe::UnitDescribeTool::execute(ctx, args).await?,
            Tool::UnitAuthoringValidate(args) => {
                unit_authoring_validate::UnitAuthoringValidateTool::execute(ctx, args).await?
            }
            Tool::TemplateExecute(args) => {
                template_execute::TemplateExecuteTool::execute(ctx, args).await?
            }
            Tool::EnvironmentProfileCheck(args) => {
                environment_profile_check::EnvironmentProfileCheckTool::execute(ctx, args).await?
            }
            Tool::EnvironmentProfilePreparePlan(args) => {
                environment_profile_prepare_plan::EnvironmentProfilePreparePlanTool::execute(
                    ctx, args,
                )
                .await?
            }
            Tool::ExecutionArchiveAdvisor(args) => {
                execution_archive_advisor::ExecutionArchiveAdvisorTool::execute(ctx, args).await?
            }
            Tool::ExecutionArchiveSuggestionWrite(args) => {
                execution_archive_suggestion_write::ExecutionArchiveSuggestionWriteTool::execute(
                    ctx, args,
                )
                .await?
            }
            Tool::LearningProposalList(args) => {
                learning_proposal_list::LearningProposalListTool::execute(ctx, args).await?
            }
            Tool::LearningProposalDecide(args) => {
                learning_proposal_decide::LearningProposalDecideTool::execute(ctx, args).await?
            }
            Tool::LearningProposalApply(args) => {
                learning_proposal_apply::LearningProposalApplyTool::execute(ctx, args).await?
            }
            Tool::LearningPreferenceCandidateList(args) => {
                learning_preference_candidate_list::LearningPreferenceCandidateListTool::execute(
                    ctx, args,
                )
                .await?
            }
            Tool::LearningPreferenceCandidatePromote(args) => {
                learning_preference_candidate_promote::LearningPreferenceCandidatePromoteTool::execute(
                    ctx, args,
                )
                .await?
            }
            Tool::LearningSelfEvolutionCreator(args) => {
                learning_self_evolution_creator::LearningSelfEvolutionCreatorTool::execute(
                    ctx, args,
                )
                .await?
            }
            Tool::LearningSelfEvolutionDraftWrite(args) => {
                learning_self_evolution_draft_write::LearningSelfEvolutionDraftWriteTool::execute(
                    ctx, args,
                )
                .await?
            }
            Tool::LearningSelfEvolutionReport(args) => {
                learning_self_evolution_report::LearningSelfEvolutionReportTool::execute(ctx, args)
                    .await?
            }
            Tool::ExecutionLineageReport(args) => {
                execution_lineage_report::ExecutionLineageReportTool::execute(ctx, args).await?
            }
            Tool::ExecutionRecordDetail(args) => {
                execution_record_detail::ExecutionRecordDetailTool::execute(ctx, args).await?
            }
            Tool::ExecutionRecordList(args) => {
                execution_record_list::ExecutionRecordListTool::execute(ctx, args).await?
            }
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
            Tool::EnterWorktree(args) => {
                enter_worktree::EnterWorktreeTool::execute(ctx, args).await?
            }
            Tool::ExitWorktree(args) => exit_worktree::ExitWorktreeTool::execute(ctx, args).await?,
            Tool::Monitor(args) => monitor::MonitorTool::execute(ctx, args).await?,
            Tool::PushNotification(args) => {
                push_notification::PushNotificationTool::execute(ctx, args).await?
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
            Tool::CronCreate(args) => cron_create::CronCreateTool::execute(ctx, args).await?,
            Tool::CronList(args) => cron_list::CronListTool::execute(ctx, args).await?,
            Tool::CronDelete(args) => cron_delete::CronDeleteTool::execute(ctx, args).await?,
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
            ToolKind::Connector => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid connector arguments: {}", e),
                })?;
                Ok(Tool::Connector(args))
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
            ToolKind::OperatorList => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid operator_list arguments: {}", e),
                })?;
                Ok(Tool::OperatorList(args))
            }
            ToolKind::OperatorDescribe => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid operator_describe arguments: {}", e),
                })?;
                Ok(Tool::OperatorDescribe(args))
            }
            ToolKind::UnitList => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid unit_list arguments: {}", e),
                })?;
                Ok(Tool::UnitList(args))
            }
            ToolKind::UnitSearch => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid unit_search arguments: {}", e),
                })?;
                Ok(Tool::UnitSearch(args))
            }
            ToolKind::UnitDescribe => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid unit_describe arguments: {}", e),
                })?;
                Ok(Tool::UnitDescribe(args))
            }
            ToolKind::UnitAuthoringValidate => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid unit_authoring_validate arguments: {}", e),
                })?;
                Ok(Tool::UnitAuthoringValidate(args))
            }
            ToolKind::TemplateExecute => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid template_execute arguments: {}", e),
                })?;
                Ok(Tool::TemplateExecute(args))
            }
            ToolKind::EnvironmentProfileCheck => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid environment_profile_check arguments: {}", e),
                })?;
                Ok(Tool::EnvironmentProfileCheck(args))
            }
            ToolKind::EnvironmentProfilePreparePlan => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid environment_profile_prepare_plan arguments: {}", e),
                })?;
                Ok(Tool::EnvironmentProfilePreparePlan(args))
            }
            ToolKind::ExecutionArchiveAdvisor => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid execution_archive_advisor arguments: {}", e),
                })?;
                Ok(Tool::ExecutionArchiveAdvisor(args))
            }
            ToolKind::ExecutionArchiveSuggestionWrite => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!(
                        "Invalid execution_archive_suggestion_write arguments: {}",
                        e
                    ),
                })?;
                Ok(Tool::ExecutionArchiveSuggestionWrite(args))
            }
            ToolKind::LearningProposalList => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid learning_proposal_list arguments: {}", e),
                })?;
                Ok(Tool::LearningProposalList(args))
            }
            ToolKind::LearningProposalDecide => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid learning_proposal_decide arguments: {}", e),
                })?;
                Ok(Tool::LearningProposalDecide(args))
            }
            ToolKind::LearningProposalApply => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid learning_proposal_apply arguments: {}", e),
                })?;
                Ok(Tool::LearningProposalApply(args))
            }
            ToolKind::LearningPreferenceCandidateList => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!(
                        "Invalid learning_preference_candidate_list arguments: {}",
                        e
                    ),
                })?;
                Ok(Tool::LearningPreferenceCandidateList(args))
            }
            ToolKind::LearningPreferenceCandidatePromote => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!(
                        "Invalid learning_preference_candidate_promote arguments: {}",
                        e
                    ),
                })?;
                Ok(Tool::LearningPreferenceCandidatePromote(args))
            }
            ToolKind::LearningSelfEvolutionCreator => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid learning_self_evolution_creator arguments: {e}"),
                })?;
                Ok(Tool::LearningSelfEvolutionCreator(args))
            }
            ToolKind::LearningSelfEvolutionDraftWrite => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid learning_self_evolution_draft_write arguments: {e}"),
                })?;
                Ok(Tool::LearningSelfEvolutionDraftWrite(args))
            }
            ToolKind::LearningSelfEvolutionReport => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid learning_self_evolution_report arguments: {}", e),
                })?;
                Ok(Tool::LearningSelfEvolutionReport(args))
            }
            ToolKind::ExecutionLineageReport => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid execution_lineage_report arguments: {}", e),
                })?;
                Ok(Tool::ExecutionLineageReport(args))
            }
            ToolKind::ExecutionRecordDetail => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid execution_record_detail arguments: {}", e),
                })?;
                Ok(Tool::ExecutionRecordDetail(args))
            }
            ToolKind::ExecutionRecordList => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid execution_record_list arguments: {}", e),
                })?;
                Ok(Tool::ExecutionRecordList(args))
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
            ToolKind::EnterWorktree => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid EnterWorktree arguments: {}", e),
                })?;
                Ok(Tool::EnterWorktree(args))
            }
            ToolKind::ExitWorktree => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid ExitWorktree arguments: {}", e),
                })?;
                Ok(Tool::ExitWorktree(args))
            }
            ToolKind::Monitor => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid Monitor arguments: {}", e),
                })?;
                Ok(Tool::Monitor(args))
            }
            ToolKind::PushNotification => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid PushNotification arguments: {}", e),
                })?;
                Ok(Tool::PushNotification(args))
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
            ToolKind::CronCreate => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid CronCreate arguments: {}", e),
                })?;
                Ok(Tool::CronCreate(args))
            }
            ToolKind::CronList => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid CronList arguments: {}", e),
                })?;
                Ok(Tool::CronList(args))
            }
            ToolKind::CronDelete => {
                let args = serde_json::from_str(json).map_err(|e| ToolError::InvalidArguments {
                    message: format!("Invalid CronDelete arguments: {}", e),
                })?;
                Ok(Tool::CronDelete(args))
            }
        }
    }

    /// Parse tool from tool name string and JSON arguments
    pub fn from_json_str(tool_name: &str, json: &str) -> Result<Self, ToolError> {
        let normalized_tool_name = normalize_legacy_retrieval_tool_name(tool_name);
        let normalized_json =
            normalize_legacy_retrieval_tool_arguments(tool_name, &normalized_tool_name, json);
        let kind = match normalized_tool_name.as_str() {
            "bash" => ToolKind::Bash,
            "file_edit" => ToolKind::FileEdit,
            "file_read" => ToolKind::FileRead,
            "file_write" => ToolKind::FileWrite,
            "ripgrep" | "Ripgrep" | "grep" | "Grep" => ToolKind::Grep,
            "glob" => ToolKind::Glob,
            "fetch" => ToolKind::Fetch,
            "query" => ToolKind::Query,
            "search" => ToolKind::Search,
            "connector" | "Connector" => ToolKind::Connector,
            "todo_write" => ToolKind::TodoWrite,
            "notebook_edit" => ToolKind::NotebookEdit,
            "operator_list" | "OperatorList" => ToolKind::OperatorList,
            "operator_describe" | "OperatorDescribe" => ToolKind::OperatorDescribe,
            "unit_list" | "UnitList" => ToolKind::UnitList,
            "unit_search" | "UnitSearch" => ToolKind::UnitSearch,
            "unit_describe" | "UnitDescribe" => ToolKind::UnitDescribe,
            "unit_authoring_validate" | "UnitAuthoringValidate" => ToolKind::UnitAuthoringValidate,
            "template_execute" | "TemplateExecute" => ToolKind::TemplateExecute,
            "environment_profile_check" | "EnvironmentProfileCheck" => {
                ToolKind::EnvironmentProfileCheck
            }
            "environment_profile_prepare_plan" | "EnvironmentProfilePreparePlan" => {
                ToolKind::EnvironmentProfilePreparePlan
            }
            "execution_archive_advisor" | "ExecutionArchiveAdvisor" => {
                ToolKind::ExecutionArchiveAdvisor
            }
            "execution_archive_suggestion_write" | "ExecutionArchiveSuggestionWrite" => {
                ToolKind::ExecutionArchiveSuggestionWrite
            }
            "learning_proposal_list" | "LearningProposalList" => ToolKind::LearningProposalList,
            "learning_proposal_decide" | "LearningProposalDecide" => {
                ToolKind::LearningProposalDecide
            }
            "learning_proposal_apply" | "LearningProposalApply" => ToolKind::LearningProposalApply,
            "learning_preference_candidate_list" | "LearningPreferenceCandidateList" => {
                ToolKind::LearningPreferenceCandidateList
            }
            "learning_preference_candidate_promote" | "LearningPreferenceCandidatePromote" => {
                ToolKind::LearningPreferenceCandidatePromote
            }
            "learning_self_evolution_creator" | "LearningSelfEvolutionCreator" => {
                ToolKind::LearningSelfEvolutionCreator
            }
            "learning_self_evolution_draft_write" | "LearningSelfEvolutionDraftWrite" => {
                ToolKind::LearningSelfEvolutionDraftWrite
            }
            "learning_self_evolution_report" | "LearningSelfEvolutionReport" => {
                ToolKind::LearningSelfEvolutionReport
            }
            "execution_lineage_report" | "ExecutionLineageReport" => {
                ToolKind::ExecutionLineageReport
            }
            "execution_record_detail" | "ExecutionRecordDetail" => ToolKind::ExecutionRecordDetail,
            "execution_record_list" | "ExecutionRecordList" => ToolKind::ExecutionRecordList,
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
            "EnterWorktree" | "enter_worktree" => ToolKind::EnterWorktree,
            "ExitWorktree" | "exit_worktree" => ToolKind::ExitWorktree,
            "Monitor" | "monitor" => ToolKind::Monitor,
            "PushNotification" | "push_notification" => ToolKind::PushNotification,
            "TaskStop" | "task_stop" | "KillShell" => ToolKind::TaskStop,
            "TaskOutput" | "task_output" => ToolKind::TaskOutput,
            "ToolSearch" | "tool_search" => ToolKind::ToolSearch,
            "TaskCreate" | "task_create" => ToolKind::TaskCreate,
            "TaskGet" | "task_get" => ToolKind::TaskGet,
            "TaskList" | "task_list" => ToolKind::TaskList,
            "TaskUpdate" | "task_update" => ToolKind::TaskUpdate,
            "workflow" | "Workflow" => ToolKind::Workflow,
            "recall" | "Recall" => ToolKind::Recall,
            "CronCreate" | "cron_create" => ToolKind::CronCreate,
            "CronList" | "cron_list" => ToolKind::CronList,
            "CronDelete" | "cron_delete" => ToolKind::CronDelete,
            _ => {
                return Err(ToolError::UnknownTool {
                    name: tool_name.to_string(),
                })
            }
        };
        Self::from_json(kind, &normalized_json)
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
    /// Optional My NCBI API key. The same key is used for E-utilities and
    /// NCBI Datasets v2 when configured.
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
    "ncbi_datasets",
    "arrayexpress",
    "biosample",
];
pub const DEFAULT_QUERY_DATASET_SOURCE_IDS: &[&str] = &["geo", "ena"];

pub const QUERY_KNOWLEDGE_SOURCE_IDS: &[&str] = &[
    "ncbi_gene",
    "ensembl",
    "uniprot",
    "reactome",
    "gene_ontology",
    "msigdb",
    "kegg",
];
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
            return if retrieval_registry::category_ids().contains(&category.as_str()) {
                retrieval_registry::normalize_enabled_ids(
                    &category,
                    values,
                    RegistryEntryKind::Source,
                    false,
                )
            } else {
                retrieval_registry::normalize_unregistered_enabled_ids(values)
            };
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
        if let Some(values) = self.enabled_sources_by_category.as_ref() {
            out.extend(retrieval_registry::configured_extra_enabled_categories(
                values,
            ));
        }
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
    /// Override for public biological-data API roots.
    /// Defaults to official endpoints; tests use this to point tools at mock servers.
    pub data_api_base_urls: crate::domain::search::data::DataApiBaseUrls,
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
    /// Shared SQLite connection pool for tools that need direct DB access (e.g. cron tools).
    pub db: Option<sqlx::SqlitePool>,
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
            data_api_base_urls: crate::domain::search::data::DataApiBaseUrls::default(),
            web_use_proxy: true,
            web_search_engine: "ddg".to_string(),
            web_search_methods: vec!["ddg".to_string(), "google".to_string(), "bing".to_string()],
            env_store: None,
            skill_cache: None,
            skill_task_context: None,
            db: None,
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

    pub fn with_data_api_base_urls(
        mut self,
        urls: crate::domain::search::data::DataApiBaseUrls,
    ) -> Self {
        self.data_api_base_urls = urls;
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

    /// Attach the shared SQLite pool so cron tools (and others) can persist to the app DB.
    pub fn with_db(mut self, pool: Option<sqlx::SqlitePool>) -> Self {
        self.db = pool;
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
        connector::schema(),
        todo_write::schema(),
        operator_list::schema(),
        operator_describe::schema(),
        unit_list::schema(),
        unit_search::schema(),
        unit_describe::schema(),
        unit_authoring_validate::schema(),
        template_execute::schema(),
        environment_profile_check::schema(),
        environment_profile_prepare_plan::schema(),
        execution_archive_advisor::schema(),
        execution_archive_suggestion_write::schema(),
        learning_proposal_list::schema(),
        learning_proposal_decide::schema(),
        learning_proposal_apply::schema(),
        learning_preference_candidate_list::schema(),
        learning_preference_candidate_promote::schema(),
        learning_self_evolution_creator::schema(),
        learning_self_evolution_draft_write::schema(),
        learning_self_evolution_report::schema(),
        execution_lineage_report::schema(),
        execution_record_detail::schema(),
        execution_record_list::schema(),
        visualization::schema(),
        sleep::schema(),
        ask_user_question::schema(),
        list_mcp_resources::schema(),
        read_mcp_resource::schema(),
        agent::schema(),
        exit_plan_mode::schema(),
        enter_plan_mode::schema(),
        enter_worktree::schema(),
        exit_worktree::schema(),
        monitor::schema(),
        push_notification::schema(),
        task_stop::schema(),
        task_output::schema(),
        tool_search::schema(),
        task_create::schema(),
        task_get::schema(),
        task_list::schema(),
        task_update::schema(),
        cron_create::schema(),
        cron_list::schema(),
        cron_delete::schema(),
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

fn tool_schema_model_order(name: &str) -> (u8, u8) {
    match name {
        // Retrieval is the preferred model-facing path for information access.
        // Keep these before `bash` so models see the safer, typed tools first.
        "recall" => (0, 0),
        "search" => (0, 1),
        "query" => (0, 2),
        "fetch" => (0, 3),
        "tool_search" | "ToolSearch" => (0, 4),

        // Read-only repo/file discovery tools.
        "file_read" => (1, 0),
        "ripgrep" | "grep" => (1, 1),
        "glob" => (1, 2),
        "connector" => (1, 3),
        "operator_list" => (1, 4),
        "operator_describe" => (1, 5),
        "unit_list" => (1, 6),
        "unit_search" => (1, 7),
        "unit_describe" => (1, 8),
        "unit_authoring_validate" => (1, 9),
        "list_mcp_resources" => (1, 10),
        "read_mcp_resource" => (1, 11),
        "execution_record_list" => (1, 12),
        "execution_record_detail" => (1, 13),
        "execution_lineage_report" => (1, 14),
        "execution_archive_advisor" => (1, 15),
        "learning_proposal_list" => (1, 16),
        "learning_preference_candidate_list" => (1, 17),
        "environment_profile_check" => (1, 18),

        // Mutating tools.
        "file_edit" => (2, 0),
        "file_write" => (2, 1),
        "notebook_edit" => (2, 2),
        "todo_write" => (2, 3),
        "template_execute" => (2, 4),
        "learning_proposal_decide" => (2, 5),
        "learning_proposal_apply" => (2, 6),
        "learning_preference_candidate_promote" => (2, 7),
        "execution_archive_suggestion_write" => (2, 8),
        "environment_profile_prepare_plan" => (2, 9),
        "learning_self_evolution_report" => (2, 10),
        "learning_self_evolution_draft_write" => (2, 11),
        "learning_self_evolution_creator" => (2, 12),

        // Orchestration and app-specific tools.
        "agent" | "Agent" => (3, 0),
        "task_create" | "TaskCreate" => (3, 1),
        "task_get" | "TaskGet" => (3, 2),
        "task_list" | "TaskList" => (3, 3),
        "task_update" | "TaskUpdate" => (3, 4),
        "task_output" | "TaskOutput" => (3, 5),
        "task_stop" | "TaskStop" => (3, 6),
        "sleep" => (3, 7),
        "ask_user_question" | "AskUserQuestion" => (3, 8),
        "skill_manage" | "skill_config" | "list_skills" | "skill_view" | "Skill" => (3, 9),
        "workflow" | "Workflow" => (3, 10),
        "visualization" => (3, 11),
        "enter_plan_mode" | "EnterPlanMode" => (3, 12),
        "exit_plan_mode" | "ExitPlanMode" => (3, 13),
        "EnterWorktree" | "enter_worktree" => (3, 14),
        "ExitWorktree" | "exit_worktree" => (3, 15),
        "Monitor" | "monitor" => (3, 16),
        "PushNotification" | "push_notification" => (3, 17),
        "CronCreate" | "cron_create" => (3, 18),
        "CronList" | "cron_list" => (3, 19),
        "CronDelete" | "cron_delete" => (3, 20),

        // Shell is intentionally late: use it for terminal operations only
        // after dedicated tools are not appropriate.
        "bash" => (9, 0),

        _ => (8, 0),
    }
}

pub fn sort_tool_schemas_for_model(schemas: &mut [ToolSchema]) {
    schemas.sort_by(|a, b| {
        tool_schema_model_order(&a.name)
            .cmp(&tool_schema_model_order(&b.name))
            .then_with(|| a.name.cmp(&b.name))
    });
}

fn is_legacy_pubmed_search_tool(tool_name: &str) -> bool {
    matches!(
        tool_name.to_ascii_lowercase().as_str(),
        "mcp__pubmed__pubmed_search_articles"
            | "mcp__pubmed__pubmed_search"
            | "mcp__pubmed__search_articles"
            | "pubmed.pubmed_search_articles"
            | "pubmed.pubmed_search"
            | "pubmed_search_articles"
            | "pubmed_search"
    )
}

fn is_legacy_pubmed_fetch_tool(tool_name: &str) -> bool {
    matches!(
        tool_name.to_ascii_lowercase().as_str(),
        "mcp__pubmed__pubmed_fetch_article"
            | "mcp__pubmed__pubmed_fetch_articles"
            | "mcp__pubmed__pubmed_get_article"
            | "mcp__pubmed__fetch_article"
            | "pubmed.pubmed_fetch_article"
            | "pubmed.pubmed_fetch_articles"
            | "pubmed.pubmed_get_article"
            | "pubmed_fetch_article"
            | "pubmed_fetch_articles"
            | "pubmed_get_article"
    )
}

pub fn normalize_legacy_retrieval_tool_name(tool_name: &str) -> String {
    let lower = tool_name.to_ascii_lowercase();
    match lower.as_str() {
        "web_search" | "websearch" => "search".to_string(),
        "web_fetch" | "webfetch" => "fetch".to_string(),
        _ if is_legacy_pubmed_search_tool(tool_name) => "search".to_string(),
        _ if is_legacy_pubmed_fetch_tool(tool_name) => "fetch".to_string(),
        _ => tool_name.to_string(),
    }
}

fn json_string_field(
    object: &serde_json::Map<String, serde_json::Value>,
    keys: &[&str],
) -> Option<String> {
    keys.iter().find_map(|key| {
        object.get(*key).and_then(|value| match value {
            serde_json::Value::String(s) => {
                let trimmed = s.trim();
                (!trimmed.is_empty()).then(|| trimmed.to_string())
            }
            serde_json::Value::Number(n) => Some(n.to_string()),
            _ => None,
        })
    })
}

pub fn normalize_legacy_retrieval_tool_arguments(
    original_tool_name: &str,
    normalized_tool_name: &str,
    arguments: &str,
) -> String {
    if !matches!(normalized_tool_name, "search" | "fetch") {
        return arguments.to_string();
    }

    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(arguments) else {
        return arguments.to_string();
    };
    let Some(object) = value.as_object_mut() else {
        return arguments.to_string();
    };

    let is_pubmed_search = is_legacy_pubmed_search_tool(original_tool_name);
    let is_pubmed_fetch = is_legacy_pubmed_fetch_tool(original_tool_name);

    if is_pubmed_search || is_pubmed_fetch {
        object
            .entry("category".to_string())
            .or_insert_with(|| serde_json::json!("literature"));
        object
            .entry("source".to_string())
            .or_insert_with(|| serde_json::json!("pubmed"));

        if normalized_tool_name == "search" && !object.contains_key("query") {
            if let Some(query) = json_string_field(object, &["search_query", "q", "term", "title"])
            {
                object.insert("query".to_string(), serde_json::json!(query));
            }
        }
        if normalized_tool_name == "search" && !object.contains_key("max_results") {
            if let Some(value) = object
                .get("retmax")
                .or_else(|| object.get("limit"))
                .or_else(|| object.get("maxResults"))
                .cloned()
            {
                object.insert("max_results".to_string(), value);
            }
        }
        if normalized_tool_name == "fetch" && !object.contains_key("id") {
            if let Some(id) = json_string_field(
                object,
                &["pmid", "pubmed_id", "article_id", "accession", "uid"],
            ) {
                object.insert("id".to_string(), serde_json::json!(id));
            }
        }
    } else {
        object
            .entry("category".to_string())
            .or_insert_with(|| serde_json::json!("web"));
        if normalized_tool_name == "search" && !object.contains_key("query") {
            if let Some(query) = json_string_field(object, &["search_query", "q"]) {
                object.insert("query".to_string(), serde_json::json!(query));
            }
        }
    }

    serde_json::to_string(&value).unwrap_or_else(|_| arguments.to_string())
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
            | "connector"
            | "operator_list"
            | "operator_describe"
            | "unit_list"
            | "unit_search"
            | "unit_describe"
            | "unit_authoring_validate"
            | "execution_record_list"
            | "execution_record_detail"
            | "execution_lineage_report"
            | "execution_archive_advisor"
            | "environment_profile_check"
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
    use crate::domain::retrieval_registry::{self, RetrievalCapability};
    use std::collections::HashSet;

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

        let t = Tool::from_json_str("unit_list", r#"{"kind":"template"}"#).unwrap();
        assert!(matches!(t, Tool::UnitList(_)));

        let t = Tool::from_json_str("unit_search", r#"{"query":"diff","stage":"count"}"#).unwrap();
        assert!(matches!(t, Tool::UnitSearch(_)));

        let t = Tool::from_json_str("unit_describe", r#"{"id":"provider/template/demo"}"#).unwrap();
        assert!(matches!(t, Tool::UnitDescribe(_)));

        let t = Tool::from_json_str("unit_authoring_validate", r#"{"includeOk":true}"#).unwrap();
        assert!(matches!(t, Tool::UnitAuthoringValidate(_)));

        let t = Tool::from_json_str(
            "template_execute",
            r#"{"id":"demo","inputs":{},"params":{},"resources":{}}"#,
        )
        .unwrap();
        assert!(matches!(t, Tool::TemplateExecute(_)));

        let t = Tool::from_json_str("execution_record_list", r#"{"limit":5}"#).unwrap();
        assert!(matches!(t, Tool::ExecutionRecordList(_)));

        let t =
            Tool::from_json_str("execution_record_detail", r#"{"recordId":"execrec_1"}"#).unwrap();
        assert!(matches!(t, Tool::ExecutionRecordDetail(_)));

        let t = Tool::from_json_str("execution_lineage_report", r#"{"limit":5}"#).unwrap();
        assert!(matches!(t, Tool::ExecutionLineageReport(_)));

        let t = Tool::from_json_str("execution_archive_advisor", r#"{"limit":5}"#).unwrap();
        assert!(matches!(t, Tool::ExecutionArchiveAdvisor(_)));

        let t =
            Tool::from_json_str("execution_archive_suggestion_write", r#"{"limit":5}"#).unwrap();
        assert!(matches!(t, Tool::ExecutionArchiveSuggestionWrite(_)));

        let t = Tool::from_json_str("learning_proposal_list", r#"{"refresh":true}"#).unwrap();
        assert!(matches!(t, Tool::LearningProposalList(_)));

        let t = Tool::from_json_str(
            "learning_proposal_decide",
            r#"{"proposalId":"learn_1","decision":"approve"}"#,
        )
        .unwrap();
        assert!(matches!(t, Tool::LearningProposalDecide(_)));

        let t = Tool::from_json_str(
            "learning_proposal_apply",
            r#"{"proposalId":"learn_1","note":"confirmed"}"#,
        )
        .unwrap();
        assert!(matches!(t, Tool::LearningProposalApply(_)));

        let t = Tool::from_json_str(
            "learning_preference_candidate_list",
            r#"{"includePromoted":true}"#,
        )
        .unwrap();
        assert!(matches!(t, Tool::LearningPreferenceCandidateList(_)));

        let t = Tool::from_json_str(
            "learning_preference_candidate_promote",
            r#"{"candidateId":"pref_learn_1","note":"confirmed"}"#,
        )
        .unwrap();
        assert!(matches!(t, Tool::LearningPreferenceCandidatePromote(_)));

        let t = Tool::from_json_str("learning_self_evolution_report", r#"{"limit":5}"#).unwrap();
        assert!(matches!(t, Tool::LearningSelfEvolutionReport(_)));

        let t =
            Tool::from_json_str("learning_self_evolution_draft_write", r#"{"limit":5}"#).unwrap();
        assert!(matches!(t, Tool::LearningSelfEvolutionDraftWrite(_)));

        let t =
            Tool::from_json_str("learning_self_evolution_creator", r#"{"refresh":true}"#).unwrap();
        assert!(matches!(t, Tool::LearningSelfEvolutionCreator(_)));

        let t = Tool::from_json_str("environment_profile_check", r#"{"envRef":"r-bioc"}"#).unwrap();
        assert!(matches!(t, Tool::EnvironmentProfileCheck(_)));

        let t = Tool::from_json_str("environment_profile_prepare_plan", r#"{"envRef":"r-bioc"}"#)
            .unwrap();
        assert!(matches!(t, Tool::EnvironmentProfilePreparePlan(_)));
    }

    #[test]
    fn legacy_web_aliases_route_to_unified_tools() {
        let t = Tool::from_json_str("web_search", r#"{"query":"TP53 lung cancer"}"#).unwrap();
        match t {
            Tool::Search(args) => {
                assert_eq!(args.category, "web");
                assert_eq!(args.query, "TP53 lung cancer");
            }
            other => panic!("expected Search, got {:?}", other.kind()),
        }

        let t = Tool::from_json_str("web_fetch", r#"{"url":"https://example.com"}"#).unwrap();
        match t {
            Tool::Fetch(args) => {
                assert_eq!(args.category, "web");
                assert_eq!(args.url.as_deref(), Some("https://example.com"));
            }
            other => panic!("expected Fetch, got {:?}", other.kind()),
        }
    }

    #[test]
    fn legacy_pubmed_mcp_aliases_route_to_unified_retrieval_tools() {
        let t = Tool::from_json_str(
            "mcp__pubmed__pubmed_search_articles",
            r#"{"term":"BRCA2","retmax":3}"#,
        )
        .unwrap();
        match t {
            Tool::Search(args) => {
                assert_eq!(args.category, "literature");
                assert_eq!(args.source.as_deref(), Some("pubmed"));
                assert_eq!(args.query, "BRCA2");
                assert_eq!(args.max_results, Some(3));
            }
            other => panic!("expected Search, got {:?}", other.kind()),
        }

        let t =
            Tool::from_json_str("pubmed.pubmed_fetch_article", r#"{"pmid":"12345678"}"#).unwrap();
        match t {
            Tool::Fetch(args) => {
                assert_eq!(args.category, "literature");
                assert_eq!(args.source.as_deref(), Some("pubmed"));
                assert_eq!(args.id.as_deref(), Some("12345678"));
            }
            other => panic!("expected Fetch, got {:?}", other.kind()),
        }
    }

    #[test]
    fn model_tool_schema_order_prioritizes_retrieval_before_bash() {
        let mut schemas = all_tool_schemas(true);
        sort_tool_schemas_for_model(&mut schemas);
        let position = |name: &str| {
            schemas
                .iter()
                .position(|schema| schema.name == name)
                .unwrap_or_else(|| panic!("{name} schema should exist"))
        };

        let bash = position("bash");
        assert!(position("search") < bash);
        assert!(position("query") < bash);
        assert!(position("fetch") < bash);
        assert!(position("recall") < bash);
    }

    #[test]
    fn tool_schema_catalog_exposes_unified_retrieval_tools_only() {
        let names: HashSet<_> = all_tool_schemas(true)
            .into_iter()
            .map(|schema| schema.name)
            .collect();

        assert!(names.contains("search"));
        assert!(names.contains("query"));
        assert!(names.contains("fetch"));
        assert!(
            !names.contains("web_search"),
            "legacy web_search must remain an execution compatibility alias, not a model-visible schema"
        );
        assert!(
            !names.contains("web_fetch"),
            "legacy web_fetch must remain an execution compatibility alias, not a model-visible schema"
        );
    }

    #[test]
    fn query_settings_allowlists_cover_available_builtin_query_sources() {
        let dataset_allowed: HashSet<_> = QUERY_DATASET_SOURCE_IDS.iter().copied().collect();
        let knowledge_allowed: HashSet<_> = QUERY_KNOWLEDGE_SOURCE_IDS.iter().copied().collect();

        for source in retrieval_registry::registry().sources {
            if !source.can_execute() || !source.supports(RetrievalCapability::Query) {
                continue;
            }
            match source.category {
                "dataset" => assert!(
                    dataset_allowed.contains(source.id),
                    "dataset query source `{}` is executable in registry but missing from QUERY_DATASET_SOURCE_IDS",
                    source.id
                ),
                "knowledge" => assert!(
                    knowledge_allowed.contains(source.id),
                    "knowledge query source `{}` is executable in registry but missing from QUERY_KNOWLEDGE_SOURCE_IDS",
                    source.id
                ),
                _ => {}
            }
        }
    }

    #[test]
    fn query_settings_allowlists_do_not_expose_unregistered_sources() {
        for id in QUERY_DATASET_SOURCE_IDS {
            let source = retrieval_registry::find_source("dataset", id).unwrap_or_else(|| {
                panic!("dataset query allowlist source `{id}` is not registered")
            });
            assert!(
                source.can_execute() && source.supports(RetrievalCapability::Query),
                "dataset query allowlist source `{id}` is not executable/query-capable"
            );
        }
        for id in QUERY_KNOWLEDGE_SOURCE_IDS {
            let source = retrieval_registry::find_source("knowledge", id).unwrap_or_else(|| {
                panic!("knowledge query allowlist source `{id}` is not registered")
            });
            assert!(
                source.can_execute() && source.supports(RetrievalCapability::Query),
                "knowledge query allowlist source `{id}` is not executable/query-capable"
            );
        }
    }
}
