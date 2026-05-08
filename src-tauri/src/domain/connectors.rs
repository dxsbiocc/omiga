//! Omiga connector catalog and connection-state management.
//!
//! Connectors are user-level, user-authorized external service links (GitHub, Linear,
//! Figma, etc.).
//! They are intentionally separate from plugins: plugins package skills/MCP/retrieval/UI metadata,
//! while connectors track whether a user has enabled or authenticated a specific outside service.
//! This module stores only user-level connector state and metadata; it never stores secret tokens.

pub(crate) mod http;
pub(crate) mod oauth;
pub(crate) mod secret_store;

use self::http::{ConnectorHttpError, ConnectorHttpRequest};
use crate::domain::mcp::client::list_tools_for_server;
use crate::domain::mcp::config::merged_mcp_servers;
use crate::domain::mcp::names::{build_mcp_tool_name, normalize_name_for_mcp};
use crate::domain::plugins;
use base64::Engine;
use chrono::Utc;
use reqwest::Method;
use rmcp::model::Tool as McpTool;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

pub use self::oauth::{
    ConnectorLoginPollResult, ConnectorLoginPollStatus, ConnectorLoginStartResult,
};

const CONNECTORS_CONFIG_FILE: &str = "connectors/config.json";
const CONNECTOR_AUDIT_FILE: &str = "audit.jsonl";
const CONNECTOR_TEST_HISTORY_LIMIT: usize = 20;
const CONNECTOR_AUDIT_DEFAULT_LIMIT: usize = 100;
const CONNECTOR_AUDIT_MAX_LIMIT: usize = 500;
const GITHUB_CLI_AUTH_TIMEOUT: Duration = Duration::from_secs(2);
const CODEX_APPS_MCP_SERVER_NAME: &str = "codex_apps";
const MCP_CONNECTOR_BRIDGE_TIMEOUT: Duration = Duration::from_secs(5);
const MAIL_ADDRESS_SECRET: &str = "mail_address";
const MAIL_AUTHORIZATION_CODE_SECRET: &str = "mail_authorization_code";
const GMAIL_OAUTH_ENV_VARS: &[&str] = &["GMAIL_ACCESS_TOKEN", "GOOGLE_OAUTH_ACCESS_TOKEN"];
const GMAIL_CONNECTOR_ENV_VARS: &[&str] = &[
    "GMAIL_ACCESS_TOKEN",
    "GOOGLE_OAUTH_ACCESS_TOKEN",
    "GMAIL_ADDRESS",
    "GMAIL_USERNAME",
    "GMAIL_APP_PASSWORD",
    "GMAIL_AUTH_CODE",
];
const NOTION_TOKEN_ENV_VARS: &[&str] = &["NOTION_TOKEN", "NOTION_API_KEY"];
const SLACK_TOKEN_ENV_VARS: &[&str] = &["SLACK_BOT_TOKEN"];

#[cfg(test)]
pub(crate) static CONNECTOR_TEST_ENV_LOCK: tokio::sync::Mutex<()> =
    tokio::sync::Mutex::const_new(());

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ConnectorAuthType {
    /// No credential is required to use the connector metadata.
    None,
    /// The connector is backed by a user-provided environment variable token.
    EnvToken,
    /// OAuth is the intended UX, but native OAuth flow is not wired yet.
    OAuth,
    /// API-key style auth; Omiga should still keep the key outside this config file.
    ApiKey,
    /// A plugin declared this connector, but Omiga does not know a first-class auth flow yet.
    ExternalMcp,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorToolDefinition {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub read_only: bool,
    #[serde(default)]
    pub required_scopes: Vec<String>,
    #[serde(default)]
    pub confirmation_required: bool,
    #[serde(default = "default_tool_execution")]
    pub execution: ConnectorToolExecution,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorToolExecution {
    /// The built-in `connector` tool can execute this operation today.
    Native,
    /// The operation is product metadata until a native/MCP/plugin executor exists.
    Declared,
    /// The operation is expected to be supplied by an external MCP/plugin runtime.
    ExternalMcp,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorDefinition {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub auth_type: ConnectorAuthType,
    #[serde(default)]
    pub env_vars: Vec<String>,
    pub install_url: Option<String>,
    pub docs_url: Option<String>,
    #[serde(default = "default_enabled")]
    pub default_enabled: bool,
    #[serde(default)]
    pub tools: Vec<ConnectorToolDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorDefinitionSource {
    BuiltIn,
    Custom,
    Plugin,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorConnectionStatus {
    Connected,
    NeedsAuth,
    Disabled,
    MetadataOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorInfo {
    pub definition: ConnectorDefinition,
    pub enabled: bool,
    pub connected: bool,
    pub accessible: bool,
    pub status: ConnectorConnectionStatus,
    pub account_label: Option<String>,
    pub auth_source: Option<String>,
    pub connected_at: Option<String>,
    pub env_configured: bool,
    pub referenced_by_plugins: Vec<String>,
    pub source: ConnectorDefinitionSource,
    pub last_connection_test: Option<ConnectorConnectionTestResult>,
    pub connection_test_history: Vec<ConnectorConnectionTestResult>,
    pub connection_health: ConnectorHealthSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorCatalog {
    pub connectors: Vec<ConnectorInfo>,
    pub scope: String,
    pub config_path: String,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorConnectRequest {
    pub connector_id: String,
    #[serde(default)]
    pub account_label: Option<String>,
    #[serde(default)]
    pub auth_source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MailConnectorCredentialRequest {
    pub connector_id: String,
    pub email_address: String,
    pub authorization_code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CustomConnectorRequest {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub auth_type: ConnectorAuthType,
    #[serde(default)]
    pub env_vars: Vec<String>,
    pub install_url: Option<String>,
    pub docs_url: Option<String>,
    #[serde(default = "default_enabled")]
    pub default_enabled: bool,
    #[serde(default)]
    pub tools: Vec<ConnectorToolDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CustomConnectorExport {
    pub version: u32,
    pub scope: String,
    pub connectors: Vec<ConnectorDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CustomConnectorImportRequest {
    #[serde(default)]
    pub connectors: Vec<CustomConnectorRequest>,
    #[serde(default)]
    pub replace_existing: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorConnectionTestKind {
    /// No network call was made; the result reflects user-level connector state only.
    LocalState,
    /// A live API endpoint was called by native connector code.
    NativeApi,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorConnectionTestResult {
    pub connector_id: String,
    pub connector_name: String,
    pub ok: bool,
    pub status: ConnectorConnectionStatus,
    pub check_kind: ConnectorConnectionTestKind,
    pub message: String,
    pub checked_at: String,
    pub account_label: Option<String>,
    pub auth_source: Option<String>,
    pub http_status: Option<u16>,
    #[serde(default)]
    pub retryable: bool,
    pub error_code: Option<String>,
    pub details: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorHealthSummary {
    pub total_checks: usize,
    pub ok_checks: usize,
    pub failed_checks: usize,
    pub retryable_failures: usize,
    pub last_ok_at: Option<String>,
    pub last_failure_at: Option<String>,
    pub last_error_code: Option<String>,
    pub last_http_status: Option<u16>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorAuditAccess {
    Read,
    Write,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorAuditOutcome {
    Ok,
    Error,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorAuditEvent {
    pub id: String,
    pub connector_id: String,
    pub operation: String,
    pub access: ConnectorAuditAccess,
    pub confirmation_required: bool,
    pub confirmed: bool,
    pub target: Option<String>,
    pub session_id: Option<String>,
    pub project_root: Option<String>,
    pub outcome: ConnectorAuditOutcome,
    pub error_code: Option<String>,
    pub message: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorAuditRecordRequest {
    pub connector_id: String,
    pub operation: String,
    pub access: ConnectorAuditAccess,
    #[serde(default)]
    pub confirmation_required: bool,
    #[serde(default)]
    pub confirmed: bool,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub project_root: Option<String>,
    pub outcome: ConnectorAuditOutcome,
    #[serde(default)]
    pub error_code: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConnectorPermissionAuditIntent {
    pub connector_id: String,
    pub operation: String,
    pub access: ConnectorAuditAccess,
    pub confirmation_required: bool,
    pub confirmed: bool,
    pub target: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct ConnectorConfigFile {
    #[serde(default)]
    connectors: HashMap<String, ConnectorConfigEntry>,
    /// User-specific first-class connector definitions. This keeps the model open for future
    /// marketplace sync without requiring a schema migration.
    #[serde(default)]
    custom_connectors: Vec<ConnectorDefinition>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct ConnectorConfigEntry {
    #[serde(default = "default_enabled")]
    enabled: bool,
    #[serde(default)]
    connected: bool,
    #[serde(default)]
    account_label: Option<String>,
    #[serde(default)]
    auth_source: Option<String>,
    #[serde(default)]
    connected_at: Option<String>,
    /// False means the user explicitly disconnected this connector even if matching env vars exist.
    #[serde(default = "default_use_env_credentials")]
    use_env_credentials: bool,
    #[serde(default)]
    last_connection_test: Option<ConnectorConnectionTestResult>,
    #[serde(default)]
    connection_test_history: Vec<ConnectorConnectionTestResult>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedConnectorDefinition {
    definition: ConnectorDefinition,
    source: ConnectorDefinitionSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConnectorCredentialSource {
    Environment,
    OAuthDevice,
    OAuthBrowser,
    GitHubCli,
    MailCredentials,
}

impl ConnectorCredentialSource {
    fn auth_source(self) -> &'static str {
        match self {
            ConnectorCredentialSource::Environment => "environment",
            ConnectorCredentialSource::OAuthDevice => "oauth_device",
            ConnectorCredentialSource::OAuthBrowser => "oauth_browser",
            ConnectorCredentialSource::GitHubCli => "github_cli",
            ConnectorCredentialSource::MailCredentials => "mail_credentials",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ConnectorAppBridge {
    connector_name: Option<String>,
    connector_description: Option<String>,
    tool_names: Vec<String>,
}

fn default_enabled() -> bool {
    true
}

fn default_use_env_credentials() -> bool {
    true
}

fn default_tool_execution() -> ConnectorToolExecution {
    ConnectorToolExecution::Declared
}

fn omiga_home() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".omiga")
}

fn connector_config_path() -> PathBuf {
    if let Some(path) = std::env::var("OMIGA_CONNECTORS_CONFIG_PATH")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        return PathBuf::from(path);
    }
    omiga_home().join(CONNECTORS_CONFIG_FILE)
}

fn connector_audit_path() -> PathBuf {
    if let Some(path) = std::env::var("OMIGA_CONNECTOR_AUDIT_PATH")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        return PathBuf::from(path);
    }
    connector_config_path()
        .parent()
        .map(|parent| parent.join(CONNECTOR_AUDIT_FILE))
        .unwrap_or_else(|| omiga_home().join("connectors").join(CONNECTOR_AUDIT_FILE))
}

fn normalize_connector_id(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .replace([' ', '/'], "_")
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '_' || *ch == '-' || *ch == '.')
        .collect()
}

fn validate_connector_id(value: &str) -> Result<String, String> {
    let normalized = normalize_connector_id(value);
    if normalized.is_empty() || normalized == "." || normalized.contains("..") {
        return Err(format!("invalid connector id `{value}`"));
    }
    Ok(normalized)
}

fn connector_tool(name: &str, description: &str, read_only: bool) -> ConnectorToolDefinition {
    ConnectorToolDefinition {
        name: name.to_string(),
        description: description.to_string(),
        read_only,
        required_scopes: Vec::new(),
        confirmation_required: !read_only,
        execution: ConnectorToolExecution::Declared,
    }
}

fn email_connector_tools() -> Vec<ConnectorToolDefinition> {
    vec![
        connector_tool(
            "search_messages",
            "Search mailbox messages by query, sender, subject, or date.",
            true,
        ),
        connector_tool(
            "read_message",
            "Read message headers, body preview, and attachments metadata.",
            true,
        ),
        connector_tool(
            "send_message",
            "Send an email message after explicit user confirmation.",
            false,
        ),
    ]
}

fn native_connector_tool(
    name: &str,
    description: &str,
    read_only: bool,
    required_scopes: &[&str],
    confirmation_required: bool,
) -> ConnectorToolDefinition {
    ConnectorToolDefinition {
        name: name.to_string(),
        description: description.to_string(),
        read_only,
        required_scopes: required_scopes
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
        confirmation_required,
        execution: ConnectorToolExecution::Native,
    }
}

#[allow(clippy::too_many_arguments)]
fn connector(
    id: &str,
    name: &str,
    description: &str,
    category: &str,
    auth_type: ConnectorAuthType,
    env_vars: &[&str],
    install_url: Option<&str>,
    docs_url: Option<&str>,
    tools: Vec<ConnectorToolDefinition>,
) -> ConnectorDefinition {
    ConnectorDefinition {
        id: id.to_string(),
        name: name.to_string(),
        description: description.to_string(),
        category: category.to_string(),
        auth_type,
        env_vars: env_vars.iter().map(|value| (*value).to_string()).collect(),
        install_url: install_url.map(str::to_string),
        docs_url: docs_url.map(str::to_string),
        default_enabled: true,
        tools,
    }
}

pub fn builtin_connector_definitions() -> Vec<ConnectorDefinition> {
    vec![
        connector(
            "github",
            "GitHub",
            "Read issues and pull requests, inspect repository metadata, and prepare PR updates.",
            "code",
            ConnectorAuthType::OAuth,
            &["GITHUB_TOKEN", "GH_TOKEN"],
            Some("https://github.com/settings/developers"),
            Some("https://docs.github.com/rest"),
            vec![
                native_connector_tool(
                    "list_issues",
                    "List and filter repository issues.",
                    true,
                    &["repo or public_repo for private repositories"],
                    false,
                ),
                native_connector_tool(
                    "read_issue",
                    "Read issue metadata and body.",
                    true,
                    &["repo or public_repo for private repositories"],
                    false,
                ),
                native_connector_tool(
                    "list_pull_requests",
                    "List repository pull requests.",
                    true,
                    &["repo or public_repo for private repositories"],
                    false,
                ),
                native_connector_tool(
                    "read_pull_request",
                    "Read pull request metadata and body.",
                    true,
                    &["repo or public_repo for private repositories"],
                    false,
                ),
            ],
        ),
        connector(
            "gitlab",
            "GitLab",
            "Read GitLab issues and merge requests and synchronize implementation status.",
            "code",
            ConnectorAuthType::EnvToken,
            &["GITLAB_TOKEN"],
            Some("https://gitlab.com/-/user_settings/personal_access_tokens"),
            Some("https://docs.gitlab.com/api/"),
            vec![
                native_connector_tool(
                    "list_issues",
                    "List project issues.",
                    true,
                    &["read_api"],
                    false,
                ),
                native_connector_tool(
                    "read_issue",
                    "Read issue metadata and description.",
                    true,
                    &["read_api"],
                    false,
                ),
                native_connector_tool(
                    "list_merge_requests",
                    "List project merge requests.",
                    true,
                    &["read_api"],
                    false,
                ),
                native_connector_tool(
                    "read_merge_request",
                    "Read merge request details.",
                    true,
                    &["read_api"],
                    false,
                ),
            ],
        ),
        connector(
            "bitbucket",
            "Bitbucket",
            "Read Bitbucket repositories, issues, and pull requests for code review context.",
            "code",
            ConnectorAuthType::ApiKey,
            &["BITBUCKET_TOKEN", "BITBUCKET_APP_PASSWORD"],
            Some("https://bitbucket.org/account/settings/app-passwords/"),
            Some("https://developer.atlassian.com/cloud/bitbucket/rest/"),
            vec![
                connector_tool("list_pull_requests", "List repository pull requests.", true),
                connector_tool("read_pull_request", "Read pull request metadata.", true),
                connector_tool("list_issues", "List repository issues.", true),
            ],
        ),
        connector(
            "azure_devops",
            "Azure DevOps",
            "Read Azure Boards work items and Azure Repos pull requests.",
            "code",
            ConnectorAuthType::ApiKey,
            &["AZURE_DEVOPS_PAT", "AZDO_PERSONAL_ACCESS_TOKEN"],
            Some("https://dev.azure.com/"),
            Some("https://learn.microsoft.com/rest/api/azure/devops/"),
            vec![
                connector_tool("list_work_items", "List Azure Boards work items.", true),
                connector_tool("read_work_item", "Read a work item.", true),
                connector_tool("read_pull_request", "Read Azure Repos pull request details.", true),
            ],
        ),
        connector(
            "linear",
            "Linear",
            "Read and update product/engineering issues from Linear.",
            "project_management",
            ConnectorAuthType::EnvToken,
            &["LINEAR_API_KEY", "LINEAR_ACCESS_TOKEN"],
            Some("https://linear.app/settings/api"),
            Some("https://developers.linear.app/docs/graphql/working-with-the-graphql-api"),
            vec![
                native_connector_tool("list_issues", "List Linear issues.", true, &["read"], false),
                native_connector_tool("read_issue", "Read a Linear issue.", true, &["read"], false),
                connector_tool(
                    "update_issue_status",
                    "Move a Linear issue through workflow states.",
                    false,
                ),
            ],
        ),
        connector(
            "asana",
            "Asana",
            "Read and update Asana tasks for product and execution tracking.",
            "project_management",
            ConnectorAuthType::EnvToken,
            &["ASANA_ACCESS_TOKEN"],
            Some("https://app.asana.com/0/my-apps"),
            Some("https://developers.asana.com/docs"),
            vec![
                connector_tool("list_tasks", "List project or workspace tasks.", true),
                connector_tool("read_task", "Read task metadata and notes.", true),
                connector_tool("update_task", "Update task status or fields.", false),
            ],
        ),
        connector(
            "trello",
            "Trello",
            "Read and update Trello boards, lists, and cards.",
            "project_management",
            ConnectorAuthType::ApiKey,
            &["TRELLO_API_KEY", "TRELLO_TOKEN"],
            Some("https://trello.com/power-ups/admin"),
            Some("https://developer.atlassian.com/cloud/trello/rest/"),
            vec![
                connector_tool("list_cards", "List cards on a board or list.", true),
                connector_tool("read_card", "Read card details.", true),
                connector_tool("move_card", "Move a card between lists.", false),
            ],
        ),
        connector(
            "confluence",
            "Confluence",
            "Search and read team documentation from Confluence spaces.",
            "knowledge",
            ConnectorAuthType::ApiKey,
            &[
                "CONFLUENCE_SITE_URL",
                "CONFLUENCE_EMAIL",
                "CONFLUENCE_API_TOKEN",
                "ATLASSIAN_SITE_URL",
                "ATLASSIAN_EMAIL",
                "ATLASSIAN_API_TOKEN",
            ],
            Some("https://id.atlassian.com/manage-profile/security/api-tokens"),
            Some("https://developer.atlassian.com/cloud/confluence/rest/v2/"),
            vec![
                connector_tool("search_pages", "Search Confluence pages.", true),
                connector_tool("read_page", "Read page content and metadata.", true),
            ],
        ),
        connector(
            "jira",
            "Jira",
            "Read Jira tickets and keep execution status aligned with project tracking.",
            "project_management",
            ConnectorAuthType::ApiKey,
            &[
                "JIRA_SITE_URL",
                "JIRA_EMAIL",
                "JIRA_API_TOKEN",
                "ATLASSIAN_SITE_URL",
                "ATLASSIAN_EMAIL",
                "ATLASSIAN_API_TOKEN",
            ],
            Some("https://id.atlassian.com/manage-profile/security/api-tokens"),
            Some("https://developer.atlassian.com/cloud/jira/platform/rest/v3/"),
            vec![
                connector_tool("read_issue", "Read a Jira issue.", true),
                connector_tool("transition_issue", "Transition a Jira issue.", false),
            ],
        ),
        connector(
            "discord",
            "Discord",
            "Read channels and post execution updates to Discord communities or teams.",
            "communication",
            ConnectorAuthType::EnvToken,
            &["DISCORD_BOT_TOKEN"],
            Some("https://discord.com/developers/applications"),
            Some("https://discord.com/developers/docs/intro"),
            vec![
                connector_tool("read_channel", "Read recent channel messages.", true),
                connector_tool("post_message", "Post a message to a channel.", false),
            ],
        ),
        connector(
            "slack",
            "Slack",
            "Read team discussion context and post concise execution updates.",
            "communication",
            ConnectorAuthType::OAuth,
            &["SLACK_BOT_TOKEN"],
            Some("https://api.slack.com/apps"),
            Some("https://api.slack.com/methods"),
            vec![
                native_connector_tool(
                    "read_thread",
                    "Read a Slack thread.",
                    true,
                    &["channels:read", "channels:history"],
                    false,
                ),
                native_connector_tool(
                    "post_message",
                    "Post a message to a channel or thread.",
                    false,
                    &["chat:write"],
                    true,
                ),
            ],
        ),
        connector(
            "microsoft_teams",
            "Microsoft Teams",
            "Use Teams channels/chats for coordination context and status updates.",
            "communication",
            ConnectorAuthType::OAuth,
            &["MS_GRAPH_TOKEN", "TEAMS_WEBHOOK_URL"],
            Some("https://portal.azure.com/#view/Microsoft_AAD_RegisteredApps/ApplicationsListBlade"),
            Some("https://learn.microsoft.com/graph/api/resources/teams-api-overview"),
            vec![
                connector_tool("read_channel", "Read channel messages via Microsoft Graph.", true),
                connector_tool("post_message", "Post status updates to Teams.", false),
            ],
        ),
        connector(
            "figma",
            "Figma",
            "Fetch design context for UI implementation and visual QA.",
            "design",
            ConnectorAuthType::EnvToken,
            &["FIGMA_TOKEN", "FIGMA_ACCESS_TOKEN"],
            Some("https://www.figma.com/developers/api#access-tokens"),
            Some("https://www.figma.com/developers/api"),
            vec![
                connector_tool("get_file", "Read Figma file metadata.", true),
                connector_tool(
                    "get_design_context",
                    "Extract implementation context for selected frames.",
                    true,
                ),
            ],
        ),
        connector(
            "sentry",
            "Sentry",
            "Read Sentry issues, events, and release health to debug production failures.",
            "observability",
            ConnectorAuthType::EnvToken,
            &["SENTRY_AUTH_TOKEN"],
            Some("https://sentry.io/settings/account/api/auth-tokens/"),
            Some("https://docs.sentry.io/api/"),
            vec![
                native_connector_tool(
                    "list_issues",
                    "List Sentry issues.",
                    true,
                    &["event:read", "project:read"],
                    false,
                ),
                native_connector_tool(
                    "read_issue",
                    "Read issue details and latest events.",
                    true,
                    &["event:read", "project:read"],
                    false,
                ),
                connector_tool("resolve_issue", "Resolve or assign an issue.", false),
            ],
        ),
        connector(
            "notion",
            "Notion",
            "Use workspace documentation and product notes as implementation context.",
            "knowledge",
            ConnectorAuthType::OAuth,
            &["NOTION_TOKEN"],
            Some("https://www.notion.so/my-integrations"),
            Some("https://developers.notion.com/reference/intro"),
            vec![
                native_connector_tool(
                    "search_pages",
                    "Search Notion pages.",
                    true,
                    &["read_content"],
                    false,
                ),
                native_connector_tool(
                    "read_page",
                    "Read Notion page metadata and content blocks.",
                    true,
                    &["read_content"],
                    false,
                ),
            ],
        ),
        connector(
            "dropbox",
            "Dropbox",
            "Search and read shared files from Dropbox for implementation context.",
            "knowledge",
            ConnectorAuthType::OAuth,
            &["DROPBOX_ACCESS_TOKEN"],
            Some("https://www.dropbox.com/developers/apps"),
            Some("https://www.dropbox.com/developers/documentation/http/overview"),
            vec![
                connector_tool("search_files", "Search Dropbox files.", true),
                connector_tool("read_file", "Read file metadata or content.", true),
            ],
        ),
        connector(
            "google_calendar",
            "Google Calendar",
            "Access calendar context for scheduling and status workflows.",
            "productivity",
            ConnectorAuthType::OAuth,
            &[],
            Some("https://calendar.google.com/"),
            Some("https://developers.google.com/calendar/api"),
            vec![connector_tool("list_events", "List calendar events.", true)],
        ),
        connector(
            "gmail",
            "Gmail",
            "Use Gmail messages through browser OAuth, with Google app passwords as a local fallback.",
            "email",
            ConnectorAuthType::OAuth,
            GMAIL_CONNECTOR_ENV_VARS,
            Some("https://mail.google.com/"),
            Some("https://support.google.com/accounts/answer/185833"),
            email_connector_tools(),
        ),
        connector(
            "qq_mail",
            "QQ 邮箱",
            "Use QQ Mail messages through provider authorization or IMAP/SMTP app passwords.",
            "email",
            ConnectorAuthType::ApiKey,
            &[
                "QQ_MAIL_ADDRESS",
                "QQ_MAIL_USERNAME",
                "QQ_MAIL_AUTH_CODE",
                "QQ_MAIL_APP_PASSWORD",
            ],
            Some("https://mail.qq.com/"),
            Some("https://service.mail.qq.com/"),
            email_connector_tools(),
        ),
        connector(
            "netease_mail",
            "网易邮箱",
            "Use NetEase Mail messages from 163/126/yeah.net accounts via authorization codes.",
            "email",
            ConnectorAuthType::ApiKey,
            &[
                "NETEASE_MAIL_ADDRESS",
                "NETEASE_MAIL_USERNAME",
                "NETEASE_MAIL_AUTH_CODE",
                "NETEASE_MAIL_APP_PASSWORD",
            ],
            Some("https://mail.163.com/"),
            Some("https://help.mail.163.com/"),
            email_connector_tools(),
        ),
        connector(
            "outlook",
            "Outlook",
            "Use Microsoft Outlook mail and calendar context for support and coordination workflows.",
            "email",
            ConnectorAuthType::OAuth,
            &["MS_GRAPH_TOKEN"],
            Some("https://portal.azure.com/#view/Microsoft_AAD_RegisteredApps/ApplicationsListBlade"),
            Some("https://learn.microsoft.com/graph/outlook-mail-concept-overview"),
            vec![
                connector_tool("search_messages", "Search Outlook mail.", true),
                connector_tool("read_message", "Read Outlook mail details.", true),
                connector_tool("send_message", "Send Outlook mail after explicit confirmation.", false),
                connector_tool("list_events", "List Outlook calendar events.", true),
            ],
        ),
        connector(
            "google_drive",
            "Google Drive",
            "Search and read shared design, product, and research documents.",
            "knowledge",
            ConnectorAuthType::OAuth,
            &[],
            Some("https://drive.google.com/"),
            Some("https://developers.google.com/drive/api"),
            vec![connector_tool("search_files", "Search Drive files.", true)],
        ),
        connector(
            "google_sheets",
            "Google Sheets",
            "Read spreadsheet context for planning, reporting, and lightweight data workflows.",
            "productivity",
            ConnectorAuthType::OAuth,
            &[],
            Some("https://docs.google.com/spreadsheets/"),
            Some("https://developers.google.com/sheets/api"),
            vec![
                connector_tool("read_spreadsheet", "Read spreadsheet metadata and values.", true),
                connector_tool("update_values", "Update spreadsheet values.", false),
            ],
        ),
    ]
}

fn read_config_from_path(path: &Path) -> ConnectorConfigFile {
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str::<ConnectorConfigFile>(&raw).ok())
        .unwrap_or_default()
}

fn write_config_to_path(path: &Path, config: &ConnectorConfigFile) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("create connector config dir: {err}"))?;
    }
    let raw = serde_json::to_string_pretty(config).map_err(|err| err.to_string())?;
    fs::write(path, format!("{raw}\n")).map_err(|err| format!("write connector config: {err}"))
}

fn sanitize_audit_optional(value: Option<String>, max_chars: usize) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(|value| truncate_for_display(&value.replace(['\r', '\n'], " "), max_chars))
}

fn append_connector_audit_event_at_path(
    path: &Path,
    request: ConnectorAuditRecordRequest,
) -> Result<ConnectorAuditEvent, String> {
    let connector_id = validate_connector_id(&request.connector_id)?;
    let operation = normalize_connector_id(&request.operation);
    if operation.is_empty() {
        return Err("connector audit operation is required".to_string());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("create connector audit dir: {err}"))?;
    }
    let event = ConnectorAuditEvent {
        id: uuid::Uuid::new_v4().to_string(),
        connector_id,
        operation,
        access: request.access,
        confirmation_required: request.confirmation_required,
        confirmed: request.confirmed,
        target: sanitize_audit_optional(request.target, 240),
        session_id: sanitize_audit_optional(request.session_id, 120),
        project_root: sanitize_audit_optional(request.project_root, 500),
        outcome: request.outcome,
        error_code: sanitize_audit_optional(request.error_code, 80),
        message: sanitize_audit_optional(request.message, 500),
        created_at: Utc::now().to_rfc3339(),
    };
    let line = serde_json::to_string(&event).map_err(|err| err.to_string())?;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|err| format!("open connector audit log: {err}"))?;
    writeln!(file, "{line}").map_err(|err| format!("write connector audit log: {err}"))?;
    Ok(event)
}

pub fn append_connector_audit_event(
    request: ConnectorAuditRecordRequest,
) -> Result<ConnectorAuditEvent, String> {
    append_connector_audit_event_at_path(&connector_audit_path(), request)
}

fn connector_permission_args(arguments: &serde_json::Value) -> &serde_json::Value {
    arguments
        .get("arguments")
        .filter(|_| {
            arguments
                .get("tool")
                .and_then(serde_json::Value::as_str)
                .map(|tool| tool.trim().eq_ignore_ascii_case("connector"))
                .unwrap_or(false)
        })
        .unwrap_or(arguments)
}

fn connector_permission_string_field<'a>(
    args: &'a serde_json::Value,
    names: &[&str],
) -> Option<&'a str> {
    names
        .iter()
        .find_map(|name| args.get(*name).and_then(serde_json::Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn normalize_connector_permission_id(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace([' ', '-'], "_")
}

fn canonical_connector_permission_operation(connector: &str, operation: &str) -> String {
    match connector {
        "slack" => match operation {
            "send_message" | "reply" => "post_message",
            "thread" | "replies" | "conversation_replies" => "read_thread",
            other => other,
        },
        "github" => match operation {
            "issues" => "list_issues",
            "get_issue" | "issue" => "read_issue",
            "list_pulls" | "pull_requests" | "prs" => "list_pull_requests",
            "get_pull_request" | "read_pr" | "get_pr" | "pr" => "read_pull_request",
            other => other,
        },
        "gitlab" => match operation {
            "issues" => "list_issues",
            "get_issue" | "issue" => "read_issue",
            "list_mrs" | "merge_requests" | "mrs" => "list_merge_requests",
            "get_merge_request" | "read_mr" | "get_mr" | "mr" => "read_merge_request",
            other => other,
        },
        "notion" => match operation {
            "search" | "search_page" | "pages" => "search_pages",
            "get_page" | "page" => "read_page",
            other => other,
        },
        "sentry" => match operation {
            "issues" => "list_issues",
            "get_issue" | "issue" => "read_issue",
            other => other,
        },
        "gmail" | "outlook" | "qq_mail" | "netease_mail" => match operation {
            "search" | "messages" | "list_messages" | "mail" | "emails" => "search_messages",
            "get_message" | "message" | "read_email" | "email" => "read_message",
            "send" | "send_email" | "compose" => "send_message",
            other => other,
        },
        _ => operation,
    }
    .to_string()
}

pub(crate) fn connector_permission_identity_from_args(
    arguments: &serde_json::Value,
) -> Option<(String, String)> {
    let args = connector_permission_args(arguments);
    let connector_id = connector_permission_string_field(args, &["connector", "connectorId"])
        .map(normalize_connector_permission_id)?;
    let operation = connector_permission_string_field(args, &["operation", "tool"])
        .map(normalize_connector_permission_id)?;
    let operation = canonical_connector_permission_operation(&connector_id, &operation);
    Some((connector_id, operation))
}

pub(crate) fn connector_permission_write_operation_from_args(
    arguments: &serde_json::Value,
) -> Option<(String, String)> {
    let (connector_id, operation) = connector_permission_identity_from_args(arguments)?;
    let is_write = matches!(
        (connector_id.as_str(), operation.as_str()),
        ("slack", "post_message")
            | ("discord", "post_message")
            | ("microsoft_teams", "post_message")
            | ("linear", "update_issue_status")
            | ("jira", "transition_issue")
            | ("sentry", "resolve_issue")
            | ("google_sheets", "update_values")
            | ("asana", "update_task")
            | ("trello", "move_card")
    ) || operation.starts_with("create_")
        || operation.starts_with("update_")
        || operation.starts_with("delete_")
        || operation.starts_with("post_")
        || operation.starts_with("send_")
        || operation.starts_with("publish_")
        || operation.starts_with("resolve_")
        || operation.starts_with("transition_");
    is_write.then_some((connector_id, operation))
}

pub(crate) fn connector_permission_write_confirmed(arguments: &serde_json::Value) -> bool {
    let args = connector_permission_args(arguments);
    args.get("confirm_write")
        .or_else(|| args.get("confirmWrite"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

fn connector_permission_u64_field(args: &serde_json::Value, names: &[&str]) -> Option<u64> {
    names
        .iter()
        .find_map(|name| args.get(*name).and_then(serde_json::Value::as_u64))
}

fn connector_permission_audit_target(
    connector_id: &str,
    operation: &str,
    arguments: &serde_json::Value,
) -> Option<String> {
    let args = connector_permission_args(arguments);
    match connector_id {
        "github" | "gitlab" => {
            let repo = connector_permission_string_field(args, &["repo", "repository"])?;
            Some(
                connector_permission_u64_field(args, &["number", "issue", "issue_number", "pr"])
                    .map(|number| format!("{repo}#{number}"))
                    .unwrap_or_else(|| repo.to_string()),
            )
        }
        "linear" => connector_permission_string_field(args, &["id", "key", "identifier"])
            .map(str::to_string)
            .or_else(|| {
                connector_permission_u64_field(args, &["number"]).map(|number| number.to_string())
            }),
        "notion" => connector_permission_string_field(args, &["id", "page_id"])
            .map(str::to_string)
            .or_else(|| {
                connector_permission_string_field(args, &["query", "search", "term"])
                    .map(|query| format!("search:{query}"))
            }),
        "sentry" => connector_permission_string_field(args, &["repo"])
            .map(str::to_string)
            .or_else(|| {
                match (
                    connector_permission_string_field(args, &["org", "organization"]),
                    connector_permission_string_field(args, &["project", "project_slug"]),
                ) {
                    (Some(org), Some(project)) => Some(format!("{org}/{project}")),
                    _ => connector_permission_string_field(args, &["id", "issue_id"])
                        .map(str::to_string),
                }
            }),
        "slack" => {
            let channel = connector_permission_string_field(args, &["channel", "repo"])?;
            let thread_ts =
                connector_permission_string_field(args, &["thread_ts", "threadTs", "thread", "id"]);
            Some(match (operation, thread_ts) {
                ("post_message", Some(thread_ts)) | ("read_thread", Some(thread_ts)) => {
                    format!("{channel} thread {thread_ts}")
                }
                _ => channel.to_string(),
            })
        }
        "gmail" | "outlook" | "qq_mail" | "netease_mail" => {
            connector_permission_string_field(args, &["id", "message_id", "messageId", "thread_id"])
                .map(str::to_string)
                .or_else(|| {
                    connector_permission_string_field(args, &["to", "recipient", "email"])
                        .map(str::to_string)
                })
                .or_else(|| {
                    connector_permission_string_field(args, &["query", "search", "subject"])
                        .map(|query| format!("search:{query}"))
                })
                .or_else(|| {
                    connector_permission_string_field(args, &["folder", "mailbox"])
                        .map(str::to_string)
                })
        }
        _ => None,
    }
}

pub(crate) fn connector_permission_audit_intent(
    tool_name: &str,
    arguments: &serde_json::Value,
) -> Option<ConnectorPermissionAuditIntent> {
    let tool_matches = tool_name.trim().eq_ignore_ascii_case("connector")
        || arguments
            .get("tool")
            .and_then(serde_json::Value::as_str)
            .map(|tool| tool.trim().eq_ignore_ascii_case("connector"))
            .unwrap_or(false);
    if !tool_matches {
        return None;
    }

    let (connector_id, operation) = connector_permission_identity_from_args(arguments)?;
    let is_write = connector_permission_write_operation_from_args(arguments).is_some();
    let access = if is_write {
        ConnectorAuditAccess::Write
    } else {
        ConnectorAuditAccess::Read
    };
    let confirmation_required = matches!(access, ConnectorAuditAccess::Write);
    Some(ConnectorPermissionAuditIntent {
        target: connector_permission_audit_target(&connector_id, &operation, arguments),
        connector_id,
        operation,
        access,
        confirmation_required,
        confirmed: confirmation_required && connector_permission_write_confirmed(arguments),
    })
}

pub(crate) fn append_connector_permission_denial_audit_event(
    tool_name: &str,
    arguments: &serde_json::Value,
    session_id: Option<&str>,
    project_root: Option<&Path>,
    reason: &str,
) -> Result<Option<ConnectorAuditEvent>, String> {
    let Some(intent) = connector_permission_audit_intent(tool_name, arguments) else {
        return Ok(None);
    };
    append_connector_audit_event(ConnectorAuditRecordRequest {
        connector_id: intent.connector_id,
        operation: intent.operation,
        access: intent.access,
        confirmation_required: intent.confirmation_required,
        confirmed: false,
        target: intent.target,
        session_id: session_id.map(str::to_string),
        project_root: project_root.map(|path| path.to_string_lossy().into_owned()),
        outcome: ConnectorAuditOutcome::Blocked,
        error_code: Some("user_denied".to_string()),
        message: Some(format!("权限审批被拒绝: {reason}")),
    })
    .map(Some)
}

fn list_connector_audit_events_from_path(
    path: &Path,
    connector_id: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<ConnectorAuditEvent>, String> {
    let connector_id = connector_id.map(validate_connector_id).transpose()?;
    let limit = limit
        .unwrap_or(CONNECTOR_AUDIT_DEFAULT_LIMIT)
        .clamp(1, CONNECTOR_AUDIT_MAX_LIMIT);
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(format!("open connector audit log: {err}")),
    };
    let reader = BufReader::new(file);
    let mut events = Vec::new();
    for line in reader.lines() {
        let line = line.map_err(|err| format!("read connector audit log: {err}"))?;
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<ConnectorAuditEvent>(&line) {
            Ok(event)
                if connector_id
                    .as_deref()
                    .map(|id| id == event.connector_id)
                    .unwrap_or(true) =>
            {
                events.push(event);
            }
            Ok(_) => {}
            Err(err) => {
                tracing::warn!(
                    target: "omiga::connectors",
                    error = %err,
                    "skipping malformed connector audit event"
                );
            }
        }
    }
    events.sort_by(|left, right| {
        right
            .created_at
            .cmp(&left.created_at)
            .then_with(|| right.id.cmp(&left.id))
    });
    events.truncate(limit);
    Ok(events)
}

pub fn list_connector_audit_events(
    connector_id: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<ConnectorAuditEvent>, String> {
    list_connector_audit_events_from_path(&connector_audit_path(), connector_id, limit)
}

fn env_var_configured(env_vars: &[String]) -> bool {
    env_vars.iter().any(|name| {
        std::env::var(name)
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
    })
}

fn plugin_ref_definition(id: &str) -> ConnectorDefinition {
    let label = id
        .split(['_', '-', '.'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    ConnectorDefinition {
        id: id.to_string(),
        name: if label.is_empty() { id.to_string() } else { label },
        description: "Connector reference declared by an enabled plugin. Add a first-class definition or matching MCP tools to make it actionable.".to_string(),
        category: "plugin".to_string(),
        auth_type: ConnectorAuthType::ExternalMcp,
        env_vars: Vec::new(),
        install_url: None,
        docs_url: None,
        default_enabled: true,
        tools: Vec::new(),
    }
}

fn merge_definitions(
    config: &ConnectorConfigFile,
    plugin_connector_ids: &[String],
) -> Vec<ResolvedConnectorDefinition> {
    let mut by_id = HashMap::<String, ResolvedConnectorDefinition>::new();
    for definition in builtin_connector_definitions() {
        let id = normalize_connector_id(&definition.id);
        if !id.is_empty() {
            by_id.insert(
                id.clone(),
                ResolvedConnectorDefinition {
                    definition: ConnectorDefinition { id, ..definition },
                    source: ConnectorDefinitionSource::BuiltIn,
                },
            );
        }
    }
    for definition in config.custom_connectors.clone() {
        let id = normalize_connector_id(&definition.id);
        if !id.is_empty() {
            by_id.insert(
                id.clone(),
                ResolvedConnectorDefinition {
                    definition: ConnectorDefinition { id, ..definition },
                    source: ConnectorDefinitionSource::Custom,
                },
            );
        }
    }
    for connector_id in plugin_connector_ids {
        let id = normalize_connector_id(connector_id);
        if !id.is_empty() && !by_id.contains_key(&id) {
            by_id.insert(
                id.clone(),
                ResolvedConnectorDefinition {
                    definition: plugin_ref_definition(&id),
                    source: ConnectorDefinitionSource::Plugin,
                },
            );
        }
    }
    let mut definitions = by_id.into_values().collect::<Vec<_>>();
    definitions.sort_by(|left, right| {
        left.definition
            .category
            .cmp(&right.definition.category)
            .then_with(|| left.definition.name.cmp(&right.definition.name))
            .then_with(|| left.definition.id.cmp(&right.definition.id))
    });
    definitions
}

fn referenced_plugin_map() -> HashMap<String, Vec<String>> {
    let mut out: HashMap<String, BTreeSet<String>> = HashMap::new();
    for plugin in plugins::plugin_load_outcome()
        .capability_summaries()
        .iter()
        .filter(|plugin| !plugin.apps.is_empty())
    {
        for connector_id in &plugin.apps {
            let id = normalize_connector_id(connector_id);
            if !id.is_empty() {
                out.entry(id)
                    .or_default()
                    .insert(plugin.display_name.clone());
            }
        }
    }
    out.into_iter()
        .map(|(id, names)| (id, names.into_iter().collect()))
        .collect()
}

fn connector_info(
    definition: ConnectorDefinition,
    source: ConnectorDefinitionSource,
    entry: Option<&ConnectorConfigEntry>,
    referenced_by_plugins: Vec<String>,
) -> ConnectorInfo {
    let enabled = entry
        .map(|entry| entry.enabled)
        .unwrap_or(definition.default_enabled);
    let use_env_credentials = entry.map(|entry| entry.use_env_credentials).unwrap_or(true);
    let env_configured = use_env_credentials && env_var_configured(&definition.env_vars);
    let credential_source = enabled
        .then(|| {
            connector_credential_source(&definition.id, &definition.env_vars, use_env_credentials)
        })
        .flatten();
    let manually_connected = entry.map(|entry| entry.connected).unwrap_or(false);
    let metadata_only = matches!(definition.auth_type, ConnectorAuthType::ExternalMcp);
    let no_auth_required = matches!(definition.auth_type, ConnectorAuthType::None);
    let connected =
        enabled && (no_auth_required || credential_source.is_some() || manually_connected);
    let accessible = connected && !metadata_only;
    let status = if !enabled {
        ConnectorConnectionStatus::Disabled
    } else if metadata_only {
        ConnectorConnectionStatus::MetadataOnly
    } else if connected {
        ConnectorConnectionStatus::Connected
    } else {
        ConnectorConnectionStatus::NeedsAuth
    };
    let connection_test_history = connector_test_history(entry);
    let connection_health = summarize_connection_health(&connection_test_history);

    ConnectorInfo {
        definition,
        enabled,
        connected,
        accessible,
        status,
        account_label: entry.and_then(|entry| entry.account_label.clone()),
        auth_source: entry
            .and_then(|entry| entry.auth_source.clone())
            .or_else(|| credential_source.map(|source| source.auth_source().to_string())),
        connected_at: entry.and_then(|entry| entry.connected_at.clone()),
        env_configured,
        referenced_by_plugins,
        source,
        last_connection_test: entry.and_then(|entry| entry.last_connection_test.clone()),
        connection_test_history,
        connection_health,
    }
}

fn connector_test_history(
    entry: Option<&ConnectorConfigEntry>,
) -> Vec<ConnectorConnectionTestResult> {
    let Some(entry) = entry else {
        return Vec::new();
    };
    let mut history = entry.connection_test_history.clone();
    if let Some(last) = &entry.last_connection_test {
        let already_recorded = history.iter().any(|item| {
            item.connector_id == last.connector_id && item.checked_at == last.checked_at
        });
        if !already_recorded {
            history.insert(0, last.clone());
        }
    }
    history.truncate(CONNECTOR_TEST_HISTORY_LIMIT);
    history
}

fn summarize_connection_health(
    history: &[ConnectorConnectionTestResult],
) -> ConnectorHealthSummary {
    let ok_checks = history.iter().filter(|result| result.ok).count();
    let failed_checks = history.len().saturating_sub(ok_checks);
    let retryable_failures = history
        .iter()
        .filter(|result| !result.ok && result.retryable)
        .count();
    let last_ok_at = history
        .iter()
        .find(|result| result.ok)
        .map(|result| result.checked_at.clone());
    let last_failure = history.iter().find(|result| !result.ok);

    ConnectorHealthSummary {
        total_checks: history.len(),
        ok_checks,
        failed_checks,
        retryable_failures,
        last_ok_at,
        last_failure_at: last_failure.map(|result| result.checked_at.clone()),
        last_error_code: last_failure.and_then(|result| result.error_code.clone()),
        last_http_status: last_failure.and_then(|result| result.http_status),
    }
}

fn clear_connector_test_observability(entry: &mut ConnectorConfigEntry) {
    entry.last_connection_test = None;
    entry.connection_test_history.clear();
}

fn list_connector_catalog_from_path(
    config_path: &Path,
    plugin_connector_ids: &[String],
    plugin_refs: &HashMap<String, Vec<String>>,
) -> ConnectorCatalog {
    let config = read_config_from_path(config_path);
    let connectors = merge_definitions(&config, plugin_connector_ids)
        .into_iter()
        .map(|resolved| {
            let id = normalize_connector_id(&resolved.definition.id);
            let refs = plugin_refs.get(id.as_str()).cloned().unwrap_or_default();
            connector_info(
                resolved.definition,
                resolved.source,
                config.connectors.get(&id),
                refs,
            )
        })
        .collect::<Vec<_>>();

    ConnectorCatalog {
        connectors,
        scope: "user".to_string(),
        config_path: config_path.to_string_lossy().into_owned(),
        notes: vec![
            "Connectors are user-level account/service links shared across projects on this machine.".to_string(),
            "Connectors model user-authorized external services separately from plugins and MCP servers.".to_string(),
            "Omiga stores user-level connector enablement/account metadata only; production connectors should use browser/software authorization, with environment credentials reserved for advanced local development or external secret managers.".to_string(),
        ],
    }
}

pub fn list_connector_catalog() -> ConnectorCatalog {
    let plugin_refs = referenced_plugin_map();
    let plugin_connector_ids = plugin_refs.keys().cloned().collect::<Vec<_>>();
    list_connector_catalog_from_path(
        &connector_config_path(),
        &plugin_connector_ids,
        &plugin_refs,
    )
}

pub async fn start_connector_login(
    connector_id: &str,
) -> Result<ConnectorLoginStartResult, String> {
    oauth::start_connector_login(connector_id).await
}

pub async fn poll_connector_login(
    login_session_id: &str,
) -> Result<ConnectorLoginPollResult, String> {
    oauth::poll_connector_login(login_session_id).await
}

fn mutate_connector_config<F>(connector_id: &str, mut update: F) -> Result<ConnectorInfo, String>
where
    F: FnMut(&mut ConnectorConfigEntry),
{
    let connector_id = validate_connector_id(connector_id)?;
    let path = connector_config_path();
    let mut config = read_config_from_path(&path);
    let plugin_refs = referenced_plugin_map();
    let plugin_connector_ids = plugin_refs.keys().cloned().collect::<Vec<_>>();
    let definitions = merge_definitions(&config, &plugin_connector_ids);
    let resolved = definitions
        .into_iter()
        .find(|resolved| resolved.definition.id == connector_id)
        .ok_or_else(|| format!("connector `{connector_id}` is not known"))?;
    let definition = resolved.definition;
    let source = resolved.source;

    let entry = config
        .connectors
        .entry(connector_id.clone())
        .or_insert_with(|| ConnectorConfigEntry {
            enabled: definition.default_enabled,
            connected: false,
            account_label: None,
            auth_source: None,
            connected_at: None,
            use_env_credentials: true,
            last_connection_test: None,
            connection_test_history: Vec::new(),
        });
    update(entry);
    write_config_to_path(&path, &config)?;
    let refs = plugin_refs.get(&connector_id).cloned().unwrap_or_default();
    Ok(connector_info(
        definition,
        source,
        config.connectors.get(&connector_id),
        refs,
    ))
}

pub fn set_connector_enabled(connector_id: &str, enabled: bool) -> Result<ConnectorInfo, String> {
    mutate_connector_config(connector_id, |entry| {
        entry.enabled = enabled;
        clear_connector_test_observability(entry);
        if enabled {
            entry.use_env_credentials = true;
        }
    })
}

pub fn connect_connector(request: ConnectorConnectRequest) -> Result<ConnectorInfo, String> {
    let connector_id = request.connector_id.clone();
    mutate_connector_config(&connector_id, |entry| {
        entry.enabled = true;
        entry.connected = true;
        entry.use_env_credentials = true;
        entry.account_label = request
            .account_label
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        entry.auth_source = request
            .auth_source
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| Some("manual".to_string()));
        entry.connected_at = Some(Utc::now().to_rfc3339());
        clear_connector_test_observability(entry);
    })
}

pub fn save_mail_connector_credentials(
    request: MailConnectorCredentialRequest,
) -> Result<ConnectorInfo, String> {
    let connector_id = validate_connector_id(&request.connector_id)?;
    if !matches!(connector_id.as_str(), "gmail" | "qq_mail" | "netease_mail") {
        return Err(format!(
            "connector `{connector_id}` does not support mailbox credential login"
        ));
    }

    let email_address = sanitize_email_address(&request.email_address)?;
    let authorization_code =
        sanitize_secret_field(&request.authorization_code, "mail authorization code", 512)?;

    secret_store::store_connector_secret(&connector_id, MAIL_ADDRESS_SECRET, &email_address)?;
    secret_store::store_connector_secret(
        &connector_id,
        MAIL_AUTHORIZATION_CODE_SECRET,
        &authorization_code,
    )?;

    connect_connector(ConnectorConnectRequest {
        connector_id,
        account_label: Some(email_address),
        auth_source: Some("mail_credentials".to_string()),
    })
}

pub fn disconnect_connector(connector_id: &str) -> Result<ConnectorInfo, String> {
    let normalized_connector_id = validate_connector_id(connector_id)?;
    match normalized_connector_id.as_str() {
        "github" => oauth::delete_github_oauth_token()?,
        "gmail" => {
            oauth::delete_gmail_oauth_token()?;
            delete_mail_connector_credentials(&normalized_connector_id)?;
        }
        "notion" => oauth::delete_notion_oauth_token()?,
        "slack" => oauth::delete_slack_oauth_token()?,
        "qq_mail" | "netease_mail" => delete_mail_connector_credentials(&normalized_connector_id)?,
        _ => {}
    }
    mutate_connector_config(connector_id, |entry| {
        entry.connected = false;
        entry.use_env_credentials = false;
        entry.account_label = None;
        entry.auth_source = None;
        entry.connected_at = None;
        clear_connector_test_observability(entry);
    })
}

fn current_plugin_refs() -> (HashMap<String, Vec<String>>, Vec<String>) {
    let plugin_refs = referenced_plugin_map();
    let plugin_connector_ids = plugin_refs.keys().cloned().collect::<Vec<_>>();
    (plugin_refs, plugin_connector_ids)
}

fn list_connector_catalog_from_path_with_current_refs(config_path: &Path) -> ConnectorCatalog {
    let (plugin_refs, plugin_connector_ids) = current_plugin_refs();
    list_connector_catalog_from_path(config_path, &plugin_connector_ids, &plugin_refs)
}

fn sanitize_text_field(value: &str, field: &str, max_chars: usize) -> Result<String, String> {
    let text = value.trim().to_string();
    if text.is_empty() {
        return Err(format!("{field} is required"));
    }
    if text.chars().count() > max_chars {
        return Err(format!("{field} must be at most {max_chars} characters"));
    }
    Ok(text)
}

fn sanitize_secret_field(value: &str, field: &str, max_chars: usize) -> Result<String, String> {
    let text = value.trim().to_string();
    if text.is_empty() {
        return Err(format!("{field} is required"));
    }
    if text.chars().count() > max_chars {
        return Err(format!("{field} must be at most {max_chars} characters"));
    }
    Ok(text)
}

fn sanitize_email_address(value: &str) -> Result<String, String> {
    let email = sanitize_text_field(value, "mail address", 254)?;
    if email.contains(char::is_whitespace)
        || !email.contains('@')
        || email.starts_with('@')
        || email.ends_with('@')
    {
        return Err("mail address must look like name@example.com".to_string());
    }
    Ok(email)
}

fn validate_optional_url(value: Option<String>, field: &str) -> Result<Option<String>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.trim().to_string();
    if value.is_empty() {
        return Ok(None);
    }
    if !(value.starts_with("https://") || value.starts_with("http://")) {
        return Err(format!("{field} must be an http(s) URL"));
    }
    if value.chars().count() > 500 {
        return Err(format!("{field} must be at most 500 characters"));
    }
    Ok(Some(value))
}

fn validate_env_var_name(value: &str) -> bool {
    let mut chars = value.chars();
    matches!(chars.next(), Some(first) if first.is_ascii_alphabetic() || first == '_')
        && chars.all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
}

fn validate_tool_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '.')
}

fn validate_custom_connector_request(
    request: CustomConnectorRequest,
) -> Result<ConnectorDefinition, String> {
    let id = validate_connector_id(&request.id)?;
    let builtin_ids = builtin_connector_definitions()
        .into_iter()
        .map(|definition| definition.id)
        .collect::<BTreeSet<_>>();
    if builtin_ids.contains(&id) {
        return Err(format!(
            "custom connector `{id}` conflicts with a built-in connector"
        ));
    }

    let name = sanitize_text_field(&request.name, "connector name", 80)?;
    let description = sanitize_text_field(&request.description, "connector description", 500)?;
    let category = request
        .category
        .trim()
        .to_ascii_lowercase()
        .replace([' ', '/'], "_")
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '_' || *ch == '-' || *ch == '.')
        .collect::<String>();
    let category = if category.is_empty() {
        "custom".to_string()
    } else {
        category
    };

    let mut env_vars = request
        .env_vars
        .into_iter()
        .map(|value| value.trim().to_ascii_uppercase())
        .filter(|value| !value.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if env_vars.len() > 8 {
        return Err("custom connector may declare at most 8 environment variables".to_string());
    }
    for env_var in &env_vars {
        if !validate_env_var_name(env_var) {
            return Err(format!(
                "invalid environment variable `{env_var}`; use names like SERVICE_API_TOKEN"
            ));
        }
    }

    let mut tools = Vec::new();
    let mut seen_tools = BTreeSet::new();
    for tool in request.tools.into_iter().take(20) {
        let name = normalize_connector_id(&tool.name);
        if !validate_tool_name(&name) {
            return Err(format!("invalid tool name `{}`", tool.name));
        }
        if !seen_tools.insert(name.clone()) {
            continue;
        }
        let description = tool.description.trim();
        tools.push(ConnectorToolDefinition {
            name,
            description: if description.is_empty() {
                "Custom connector operation.".to_string()
            } else {
                truncate_for_display(description, 240)
            },
            read_only: tool.read_only,
            required_scopes: tool
                .required_scopes
                .into_iter()
                .map(|scope| truncate_for_display(scope.trim(), 80))
                .filter(|scope| !scope.is_empty())
                .take(12)
                .collect(),
            confirmation_required: tool.confirmation_required || !tool.read_only,
            execution: ConnectorToolExecution::Declared,
        });
    }

    // Keep secret material outside the user-level connector config. Auth type and env var names
    // are metadata only; actual tokens live in the user's shell environment or secret manager.
    if matches!(&request.auth_type, ConnectorAuthType::ExternalMcp) {
        env_vars.clear();
    }

    Ok(ConnectorDefinition {
        id,
        name,
        description,
        category,
        auth_type: request.auth_type,
        env_vars,
        install_url: validate_optional_url(request.install_url, "install URL")?,
        docs_url: validate_optional_url(request.docs_url, "docs URL")?,
        default_enabled: request.default_enabled,
        tools,
    })
}

fn upsert_custom_connector_at_path(
    path: &Path,
    request: CustomConnectorRequest,
) -> Result<ConnectorCatalog, String> {
    let definition = validate_custom_connector_request(request)?;
    let connector_id = definition.id.clone();
    let mut config = read_config_from_path(path);
    config
        .custom_connectors
        .retain(|item| normalize_connector_id(&item.id) != definition.id);
    if let Some(entry) = config.connectors.get_mut(&connector_id) {
        clear_connector_test_observability(entry);
    }
    config.custom_connectors.push(definition);
    config.custom_connectors.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.id.cmp(&right.id))
    });
    write_config_to_path(path, &config)?;
    Ok(list_connector_catalog_from_path_with_current_refs(path))
}

pub fn upsert_custom_connector(
    request: CustomConnectorRequest,
) -> Result<ConnectorCatalog, String> {
    upsert_custom_connector_at_path(&connector_config_path(), request)
}

fn delete_custom_connector_at_path(
    path: &Path,
    connector_id: &str,
) -> Result<ConnectorCatalog, String> {
    let connector_id = validate_connector_id(connector_id)?;
    let mut config = read_config_from_path(path);
    let before = config.custom_connectors.len();
    config
        .custom_connectors
        .retain(|definition| normalize_connector_id(&definition.id) != connector_id);
    if before == config.custom_connectors.len() {
        return Err(format!("custom connector `{connector_id}` is not defined"));
    }
    config.connectors.remove(&connector_id);
    write_config_to_path(path, &config)?;
    Ok(list_connector_catalog_from_path_with_current_refs(path))
}

pub fn delete_custom_connector(connector_id: &str) -> Result<ConnectorCatalog, String> {
    delete_custom_connector_at_path(&connector_config_path(), connector_id)
}

fn export_custom_connectors_from_path(path: &Path) -> CustomConnectorExport {
    let mut connectors = read_config_from_path(path).custom_connectors;
    connectors.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.id.cmp(&right.id))
    });
    CustomConnectorExport {
        version: 1,
        scope: "user".to_string(),
        connectors,
    }
}

pub fn export_custom_connectors() -> CustomConnectorExport {
    export_custom_connectors_from_path(&connector_config_path())
}

fn import_custom_connectors_at_path(
    path: &Path,
    request: CustomConnectorImportRequest,
) -> Result<ConnectorCatalog, String> {
    if request.connectors.is_empty() {
        return Err("custom connector import did not include any connectors".to_string());
    }

    let definitions = request
        .connectors
        .into_iter()
        .map(validate_custom_connector_request)
        .collect::<Result<Vec<_>, _>>()?;
    let imported_ids = definitions
        .iter()
        .map(|definition| definition.id.clone())
        .collect::<BTreeSet<_>>();

    let mut config = read_config_from_path(path);
    let previous_custom_ids = config
        .custom_connectors
        .iter()
        .map(|definition| normalize_connector_id(&definition.id))
        .collect::<BTreeSet<_>>();

    if request.replace_existing {
        config.custom_connectors.clear();
        for connector_id in previous_custom_ids.difference(&imported_ids) {
            config.connectors.remove(connector_id);
        }
    } else {
        config
            .custom_connectors
            .retain(|definition| !imported_ids.contains(&normalize_connector_id(&definition.id)));
    }
    for connector_id in &imported_ids {
        if let Some(entry) = config.connectors.get_mut(connector_id) {
            clear_connector_test_observability(entry);
        }
    }

    config.custom_connectors.extend(definitions);
    config.custom_connectors.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.id.cmp(&right.id))
    });
    write_config_to_path(path, &config)?;
    Ok(list_connector_catalog_from_path_with_current_refs(path))
}

pub fn import_custom_connectors(
    request: CustomConnectorImportRequest,
) -> Result<ConnectorCatalog, String> {
    import_custom_connectors_at_path(&connector_config_path(), request)
}

fn connector_test_result(
    connector: &ConnectorInfo,
    ok: bool,
    check_kind: ConnectorConnectionTestKind,
    message: impl Into<String>,
) -> ConnectorConnectionTestResult {
    connector_test_result_with_observability(
        connector, ok, check_kind, message, None, false, None, None,
    )
}

#[allow(clippy::too_many_arguments)]
fn connector_test_result_with_observability(
    connector: &ConnectorInfo,
    ok: bool,
    check_kind: ConnectorConnectionTestKind,
    message: impl Into<String>,
    http_status: Option<u16>,
    retryable: bool,
    error_code: Option<String>,
    details: Option<String>,
) -> ConnectorConnectionTestResult {
    ConnectorConnectionTestResult {
        connector_id: connector.definition.id.clone(),
        connector_name: connector.definition.name.clone(),
        ok,
        status: connector.status.clone(),
        check_kind,
        message: message.into(),
        checked_at: Utc::now().to_rfc3339(),
        account_label: connector.account_label.clone(),
        auth_source: connector.auth_source.clone(),
        http_status,
        retryable,
        error_code,
        details,
    }
}

fn connector_test_result_from_check(
    connector: &ConnectorInfo,
    ok: bool,
    check_kind: ConnectorConnectionTestKind,
    check: NativeConnectionCheck,
) -> ConnectorConnectionTestResult {
    connector_test_result_with_observability(
        connector,
        ok,
        check_kind,
        check.message,
        check.http_status,
        check.retryable,
        check.error_code,
        check.details,
    )
}

fn native_connector_test_available(connector_id: &str) -> bool {
    matches!(
        connector_id,
        "github"
            | "gmail"
            | "gitlab"
            | "linear"
            | "notion"
            | "sentry"
            | "slack"
            | "qq_mail"
            | "netease_mail"
            | "discord"
            | "jira"
            | "confluence"
    )
}

fn connector_match_key(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn connector_metadata_matches(connector: &ConnectorInfo, value: &str) -> bool {
    let candidate = connector_match_key(value);
    if candidate.is_empty() {
        return false;
    }
    let id_key = connector_match_key(&connector.definition.id);
    let name_key = connector_match_key(&connector.definition.name);
    candidate == id_key
        || candidate == name_key
        || (id_key.len() >= 4 && candidate.contains(&id_key))
        || (name_key.len() >= 4 && candidate.contains(&name_key))
}

fn mcp_tool_meta_string(tool: &McpTool, keys: &[&str]) -> Option<String> {
    let meta = tool.meta.as_ref()?;
    keys.iter().find_map(|key| {
        meta.0
            .get(*key)
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

fn codex_apps_bridge_from_mcp_tools<I>(
    server_name: &str,
    tools: I,
    connector: &ConnectorInfo,
) -> Option<ConnectorAppBridge>
where
    I: IntoIterator<Item = McpTool>,
{
    if normalize_name_for_mcp(server_name) != CODEX_APPS_MCP_SERVER_NAME {
        return None;
    }

    let mut connector_name = None;
    let mut connector_description = None;
    let mut tool_names = BTreeSet::new();

    for tool in tools {
        let meta_connector_id = mcp_tool_meta_string(&tool, &["connector_id", "connectorId"]);
        let meta_connector_name = mcp_tool_meta_string(
            &tool,
            &[
                "connector_name",
                "connectorName",
                "connector_display_name",
                "connectorDisplayName",
            ],
        );
        let matches = meta_connector_id
            .as_deref()
            .is_some_and(|value| connector_metadata_matches(connector, value))
            || meta_connector_name
                .as_deref()
                .is_some_and(|value| connector_metadata_matches(connector, value));
        if !matches {
            continue;
        }

        if connector_name.is_none() {
            connector_name = meta_connector_name;
        }
        if connector_description.is_none() {
            connector_description =
                mcp_tool_meta_string(&tool, &["connector_description", "connectorDescription"]);
        }
        tool_names.insert(build_mcp_tool_name(server_name, tool.name.as_ref()));
    }

    if tool_names.is_empty() {
        None
    } else {
        Some(ConnectorAppBridge {
            connector_name,
            connector_description,
            tool_names: tool_names.into_iter().collect(),
        })
    }
}

fn codex_apps_server_name(project_root: &Path) -> Option<String> {
    merged_mcp_servers(project_root)
        .keys()
        .find(|name| normalize_name_for_mcp(name) == CODEX_APPS_MCP_SERVER_NAME)
        .cloned()
}

async fn discover_codex_apps_bridge_for_connector(
    project_root: &Path,
    connector: &ConnectorInfo,
) -> Option<ConnectorAppBridge> {
    let server_name = codex_apps_server_name(project_root)?;
    let tools = list_tools_for_server(project_root, &server_name, MCP_CONNECTOR_BRIDGE_TIMEOUT)
        .await
        .ok()?;
    codex_apps_bridge_from_mcp_tools(&server_name, tools, connector)
}

fn connector_auth_source_is_codex_apps(connector: &ConnectorInfo) -> bool {
    connector
        .auth_source
        .as_deref()
        .is_some_and(|source| matches!(source, "codex_apps" | "mcp_app"))
}

fn connector_app_bridge_test_result(
    connector: &ConnectorInfo,
    bridge: &ConnectorAppBridge,
) -> ConnectorConnectionTestResult {
    let tool_summary = if bridge.tool_names.is_empty() {
        "no tools reported".to_string()
    } else {
        bridge.tool_names.join(", ")
    };
    let description = bridge
        .connector_description
        .as_deref()
        .map(|value| format!(" {value}"))
        .unwrap_or_default();
    connector_test_result_with_observability(
        connector,
        true,
        ConnectorConnectionTestKind::LocalState,
        format!(
            "{} is available through the Codex/OpenAI apps MCP bridge.{description}",
            connector.definition.name
        ),
        None,
        false,
        Some("codex_apps_bridge".to_string()),
        Some(format!(
            "Use the listed MCP app tools instead of local token-backed native connector calls: {tool_summary}."
        )),
    )
}

fn first_non_empty_env(names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        std::env::var(name)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

fn first_base_url_env(names: &[&str]) -> Option<String> {
    first_non_empty_env(names).map(|value| value.trim_end_matches('/').to_string())
}

fn github_api_base_url() -> String {
    first_base_url_env(&["OMIGA_GITHUB_API_BASE_URL"])
        .unwrap_or_else(|| "https://api.github.com".to_string())
}

fn github_env_token() -> Option<String> {
    first_non_empty_env(&["GITHUB_TOKEN", "GH_TOKEN"])
}

fn command_output_trimmed_with_timeout(
    program: &str,
    args: &[&str],
    timeout: Duration,
) -> Option<String> {
    let mut child = Command::new(program)
        .args(args)
        .env("GH_PROMPT_DISABLED", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let started_at = Instant::now();

    loop {
        match child.try_wait() {
            Ok(Some(_status)) => {
                let output = child.wait_with_output().ok()?;
                if !output.status.success() {
                    return None;
                }
                return Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
                    .filter(|value| !value.is_empty());
            }
            Ok(None) => {
                if started_at.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                thread::sleep(Duration::from_millis(25));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

fn github_cli_auth_enabled() -> bool {
    if first_non_empty_env(&["OMIGA_DISABLE_GITHUB_CLI_AUTH"]).is_some() {
        return false;
    }

    #[cfg(test)]
    {
        first_non_empty_env(&["OMIGA_TEST_ENABLE_GITHUB_CLI_AUTH"]).is_some()
    }

    #[cfg(not(test))]
    {
        true
    }
}

fn github_cli_binary() -> String {
    first_non_empty_env(&["OMIGA_GITHUB_CLI_PATH"]).unwrap_or_else(|| "gh".to_string())
}

fn github_cli_token() -> Option<String> {
    if !github_cli_auth_enabled() {
        return None;
    }
    command_output_trimmed_with_timeout(
        &github_cli_binary(),
        &["auth", "token"],
        GITHUB_CLI_AUTH_TIMEOUT,
    )
}

fn github_token_with_source(
    use_external_credentials: bool,
) -> Option<(String, ConnectorCredentialSource)> {
    oauth::github_oauth_token()
        .map(|token| (token, ConnectorCredentialSource::OAuthDevice))
        .or_else(|| {
            use_external_credentials
                .then(github_env_token)
                .flatten()
                .map(|token| (token, ConnectorCredentialSource::Environment))
        })
        .or_else(|| {
            use_external_credentials
                .then(github_cli_token)
                .flatten()
                .map(|token| (token, ConnectorCredentialSource::GitHubCli))
        })
}

fn gmail_token_with_source(
    use_external_credentials: bool,
) -> Option<(String, ConnectorCredentialSource)> {
    oauth_or_env_token_with_source(
        oauth::gmail_oauth_token(),
        ConnectorCredentialSource::OAuthBrowser,
        use_external_credentials,
        GMAIL_OAUTH_ENV_VARS,
    )
}

fn notion_token_with_source(
    use_external_credentials: bool,
) -> Option<(String, ConnectorCredentialSource)> {
    oauth_or_env_token_with_source(
        oauth::notion_oauth_token(),
        ConnectorCredentialSource::OAuthBrowser,
        use_external_credentials,
        NOTION_TOKEN_ENV_VARS,
    )
}

fn slack_token_with_source(
    use_external_credentials: bool,
) -> Option<(String, ConnectorCredentialSource)> {
    oauth_or_env_token_with_source(
        oauth::slack_oauth_token(),
        ConnectorCredentialSource::OAuthBrowser,
        use_external_credentials,
        SLACK_TOKEN_ENV_VARS,
    )
}

fn oauth_or_env_token_with_source(
    oauth_token: Option<String>,
    oauth_source: ConnectorCredentialSource,
    use_external_credentials: bool,
    env_vars: &[&str],
) -> Option<(String, ConnectorCredentialSource)> {
    oauth_token.map(|token| (token, oauth_source)).or_else(|| {
        use_external_credentials
            .then(|| first_non_empty_env(env_vars))
            .flatten()
            .map(|token| (token, ConnectorCredentialSource::Environment))
    })
}

fn connector_credential_source(
    connector_id: &str,
    env_vars: &[String],
    use_env_credentials: bool,
) -> Option<ConnectorCredentialSource> {
    match connector_id {
        "github" => github_token_with_source(use_env_credentials).map(|(_, source)| source),
        "gmail" => gmail_token_with_source(use_env_credentials)
            .map(|(_, source)| source)
            .or_else(|| {
                stored_mail_credentials_configured(connector_id)
                    .then_some(ConnectorCredentialSource::MailCredentials)
            })
            .or_else(|| {
                (use_env_credentials && mail_credentials_configured(connector_id))
                    .then_some(ConnectorCredentialSource::Environment)
            }),
        "notion" => notion_token_with_source(use_env_credentials).map(|(_, source)| source),
        "slack" => slack_token_with_source(use_env_credentials).map(|(_, source)| source),
        "qq_mail" | "netease_mail" if stored_mail_credentials_configured(connector_id) => {
            Some(ConnectorCredentialSource::MailCredentials)
        }
        "qq_mail" | "netease_mail"
            if use_env_credentials && mail_credentials_configured(connector_id) =>
        {
            Some(ConnectorCredentialSource::Environment)
        }
        _ if use_env_credentials && env_var_configured(env_vars) => {
            Some(ConnectorCredentialSource::Environment)
        }
        _ => None,
    }
}

pub(crate) fn github_token() -> Option<String> {
    github_token_with_source(true).map(|(token, _source)| token)
}

pub(crate) fn gmail_api_base_url() -> String {
    first_base_url_env(&["OMIGA_GMAIL_API_BASE_URL"])
        .unwrap_or_else(|| "https://gmail.googleapis.com/gmail/v1".to_string())
}

fn stored_mail_secret_value(connector_id: &str, secret_name: &str) -> Option<String> {
    secret_store::read_connector_secret(connector_id, secret_name)
        .ok()
        .flatten()
}

fn stored_mail_credentials_configured(connector_id: &str) -> bool {
    stored_mail_secret_value(connector_id, MAIL_ADDRESS_SECRET).is_some()
        && stored_mail_secret_value(connector_id, MAIL_AUTHORIZATION_CODE_SECRET).is_some()
}

fn delete_mail_connector_credentials(connector_id: &str) -> Result<(), String> {
    secret_store::delete_connector_secret(connector_id, MAIL_ADDRESS_SECRET)?;
    secret_store::delete_connector_secret(connector_id, MAIL_AUTHORIZATION_CODE_SECRET)
}

fn mail_identity(connector_id: &str) -> Option<String> {
    if let Some(identity) = stored_mail_secret_value(connector_id, MAIL_ADDRESS_SECRET) {
        return Some(identity);
    }
    match connector_id {
        "gmail" => first_non_empty_env(&[
            "GMAIL_ADDRESS",
            "GMAIL_USERNAME",
            "MAIL_ADDRESS",
            "MAIL_USERNAME",
        ]),
        "qq_mail" => first_non_empty_env(&[
            "QQ_MAIL_ADDRESS",
            "QQ_MAIL_USERNAME",
            "MAIL_ADDRESS",
            "MAIL_USERNAME",
        ]),
        "netease_mail" => first_non_empty_env(&[
            "NETEASE_MAIL_ADDRESS",
            "NETEASE_MAIL_USERNAME",
            "MAIL_ADDRESS",
            "MAIL_USERNAME",
        ]),
        _ => None,
    }
}

fn mail_secret(connector_id: &str) -> Option<String> {
    if let Some(secret) = stored_mail_secret_value(connector_id, MAIL_AUTHORIZATION_CODE_SECRET) {
        return Some(secret);
    }
    match connector_id {
        "gmail" => {
            first_non_empty_env(&["GMAIL_APP_PASSWORD", "GMAIL_AUTH_CODE", "MAIL_APP_PASSWORD"])
        }
        "qq_mail" => first_non_empty_env(&[
            "QQ_MAIL_AUTH_CODE",
            "QQ_MAIL_APP_PASSWORD",
            "MAIL_APP_PASSWORD",
        ]),
        "netease_mail" => first_non_empty_env(&[
            "NETEASE_MAIL_AUTH_CODE",
            "NETEASE_MAIL_APP_PASSWORD",
            "MAIL_APP_PASSWORD",
        ]),
        _ => None,
    }
}

fn mail_imap_host(connector_id: &str) -> Option<String> {
    match connector_id {
        "gmail" => first_non_empty_env(&["GMAIL_IMAP_HOST", "MAIL_IMAP_HOST"])
            .or_else(|| Some("imap.gmail.com".to_string())),
        "qq_mail" => first_non_empty_env(&["QQ_MAIL_IMAP_HOST", "MAIL_IMAP_HOST"])
            .or_else(|| Some("imap.qq.com".to_string())),
        "netease_mail" => first_non_empty_env(&["NETEASE_MAIL_IMAP_HOST", "MAIL_IMAP_HOST"])
            .or_else(|| Some("imap.163.com".to_string())),
        _ => None,
    }
}

fn mail_imap_port(connector_id: &str) -> u16 {
    let env_port = match connector_id {
        "gmail" => first_non_empty_env(&["GMAIL_IMAP_PORT", "MAIL_IMAP_PORT"]),
        "qq_mail" => first_non_empty_env(&["QQ_MAIL_IMAP_PORT", "MAIL_IMAP_PORT"]),
        "netease_mail" => first_non_empty_env(&["NETEASE_MAIL_IMAP_PORT", "MAIL_IMAP_PORT"]),
        _ => None,
    };
    env_port
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|port| *port > 0)
        .unwrap_or(993)
}

fn mail_credentials_configured(connector_id: &str) -> bool {
    mail_identity(connector_id).is_some() && mail_secret(connector_id).is_some()
}

fn gitlab_api_base_url() -> String {
    first_base_url_env(&["OMIGA_GITLAB_API_BASE_URL"])
        .unwrap_or_else(|| "https://gitlab.com/api/v4".to_string())
}

fn gitlab_token() -> Option<String> {
    first_non_empty_env(&["GITLAB_TOKEN"])
}

fn linear_graphql_url() -> String {
    first_base_url_env(&["OMIGA_LINEAR_GRAPHQL_URL"])
        .unwrap_or_else(|| "https://api.linear.app/graphql".to_string())
}

fn linear_authorization_header() -> Option<String> {
    first_non_empty_env(&["LINEAR_ACCESS_TOKEN"])
        .map(|token| format!("Bearer {token}"))
        .or_else(|| first_non_empty_env(&["LINEAR_API_KEY"]))
}

fn notion_api_base_url() -> String {
    first_base_url_env(&["OMIGA_NOTION_API_BASE_URL"])
        .unwrap_or_else(|| "https://api.notion.com/v1".to_string())
}

fn notion_version() -> String {
    first_non_empty_env(&["OMIGA_NOTION_VERSION"]).unwrap_or_else(|| "2022-06-28".to_string())
}

fn notion_token() -> Option<String> {
    notion_token_with_source(true).map(|(token, _source)| token)
}

fn sentry_api_base_url() -> String {
    first_base_url_env(&["OMIGA_SENTRY_API_BASE_URL"])
        .unwrap_or_else(|| "https://sentry.io/api/0".to_string())
}

fn sentry_token() -> Option<String> {
    first_non_empty_env(&["SENTRY_AUTH_TOKEN"])
}

fn slack_api_base_url() -> String {
    first_base_url_env(&["OMIGA_SLACK_API_BASE_URL"])
        .unwrap_or_else(|| "https://slack.com/api".to_string())
}

fn slack_token() -> Option<String> {
    slack_token_with_source(true).map(|(token, _source)| token)
}

fn discord_api_base_url() -> String {
    first_base_url_env(&["OMIGA_DISCORD_API_BASE_URL"])
        .unwrap_or_else(|| "https://discord.com/api/v10".to_string())
}

fn discord_authorization_header() -> Option<String> {
    first_non_empty_env(&["DISCORD_BOT_TOKEN"]).map(|token| format!("Bot {token}"))
}

fn jira_api_base_url() -> Option<String> {
    first_base_url_env(&["OMIGA_JIRA_API_BASE_URL"]).or_else(|| {
        first_base_url_env(&["JIRA_SITE_URL", "JIRA_BASE_URL", "ATLASSIAN_SITE_URL"])
            .map(|site_url| format!("{site_url}/rest/api/3"))
    })
}

fn confluence_api_base_url() -> Option<String> {
    first_base_url_env(&["OMIGA_CONFLUENCE_API_BASE_URL"]).or_else(|| {
        first_base_url_env(&[
            "CONFLUENCE_SITE_URL",
            "CONFLUENCE_BASE_URL",
            "ATLASSIAN_SITE_URL",
        ])
        .map(|site_url| format!("{site_url}/wiki/rest/api"))
    })
}

fn basic_auth_header(username: &str, token: &str) -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(format!("{username}:{token}"));
    format!("Basic {encoded}")
}

fn jira_authorization_header() -> Option<String> {
    let email = first_non_empty_env(&["JIRA_EMAIL", "ATLASSIAN_EMAIL"])?;
    let token = first_non_empty_env(&["JIRA_API_TOKEN", "ATLASSIAN_API_TOKEN"])?;
    Some(basic_auth_header(&email, &token))
}

fn confluence_authorization_header() -> Option<String> {
    let email = first_non_empty_env(&["CONFLUENCE_EMAIL", "ATLASSIAN_EMAIL"])?;
    let token = first_non_empty_env(&["CONFLUENCE_API_TOKEN", "ATLASSIAN_API_TOKEN"])?;
    Some(basic_auth_header(&email, &token))
}

#[derive(Debug, Clone)]
struct NativeConnectionCheck {
    message: String,
    http_status: Option<u16>,
    retryable: bool,
    error_code: Option<String>,
    details: Option<String>,
}

impl NativeConnectionCheck {
    fn ok(service_name: &str, status: reqwest::StatusCode) -> Self {
        Self {
            message: format!("{service_name} API responded with {status}."),
            http_status: Some(status.as_u16()),
            retryable: false,
            error_code: None,
            details: None,
        }
    }

    fn local_ok(message: impl Into<String>, details: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            http_status: None,
            retryable: false,
            error_code: None,
            details: Some(details.into()),
        }
    }

    fn local_error(
        message: impl Into<String>,
        error_code: impl Into<String>,
        details: impl Into<String>,
        retryable: bool,
    ) -> Self {
        Self {
            message: message.into(),
            http_status: None,
            retryable,
            error_code: Some(error_code.into()),
            details: Some(details.into()),
        }
    }

    fn missing_credentials(service_name: &str, env_hint: &str) -> Self {
        Self {
            message: format!("{service_name} native API check requires {env_hint}."),
            http_status: None,
            retryable: false,
            error_code: Some("missing_credentials".to_string()),
            details: Some(format!("Set {env_hint} outside connectors/config.json.")),
        }
    }

    fn api_error(
        service_name: &str,
        status: reqwest::StatusCode,
        error_code: impl Into<String>,
        details: impl Into<String>,
    ) -> Self {
        let error_code = error_code.into();
        Self {
            message: format!("{service_name} API returned application error `{error_code}`."),
            http_status: Some(status.as_u16()),
            retryable: false,
            error_code: Some(error_code),
            details: Some(details.into()),
        }
    }

    fn invalid_response(
        service_name: &str,
        status: reqwest::StatusCode,
        details: impl Into<String>,
    ) -> Self {
        Self {
            message: format!("{service_name} API returned an invalid response."),
            http_status: Some(status.as_u16()),
            retryable: false,
            error_code: Some("invalid_response".to_string()),
            details: Some(details.into()),
        }
    }

    fn from_http_error(err: ConnectorHttpError) -> Self {
        let error_code = err
            .status
            .map(|status| format!("http_{}", status.as_u16()))
            .or_else(|| {
                Some(if err.retryable {
                    "network_retryable".to_string()
                } else {
                    "network_error".to_string()
                })
            });
        Self {
            message: err.user_message(),
            http_status: err.status.map(|status| status.as_u16()),
            retryable: err.retryable,
            error_code,
            details: Some(if err.retryable {
                "Transient connector failure. Check service status/rate limits and retry."
                    .to_string()
            } else {
                "Connector request failed. Check credentials, endpoint configuration, and service access.".to_string()
            }),
        }
    }
}

async fn slack_auth_test_check(
    token: String,
) -> Result<NativeConnectionCheck, NativeConnectionCheck> {
    let request = ConnectorHttpRequest::new(
        "Slack",
        Method::GET,
        format!("{}/auth.test", slack_api_base_url()),
    )
    .bearer_token(token);
    match http::send_connector_request(request).await {
        Ok(response) => match serde_json::from_str::<serde_json::Value>(&response.body) {
            Ok(body)
                if body
                    .get("ok")
                    .and_then(|value| value.as_bool())
                    .unwrap_or(false) =>
            {
                Ok(NativeConnectionCheck::ok("Slack", response.status))
            }
            Ok(body) => {
                let error_code = body
                    .get("error")
                    .and_then(|value| value.as_str())
                    .unwrap_or("slack_auth_failed");
                Err(NativeConnectionCheck::api_error(
                    "Slack",
                    response.status,
                    error_code,
                    "Slack auth.test returned ok=false. Check SLACK_BOT_TOKEN scopes and workspace installation.",
                ))
            }
            Err(err) => Err(NativeConnectionCheck::invalid_response(
                "Slack",
                response.status,
                format!("Slack auth.test response JSON parse failed: {err}"),
            )),
        },
        Err(err) => Err(NativeConnectionCheck::from_http_error(err)),
    }
}

fn mail_connection_test_enabled() -> bool {
    first_non_empty_env(&["OMIGA_MAIL_SKIP_NETWORK_CHECK"]).is_none()
}

fn mail_credential_check(connector: &ConnectorInfo) -> NativeConnectionCheck {
    let connector_id = connector.definition.id.as_str();
    let service_name = connector.definition.name.as_str();
    let Some(identity) = mail_identity(connector_id) else {
        return NativeConnectionCheck::local_error(
            format!("{service_name} 需要邮箱地址才能连接。"),
            "missing_credentials",
            "在连接界面输入邮箱地址；Omiga 会把账号标识写入用户级连接状态。",
            false,
        );
    };
    if mail_secret(connector_id).is_none() {
        let (credential_name, credential_hint) = if connector_id == "gmail" {
            (
                "Google 应用专用密码",
                "在连接界面输入 Gmail 地址和 Google 应用专用密码；Omiga 会保存到系统安全存储，不写入 connectors/config.json。",
            )
        } else {
            (
                "邮箱授权码或应用专用密码",
                "在连接界面输入邮箱服务商生成的授权码；Omiga 会保存到系统安全存储，不写入 connectors/config.json。",
            )
        };
        return NativeConnectionCheck::local_error(
            format!("{service_name} 需要{credential_name}才能连接。"),
            "missing_credentials",
            credential_hint,
            false,
        );
    }

    if !mail_connection_test_enabled() {
        return NativeConnectionCheck::local_ok(
            format!("{service_name} mailbox credentials are configured for {identity}."),
            "Network IMAP reachability check skipped by OMIGA_MAIL_SKIP_NETWORK_CHECK; secrets remain outside connectors/config.json.",
        );
    }

    let Some(host) = mail_imap_host(connector_id) else {
        return NativeConnectionCheck::local_ok(
            format!("{service_name} mailbox credentials are configured for {identity}."),
            "No IMAP host was configured, so Omiga validated local credential presence only.",
        );
    };
    let timeout = Duration::from_secs(3);
    let port = mail_imap_port(connector_id);
    let Some(secret) = mail_secret(connector_id) else {
        unreachable!("mail secret presence is checked before IMAP login")
    };
    mail_imap_login_check(service_name, &identity, &secret, &host, port, timeout)
}

fn mail_imap_login_check(
    service_name: &str,
    identity: &str,
    secret: &str,
    host: &str,
    port: u16,
    timeout: Duration,
) -> NativeConnectionCheck {
    match mail_imap_login(host, port, identity, secret, timeout) {
        Ok(details) => NativeConnectionCheck::local_ok(
            format!("{service_name} IMAP 登录成功。"),
            format!(
                "Authenticated account {identity} against {host}:{port}. {details} Secrets remain outside connectors/config.json."
            ),
        ),
        Err(MailImapLoginError::AuthRejected(details)) => NativeConnectionCheck::local_error(
            format!("{service_name} IMAP 登录被拒绝。"),
            "mail_imap_auth_failed",
            format!(
                "邮箱服务拒绝了 {identity} 的登录：{details}。请确认授权码、应用专用密码或 IMAP 开关。"
            ),
            false,
        ),
        Err(MailImapLoginError::Network(details)) => NativeConnectionCheck::local_error(
            format!("{service_name} IMAP 登录不可用。"),
            "mail_imap_login_unreachable",
            format!("Tried {host}:{port} for account {identity}: {details}"),
            true,
        ),
        Err(MailImapLoginError::Tls(details)) => NativeConnectionCheck::local_error(
            format!("{service_name} IMAP TLS 校验失败。"),
            "mail_imap_tls_failed",
            format!("Tried {host}:{port} for account {identity}: {details}"),
            true,
        ),
        Err(MailImapLoginError::Protocol(details)) => NativeConnectionCheck::local_error(
            format!("{service_name} IMAP 响应无法识别。"),
            "mail_imap_protocol_error",
            format!("Tried {host}:{port} for account {identity}: {details}"),
            true,
        ),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum MailImapLoginError {
    AuthRejected(String),
    Network(String),
    Tls(String),
    Protocol(String),
}

fn mail_imap_login(
    host: &str,
    port: u16,
    username: &str,
    password: &str,
    timeout: Duration,
) -> Result<String, MailImapLoginError> {
    let addr = format!("{host}:{port}");
    let socket_addr = addr
        .to_socket_addrs()
        .map_err(|err| MailImapLoginError::Network(format!("resolve {addr}: {err}")))?
        .next()
        .ok_or_else(|| MailImapLoginError::Network(format!("{addr} did not resolve")))?;
    let tcp_stream = TcpStream::connect_timeout(&socket_addr, timeout)
        .map_err(|err| MailImapLoginError::Network(format!("connect {addr}: {err}")))?;
    tcp_stream
        .set_read_timeout(Some(timeout))
        .map_err(|err| MailImapLoginError::Network(format!("set read timeout: {err}")))?;
    tcp_stream
        .set_write_timeout(Some(timeout))
        .map_err(|err| MailImapLoginError::Network(format!("set write timeout: {err}")))?;

    let mut root_store = rustls::RootCertStore::empty();
    let certs = rustls_native_certs::load_native_certs();
    for cert in certs.certs {
        root_store
            .add(cert)
            .map_err(|err| MailImapLoginError::Tls(format!("load root certificate: {err}")))?;
    }
    if root_store.is_empty() {
        return Err(MailImapLoginError::Tls(
            "no platform root certificates were loaded".to_string(),
        ));
    }

    let server_name = rustls::pki_types::ServerName::try_from(host.to_string())
        .map_err(|err| MailImapLoginError::Tls(format!("invalid server name `{host}`: {err}")))?;
    let tls_config = rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();
    let tls_connection = rustls::ClientConnection::new(Arc::new(tls_config), server_name)
        .map_err(|err| MailImapLoginError::Tls(format!("create TLS client: {err}")))?;
    let tls_stream = rustls::StreamOwned::new(tls_connection, tcp_stream);
    let mut reader = BufReader::new(tls_stream);

    let greeting = read_imap_line(&mut reader).map_err(MailImapLoginError::Network)?;
    if !greeting.starts_with("* OK") {
        return Err(MailImapLoginError::Protocol(format!(
            "unexpected greeting `{}`",
            sanitize_imap_response(&greeting)
        )));
    }

    let tag = "A0001";
    let command = format!(
        "{tag} LOGIN \"{}\" \"{}\"\r\n",
        imap_quoted(username),
        imap_quoted(password)
    );
    reader
        .get_mut()
        .write_all(command.as_bytes())
        .map_err(|err| MailImapLoginError::Network(format!("write LOGIN command: {err}")))?;
    reader
        .get_mut()
        .flush()
        .map_err(|err| MailImapLoginError::Network(format!("flush LOGIN command: {err}")))?;

    let response = read_imap_tagged_response(&mut reader, tag)?;
    let sanitized = sanitize_imap_response(&response);
    if response.starts_with(&format!("{tag} OK")) {
        let logout = "A0002 LOGOUT\r\n";
        let _ = reader.get_mut().write_all(logout.as_bytes());
        let _ = reader.get_mut().flush();
        return Ok(format!("Server response: {sanitized}."));
    }
    if response.starts_with(&format!("{tag} NO")) || response.starts_with(&format!("{tag} BAD")) {
        return Err(MailImapLoginError::AuthRejected(sanitized));
    }
    Err(MailImapLoginError::Protocol(format!(
        "unexpected tagged response `{sanitized}`"
    )))
}

fn read_imap_line<R: BufRead>(reader: &mut R) -> Result<String, String> {
    let mut line = String::new();
    let read = reader
        .read_line(&mut line)
        .map_err(|err| format!("read IMAP response: {err}"))?;
    if read == 0 {
        return Err("server closed the IMAP connection".to_string());
    }
    Ok(line.trim_end_matches(['\r', '\n']).to_string())
}

fn read_imap_tagged_response<R: BufRead>(
    reader: &mut R,
    tag: &str,
) -> Result<String, MailImapLoginError> {
    for _ in 0..64 {
        let line = read_imap_line(reader).map_err(MailImapLoginError::Network)?;
        if line.starts_with(tag) {
            return Ok(line);
        }
    }
    Err(MailImapLoginError::Protocol(
        "IMAP server did not return a tagged LOGIN response".to_string(),
    ))
}

fn imap_quoted(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace(['\r', '\n'], " ")
}

fn sanitize_imap_response(value: &str) -> String {
    truncate_for_display(value.trim(), 240)
}

async fn http_status_check(
    service_name: &str,
    url: String,
    token: Option<String>,
    auth_header: Option<&str>,
) -> Result<NativeConnectionCheck, NativeConnectionCheck> {
    http_status_check_with_headers(service_name, url, token, auth_header, &[]).await
}

async fn http_status_check_with_headers(
    service_name: &str,
    url: String,
    token: Option<String>,
    auth_header: Option<&str>,
    extra_headers: &[(&str, String)],
) -> Result<NativeConnectionCheck, NativeConnectionCheck> {
    let mut request = ConnectorHttpRequest::new(service_name, Method::GET, url);
    for (header, value) in extra_headers {
        request = request.header(*header, value.clone());
    }
    if let (Some(header), Some(token)) = (auth_header, token.as_deref()) {
        request = request.header(header, token);
    } else if let Some(token) = token.as_deref() {
        request = request.bearer_token(token);
    }
    match http::send_connector_request(request).await {
        Ok(response) => Ok(NativeConnectionCheck::ok(service_name, response.status)),
        Err(err) => Err(NativeConnectionCheck::from_http_error(err)),
    }
}

async fn http_post_json_status_check(
    service_name: &str,
    url: String,
    body: serde_json::Value,
    token: Option<String>,
    auth_header: Option<&str>,
    extra_headers: &[(&str, String)],
) -> Result<NativeConnectionCheck, NativeConnectionCheck> {
    let mut request = ConnectorHttpRequest::new(service_name, Method::POST, url).json_body(body);
    for (header, value) in extra_headers {
        request = request.header(*header, value.clone());
    }
    if let (Some(header), Some(token)) = (auth_header, token.as_deref()) {
        request = request.header(header, token);
    } else if let Some(token) = token.as_deref() {
        request = request.bearer_token(token);
    }
    match http::send_connector_request(request).await {
        Ok(response) => Ok(NativeConnectionCheck::ok(service_name, response.status)),
        Err(err) => Err(NativeConnectionCheck::from_http_error(err)),
    }
}

fn connector_native_api_check_result(
    connector: &ConnectorInfo,
    check: Result<NativeConnectionCheck, NativeConnectionCheck>,
) -> ConnectorConnectionTestResult {
    match check {
        Ok(check) => connector_test_result_from_check(
            connector,
            true,
            ConnectorConnectionTestKind::NativeApi,
            check,
        ),
        Err(check) => connector_test_result_from_check(
            connector,
            false,
            ConnectorConnectionTestKind::NativeApi,
            check,
        ),
    }
}

fn truncate_for_display(value: &str, max_chars: usize) -> String {
    let mut out = value.chars().take(max_chars).collect::<String>();
    if value.chars().count() > max_chars {
        out.push('…');
    }
    out
}

async fn test_native_connector_connection(
    connector: &ConnectorInfo,
) -> ConnectorConnectionTestResult {
    match connector.definition.id.as_str() {
        "github" => {
            let token = github_token();
            let endpoint = if token.is_some() { "user" } else { "rate_limit" };
            let url = format!("{}/{}", github_api_base_url(), endpoint);
            connector_native_api_check_result(connector, http_status_check("GitHub", url, token, None).await)
        }
        "gmail" => {
            let token = oauth::gmail_oauth_token_or_refresh()
                .await
                .or_else(|| first_non_empty_env(GMAIL_OAUTH_ENV_VARS));
            if let Some(token) = token {
                connector_native_api_check_result(
                    connector,
                    http_status_check(
                        "Gmail",
                        format!("{}/users/me/profile", gmail_api_base_url()),
                        Some(token),
                        None,
                    )
                    .await,
                )
            } else if mail_credentials_configured("gmail") {
                let check = mail_credential_check(connector);
                connector_test_result_from_check(
                    connector,
                    check.error_code.is_none(),
                    ConnectorConnectionTestKind::LocalState,
                    check,
                )
            } else {
                connector_test_result_from_check(
                    connector,
                    false,
                    ConnectorConnectionTestKind::NativeApi,
                    NativeConnectionCheck::missing_credentials(
                        "Gmail",
                        "Gmail browser OAuth login or Google app password fallback",
                    ),
                )
            }
        }
        "gitlab" => {
            let token = gitlab_token();
            let endpoint = if token.is_some() { "user" } else { "version" };
            let url = format!("{}/{}", gitlab_api_base_url(), endpoint);
            connector_native_api_check_result(
                connector,
                http_status_check("GitLab", url, token, Some("PRIVATE-TOKEN")).await,
            )
        }
        "linear" => {
            let Some(authorization) = linear_authorization_header() else {
                return connector_test_result_from_check(
                    connector,
                    false,
                    ConnectorConnectionTestKind::NativeApi,
                    NativeConnectionCheck::missing_credentials(
                        "Linear",
                        "LINEAR_API_KEY or LINEAR_ACCESS_TOKEN",
                    ),
                );
            };
            connector_native_api_check_result(
                connector,
                http_post_json_status_check(
                    "Linear",
                    linear_graphql_url(),
                    serde_json::json!({ "query": "query OmigaConnectorTest { viewer { id name email } }" }),
                    Some(authorization),
                    Some("Authorization"),
                    &[],
                )
                .await,
            )
        }
        "notion" => {
            let Some(token) = notion_token() else {
                return connector_test_result_from_check(
                    connector,
                    false,
                    ConnectorConnectionTestKind::NativeApi,
                    NativeConnectionCheck::missing_credentials(
                        "Notion",
                        "Notion browser OAuth login or NOTION_TOKEN/NOTION_API_KEY advanced credentials",
                    ),
                );
            };
            connector_native_api_check_result(
                connector,
                http_status_check_with_headers(
                    "Notion",
                    format!("{}/users/me", notion_api_base_url()),
                    Some(token),
                    None,
                    &[("Notion-Version", notion_version())],
                )
                .await,
            )
        }
        "sentry" => {
            let Some(token) = sentry_token() else {
                return connector_test_result_from_check(
                    connector,
                    false,
                    ConnectorConnectionTestKind::NativeApi,
                    NativeConnectionCheck::missing_credentials("Sentry", "SENTRY_AUTH_TOKEN"),
                );
            };
            connector_native_api_check_result(
                connector,
                http_status_check(
                    "Sentry",
                    format!("{}/organizations/", sentry_api_base_url()),
                    Some(token),
                    None,
                )
                .await,
            )
        }
        "slack" => {
            let Some(token) = slack_token() else {
                return connector_test_result_from_check(
                    connector,
                    false,
                    ConnectorConnectionTestKind::NativeApi,
                    NativeConnectionCheck::missing_credentials(
                        "Slack",
                        "Slack browser OAuth login or SLACK_BOT_TOKEN advanced credentials",
                    ),
                );
            };
            connector_native_api_check_result(connector, slack_auth_test_check(token).await)
        }
        "qq_mail" | "netease_mail" => {
            let check = mail_credential_check(connector);
            connector_test_result_from_check(
                connector,
                check.error_code.is_none(),
                ConnectorConnectionTestKind::LocalState,
                check,
            )
        }
        "discord" => {
            let Some(authorization) = discord_authorization_header() else {
                return connector_test_result_from_check(
                    connector,
                    false,
                    ConnectorConnectionTestKind::NativeApi,
                    NativeConnectionCheck::missing_credentials("Discord", "DISCORD_BOT_TOKEN"),
                );
            };
            connector_native_api_check_result(
                connector,
                http_status_check(
                    "Discord",
                    format!("{}/users/@me", discord_api_base_url()),
                    Some(authorization),
                    Some("Authorization"),
                )
                .await,
            )
        }
        "jira" => {
            let (Some(base_url), Some(authorization)) =
                (jira_api_base_url(), jira_authorization_header())
            else {
                return connector_test_result_from_check(
                    connector,
                    false,
                    ConnectorConnectionTestKind::NativeApi,
                    NativeConnectionCheck::missing_credentials(
                        "Jira",
                        "JIRA_SITE_URL or OMIGA_JIRA_API_BASE_URL plus JIRA_EMAIL and JIRA_API_TOKEN",
                    ),
                );
            };
            connector_native_api_check_result(
                connector,
                http_status_check(
                    "Jira",
                    format!("{base_url}/myself"),
                    Some(authorization),
                    Some("Authorization"),
                )
                .await,
            )
        }
        "confluence" => {
            let (Some(base_url), Some(authorization)) = (
                confluence_api_base_url(),
                confluence_authorization_header(),
            ) else {
                return connector_test_result_from_check(
                    connector,
                    false,
                    ConnectorConnectionTestKind::NativeApi,
                    NativeConnectionCheck::missing_credentials(
                        "Confluence",
                        "CONFLUENCE_SITE_URL or OMIGA_CONFLUENCE_API_BASE_URL plus CONFLUENCE_EMAIL and CONFLUENCE_API_TOKEN",
                    ),
                );
            };
            connector_native_api_check_result(
                connector,
                http_status_check(
                    "Confluence",
                    format!("{base_url}/user/current"),
                    Some(authorization),
                    Some("Authorization"),
                )
                .await,
            )
        }
        _ => connector_test_result(
            connector,
            true,
            ConnectorConnectionTestKind::LocalState,
            "Connector is accessible from user-level state, but no native live API test is implemented yet.",
        ),
    }
}

async fn test_connector_connection_from_catalog(
    connector_id: &str,
    catalog: &ConnectorCatalog,
) -> Result<ConnectorConnectionTestResult, String> {
    let connector_id = validate_connector_id(connector_id)?;
    let connector = catalog
        .connectors
        .iter()
        .find(|item| item.definition.id == connector_id)
        .ok_or_else(|| format!("connector `{connector_id}` is not known"))?;

    if !connector.enabled || connector.status == ConnectorConnectionStatus::Disabled {
        return Ok(connector_test_result_with_observability(
            connector,
            false,
            ConnectorConnectionTestKind::LocalState,
            "Connector is disabled in user-level settings.",
            None,
            false,
            Some("disabled".to_string()),
            Some(
                "Enable this connector in Settings → Connectors before testing live access."
                    .to_string(),
            ),
        ));
    }

    if connector.status == ConnectorConnectionStatus::MetadataOnly {
        return Ok(connector_test_result_with_observability(
            connector,
            false,
            ConnectorConnectionTestKind::LocalState,
            "Connector is a plugin metadata reference; add a first-class connector definition or matching MCP/native tool before testing live access.",
            None,
            false,
            Some("metadata_only".to_string()),
            Some("Install or create a native connector/tool implementation for this plugin-declared service.".to_string()),
        ));
    }

    if !connector.accessible {
        let credential_hint = if connector.definition.env_vars.is_empty() {
            "Supported connection methods: browser/software login when available.".to_string()
        } else {
            format!(
                "Supported connection methods: browser/software login when available; advanced credential providers may expose {} outside connectors/config.json.",
                connector.definition.env_vars.join(" or ")
            )
        };
        return Ok(connector_test_result_with_observability(
            connector,
            false,
            ConnectorConnectionTestKind::LocalState,
            "Connector is not connected. Complete the provider/browser login or configure a supported external credential provider, then test again.",
            None,
            false,
            Some("needs_auth".to_string()),
            Some(credential_hint),
        ));
    }

    if connector_auth_source_is_codex_apps(connector) {
        return Ok(connector_test_result_with_observability(
            connector,
            true,
            ConnectorConnectionTestKind::LocalState,
            "Connector is available through the Codex/OpenAI apps MCP bridge.",
            None,
            false,
            Some("codex_apps_bridge".to_string()),
            Some(
                "This connector is backed by MCP app tools; local native API token checks are intentionally skipped."
                    .to_string(),
            ),
        ));
    }

    if native_connector_test_available(&connector.definition.id) {
        Ok(test_native_connector_connection(connector).await)
    } else {
        Ok(connector_test_result_with_observability(
            connector,
            true,
            ConnectorConnectionTestKind::LocalState,
            "Connector is accessible from user-level state; no native live API test is implemented yet.",
            None,
            false,
            Some("no_native_test".to_string()),
            Some("User-level state is valid, but this connector has no live API checker yet.".to_string()),
        ))
    }
}

fn persist_connector_test_result_at_path(
    path: &Path,
    result: &ConnectorConnectionTestResult,
) -> Result<(), String> {
    let connector_id = validate_connector_id(&result.connector_id)?;
    let mut config = read_config_from_path(path);
    let entry = config
        .connectors
        .entry(connector_id)
        .or_insert_with(|| ConnectorConfigEntry {
            enabled: true,
            connected: false,
            account_label: None,
            auth_source: None,
            connected_at: None,
            use_env_credentials: true,
            last_connection_test: None,
            connection_test_history: Vec::new(),
        });
    entry.last_connection_test = Some(result.clone());
    entry.connection_test_history.retain(|item| {
        !(item.connector_id == result.connector_id && item.checked_at == result.checked_at)
    });
    entry.connection_test_history.insert(0, result.clone());
    entry
        .connection_test_history
        .truncate(CONNECTOR_TEST_HISTORY_LIMIT);
    write_config_to_path(path, &config)
}

fn persist_connector_test_result(result: &ConnectorConnectionTestResult) -> Result<(), String> {
    persist_connector_test_result_at_path(&connector_config_path(), result)
}

fn connector_bridge_project_root(project_root: Option<&Path>) -> PathBuf {
    project_root
        .map(Path::to_path_buf)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

pub async fn test_connector_connection(
    connector_id: &str,
    project_root: Option<&Path>,
) -> Result<ConnectorConnectionTestResult, String> {
    let catalog = list_connector_catalog();
    let result = test_connector_connection_from_catalog(connector_id, &catalog).await?;
    if !result.ok && result.error_code.as_deref() == Some("needs_auth") {
        if let Some(connector) = catalog
            .connectors
            .iter()
            .find(|item| item.definition.id == result.connector_id)
        {
            let project_root = connector_bridge_project_root(project_root);
            if let Some(bridge) =
                discover_codex_apps_bridge_for_connector(&project_root, connector).await
            {
                let connected = connect_connector(ConnectorConnectRequest {
                    connector_id: connector.definition.id.clone(),
                    account_label: bridge
                        .connector_name
                        .clone()
                        .or_else(|| Some(connector.definition.name.clone())),
                    auth_source: Some("codex_apps".to_string()),
                })?;
                let result = connector_app_bridge_test_result(&connected, &bridge);
                persist_connector_test_result(&result)?;
                return Ok(result);
            }
        }
    }
    persist_connector_test_result(&result)?;
    Ok(result)
}

fn native_connector_tool_operations(connector_id: &str) -> &'static [&'static str] {
    match connector_id {
        "github" => &[
            "list_issues",
            "read_issue",
            "list_pull_requests",
            "read_pull_request",
        ],
        "gitlab" => &[
            "list_issues",
            "read_issue",
            "list_merge_requests",
            "read_merge_request",
        ],
        "linear" => &["list_issues", "read_issue"],
        "notion" => &["search_pages", "read_page"],
        "sentry" => &["list_issues", "read_issue"],
        "slack" => &["read_thread", "post_message"],
        _ => &[],
    }
}

pub fn format_connectors_system_section(catalog: &ConnectorCatalog) -> Option<String> {
    let active = catalog
        .connectors
        .iter()
        .filter(|connector| connector.enabled && connector.accessible)
        .collect::<Vec<_>>();
    if active.is_empty() {
        return None;
    }

    let mut lines = vec![
        "## Connectors (available)".to_string(),
        "Connectors are user-level, user-authorized external service links shared across projects on this machine. Prefer connector-backed MCP/tools only when the matching tool is explicitly available; do not invent external-service access from connector metadata alone.".to_string(),
        String::new(),
    ];
    for connector in active {
        let native_operations = native_connector_tool_operations(&connector.definition.id);
        let tools = if connector_auth_source_is_codex_apps(connector) {
            format!(
                "Codex/OpenAI apps MCP tools for `{}` when listed by the `codex_apps` MCP server",
                connector.definition.name
            )
        } else if !native_operations.is_empty() {
            format!(
                "native `connector` operations: {}",
                native_operations.join(", ")
            )
        } else if connector.definition.tools.is_empty() {
            "metadata only".to_string()
        } else {
            let declared = connector
                .definition
                .tools
                .iter()
                .map(|tool| tool.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            format!("declared tools require matching MCP/plugin/native executor: {declared}")
        };
        let account = connector
            .account_label
            .as_deref()
            .map(|label| format!(" account={label};"))
            .unwrap_or_default();
        lines.push(format!(
            "- `{}` ({}){} tools: {}",
            connector.definition.name, connector.definition.id, account, tools
        ));
    }
    Some(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::{JsonObject, Meta, Tool};
    use serde_json::json;
    use std::borrow::Cow;
    use std::net::TcpListener;
    use std::sync::Arc;
    use tempfile::tempdir;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    struct ScopedEnv {
        previous: Vec<(String, Option<String>)>,
    }

    impl ScopedEnv {
        fn set(vars: &[(&str, String)]) -> Self {
            let previous = vars
                .iter()
                .map(|(name, _)| ((*name).to_string(), std::env::var(name).ok()))
                .collect::<Vec<_>>();
            for (name, value) in vars {
                std::env::set_var(name, value);
            }
            Self { previous }
        }
    }

    impl Drop for ScopedEnv {
        fn drop(&mut self) {
            for (name, value) in self.previous.iter().rev() {
                if let Some(value) = value {
                    std::env::set_var(name, value);
                } else {
                    std::env::remove_var(name);
                }
            }
        }
    }

    fn local_no_proxy() -> String {
        "127.0.0.1,localhost".to_string()
    }

    async fn start_connector_mock_server() -> Option<MockServer> {
        let listener = match TcpListener::bind("127.0.0.1:0") {
            Ok(listener) => listener,
            Err(err) => {
                eprintln!("skipping connector HTTP mock test: cannot bind localhost: {err}");
                return None;
            }
        };
        Some(MockServer::builder().listener(listener).start().await)
    }

    fn connected_catalog_for(connector_ids: &[&str]) -> ConnectorCatalog {
        let definitions = builtin_connector_definitions();
        ConnectorCatalog {
            connectors: connector_ids
                .iter()
                .map(|connector_id| {
                    let definition = definitions
                        .iter()
                        .find(|definition| definition.id == *connector_id)
                        .cloned()
                        .unwrap_or_else(|| panic!("missing built-in connector {connector_id}"));
                    connector_info(
                        definition,
                        ConnectorDefinitionSource::BuiltIn,
                        Some(&ConnectorConfigEntry {
                            enabled: true,
                            connected: true,
                            account_label: None,
                            auth_source: Some("manual".to_string()),
                            connected_at: None,
                            use_env_credentials: true,
                            last_connection_test: None,
                            connection_test_history: Vec::new(),
                        }),
                        Vec::new(),
                    )
                })
                .collect(),
            scope: "user".to_string(),
            config_path: "connectors.json".to_string(),
            notes: Vec::new(),
        }
    }

    fn codex_app_tool(
        tool_name: &str,
        connector_id: &str,
        connector_name: &str,
        connector_description: Option<&str>,
    ) -> Tool {
        let mut tool = Tool::new(
            Cow::Owned(tool_name.to_string()),
            Cow::Borrowed("Codex app test tool"),
            Arc::new(JsonObject::default()),
        );
        let mut meta = serde_json::Map::new();
        meta.insert("connector_id".to_string(), json!(connector_id));
        meta.insert("connector_name".to_string(), json!(connector_name));
        if let Some(description) = connector_description {
            meta.insert("connector_description".to_string(), json!(description));
        }
        tool.meta = Some(Meta(meta));
        tool
    }

    #[test]
    fn plugin_refs_create_metadata_only_connector() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("connectors.json");
        let plugin_refs =
            HashMap::from([("calendar".to_string(), vec!["Calendar Plugin".to_string()])]);
        let catalog =
            list_connector_catalog_from_path(&config_path, &["calendar".to_string()], &plugin_refs);
        let calendar = catalog
            .connectors
            .iter()
            .find(|connector| connector.definition.id == "calendar")
            .expect("calendar connector");
        assert_eq!(calendar.status, ConnectorConnectionStatus::MetadataOnly);
        assert_eq!(calendar.referenced_by_plugins, vec!["Calendar Plugin"]);
    }

    #[test]
    fn config_drives_enabled_and_connected_state() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("connectors.json");
        let config = ConnectorConfigFile {
            connectors: HashMap::from([(
                "github".to_string(),
                ConnectorConfigEntry {
                    enabled: true,
                    connected: true,
                    account_label: Some("octo".to_string()),
                    auth_source: Some("manual".to_string()),
                    connected_at: Some("2026-05-03T00:00:00Z".to_string()),
                    use_env_credentials: true,
                    last_connection_test: None,
                    connection_test_history: Vec::new(),
                },
            )]),
            custom_connectors: Vec::new(),
        };
        write_config_to_path(&config_path, &config).unwrap();
        let catalog = list_connector_catalog_from_path(&config_path, &[], &HashMap::new());
        let github = catalog
            .connectors
            .iter()
            .find(|connector| connector.definition.id == "github")
            .expect("github connector");
        assert!(github.connected);
        assert!(github.accessible);
        assert_eq!(github.account_label.as_deref(), Some("octo"));
        assert_eq!(catalog.scope, "user");
        assert!(catalog.notes.iter().any(|note| note.contains("user-level")));
        assert_eq!(github.source, ConnectorDefinitionSource::BuiltIn);
    }

    #[test]
    fn connector_permission_audit_intent_handles_nested_tool_arguments() {
        let intent = connector_permission_audit_intent(
            "execute_tool",
            &json!({
                "tool": "Connector",
                "arguments": {
                    "connector": "Slack",
                    "operation": "send_message",
                    "channel": "C123",
                    "threadTs": "1712345678.123456",
                    "text": "Ship it",
                    "confirmWrite": true
                }
            }),
        )
        .expect("connector permission audit intent");

        assert_eq!(intent.connector_id, "slack");
        assert_eq!(intent.operation, "post_message");
        assert_eq!(intent.access, ConnectorAuditAccess::Write);
        assert!(intent.confirmation_required);
        assert!(intent.confirmed);
        assert_eq!(
            intent.target.as_deref(),
            Some("C123 thread 1712345678.123456")
        );
    }

    #[tokio::test]
    async fn permission_denial_audit_records_connector_write_without_confirming() {
        let _lock = CONNECTOR_TEST_ENV_LOCK.lock().await;
        let dir = tempdir().unwrap();
        let audit_path = dir.path().join("audit.jsonl");
        let _env = ScopedEnv::set(&[(
            "OMIGA_CONNECTOR_AUDIT_PATH",
            audit_path.to_string_lossy().to_string(),
        )]);

        let event = append_connector_permission_denial_audit_event(
            "connector",
            &json!({
                "connector": "slack",
                "operation": "post_message",
                "channel": "C123",
                "thread_ts": "1712345678.123456",
                "text": "Ship it",
                "confirm_write": true
            }),
            Some("session-1"),
            Some(dir.path()),
            "用户拒绝",
        )
        .expect("append denial audit")
        .expect("connector audit event");

        assert_eq!(event.connector_id, "slack");
        assert_eq!(event.operation, "post_message");
        assert_eq!(event.access, ConnectorAuditAccess::Write);
        assert_eq!(event.outcome, ConnectorAuditOutcome::Blocked);
        assert!(event.confirmation_required);
        assert!(!event.confirmed);
        assert_eq!(event.error_code.as_deref(), Some("user_denied"));
        assert_eq!(
            event.target.as_deref(),
            Some("C123 thread 1712345678.123456")
        );

        let events = list_connector_audit_events(Some("slack"), Some(10)).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0], event);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn github_cli_auth_makes_github_accessible_without_storing_token() {
        let _lock = CONNECTOR_TEST_ENV_LOCK.lock().await;
        let dir = tempdir().unwrap();
        let gh_path = dir.path().join("gh");
        std::fs::write(
            &gh_path,
            "#!/bin/sh\nif [ \"$1\" = \"auth\" ] && [ \"$2\" = \"token\" ]; then echo gh-cli-token; exit 0; fi\nexit 1\n",
        )
        .unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&gh_path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let secret_dir = dir.path().join("secrets");
        let _env = ScopedEnv::set(&[
            (
                "OMIGA_GITHUB_CLI_PATH",
                gh_path.to_string_lossy().to_string(),
            ),
            (
                "OMIGA_CONNECTOR_SECRET_STORE_DIR",
                secret_dir.to_string_lossy().to_string(),
            ),
            ("OMIGA_TEST_ENABLE_GITHUB_CLI_AUTH", "1".to_string()),
            ("OMIGA_DISABLE_GITHUB_CLI_AUTH", String::new()),
            ("GITHUB_TOKEN", String::new()),
            ("GH_TOKEN", String::new()),
        ]);

        assert_eq!(github_token().as_deref(), Some("gh-cli-token"));

        let definition = builtin_connector_definitions()
            .into_iter()
            .find(|definition| definition.id == "github")
            .unwrap();
        let github = connector_info(
            definition,
            ConnectorDefinitionSource::BuiltIn,
            None,
            Vec::new(),
        );

        assert!(github.connected);
        assert!(github.accessible);
        assert_eq!(github.auth_source.as_deref(), Some("github_cli"));
        assert!(!github.env_configured);
    }

    #[tokio::test]
    async fn notion_oauth_secret_makes_notion_accessible_without_env_token() {
        let _lock = CONNECTOR_TEST_ENV_LOCK.lock().await;
        let dir = tempdir().unwrap();
        let secret_dir = dir.path().join("secrets");
        let _env = ScopedEnv::set(&[
            (
                "OMIGA_CONNECTOR_SECRET_STORE_DIR",
                secret_dir.to_string_lossy().to_string(),
            ),
            ("NOTION_TOKEN", String::new()),
            ("NOTION_API_KEY", String::new()),
        ]);

        secret_store::store_connector_secret("notion", "oauth_access_token", "notion-oauth-token")
            .unwrap();
        assert_eq!(notion_token().as_deref(), Some("notion-oauth-token"));

        let definition = builtin_connector_definitions()
            .into_iter()
            .find(|definition| definition.id == "notion")
            .unwrap();
        let notion = connector_info(
            definition,
            ConnectorDefinitionSource::BuiltIn,
            None,
            Vec::new(),
        );

        assert!(notion.connected);
        assert!(notion.accessible);
        assert_eq!(notion.auth_source.as_deref(), Some("oauth_browser"));
        assert!(!notion.env_configured);
    }

    #[tokio::test]
    async fn slack_oauth_secret_makes_slack_accessible_without_env_token() {
        let _lock = CONNECTOR_TEST_ENV_LOCK.lock().await;
        let dir = tempdir().unwrap();
        let secret_dir = dir.path().join("secrets");
        let _env = ScopedEnv::set(&[
            (
                "OMIGA_CONNECTOR_SECRET_STORE_DIR",
                secret_dir.to_string_lossy().to_string(),
            ),
            ("SLACK_BOT_TOKEN", String::new()),
        ]);

        secret_store::store_connector_secret("slack", "oauth_access_token", "xoxb-oauth-token")
            .unwrap();
        assert_eq!(slack_token().as_deref(), Some("xoxb-oauth-token"));

        let definition = builtin_connector_definitions()
            .into_iter()
            .find(|definition| definition.id == "slack")
            .unwrap();
        let slack = connector_info(
            definition,
            ConnectorDefinitionSource::BuiltIn,
            None,
            Vec::new(),
        );

        assert!(slack.connected);
        assert!(slack.accessible);
        assert_eq!(slack.auth_source.as_deref(), Some("oauth_browser"));
        assert!(!slack.env_configured);
    }

    #[tokio::test]
    async fn gmail_oauth_secret_makes_gmail_accessible_before_mail_fallback() {
        let _lock = CONNECTOR_TEST_ENV_LOCK.lock().await;
        let dir = tempdir().unwrap();
        let secret_dir = dir.path().join("secrets");
        let _env = ScopedEnv::set(&[
            (
                "OMIGA_CONNECTOR_SECRET_STORE_DIR",
                secret_dir.to_string_lossy().to_string(),
            ),
            ("GMAIL_ACCESS_TOKEN", String::new()),
            ("GOOGLE_OAUTH_ACCESS_TOKEN", String::new()),
            ("GMAIL_ADDRESS", String::new()),
            ("GMAIL_USERNAME", String::new()),
            ("GMAIL_APP_PASSWORD", String::new()),
            ("GMAIL_AUTH_CODE", String::new()),
        ]);

        secret_store::store_connector_secret("gmail", "oauth_access_token", "gmail-oauth-token")
            .unwrap();
        assert_eq!(
            oauth::gmail_oauth_token().as_deref(),
            Some("gmail-oauth-token")
        );

        let definition = builtin_connector_definitions()
            .into_iter()
            .find(|definition| definition.id == "gmail")
            .unwrap();
        let gmail = connector_info(
            definition,
            ConnectorDefinitionSource::BuiltIn,
            None,
            Vec::new(),
        );

        assert!(gmail.connected);
        assert!(gmail.accessible);
        assert_eq!(gmail.status, ConnectorConnectionStatus::Connected);
        assert_eq!(gmail.auth_source.as_deref(), Some("oauth_browser"));
        assert!(!gmail.env_configured);
    }

    #[tokio::test]
    async fn gmail_env_oauth_token_makes_gmail_accessible_without_mail_fallback() {
        let _lock = CONNECTOR_TEST_ENV_LOCK.lock().await;
        let dir = tempdir().unwrap();
        let secret_dir = dir.path().join("secrets");
        let _env = ScopedEnv::set(&[
            (
                "OMIGA_CONNECTOR_SECRET_STORE_DIR",
                secret_dir.to_string_lossy().to_string(),
            ),
            ("GMAIL_ACCESS_TOKEN", "gmail-env-token".to_string()),
            ("GOOGLE_OAUTH_ACCESS_TOKEN", String::new()),
            ("GMAIL_ADDRESS", String::new()),
            ("GMAIL_USERNAME", String::new()),
            ("GMAIL_APP_PASSWORD", String::new()),
            ("GMAIL_AUTH_CODE", String::new()),
        ]);

        let definition = builtin_connector_definitions()
            .into_iter()
            .find(|definition| definition.id == "gmail")
            .unwrap();
        let gmail = connector_info(
            definition,
            ConnectorDefinitionSource::BuiltIn,
            None,
            Vec::new(),
        );

        assert!(gmail.connected);
        assert!(gmail.accessible);
        assert_eq!(gmail.status, ConnectorConnectionStatus::Connected);
        assert_eq!(gmail.auth_source.as_deref(), Some("environment"));
        assert!(gmail.env_configured);
    }

    #[tokio::test]
    async fn gmail_connection_test_reports_missing_credentials_without_network() {
        let _lock = CONNECTOR_TEST_ENV_LOCK.lock().await;
        let dir = tempdir().unwrap();
        let secret_dir = dir.path().join("secrets");
        let _env = ScopedEnv::set(&[
            (
                "OMIGA_CONNECTOR_SECRET_STORE_DIR",
                secret_dir.to_string_lossy().to_string(),
            ),
            ("GMAIL_ACCESS_TOKEN", String::new()),
            ("GOOGLE_OAUTH_ACCESS_TOKEN", String::new()),
            ("GMAIL_ADDRESS", String::new()),
            ("GMAIL_USERNAME", String::new()),
            ("GMAIL_APP_PASSWORD", String::new()),
            ("GMAIL_AUTH_CODE", String::new()),
        ]);

        let definition = builtin_connector_definitions()
            .into_iter()
            .find(|definition| definition.id == "gmail")
            .unwrap();
        let connector = connector_info(
            definition,
            ConnectorDefinitionSource::BuiltIn,
            None,
            Vec::new(),
        );
        let result = test_native_connector_connection(&connector).await;

        assert!(!result.ok);
        assert_eq!(result.check_kind, ConnectorConnectionTestKind::NativeApi);
        assert_eq!(result.error_code.as_deref(), Some("missing_credentials"));
        assert!(result.message.contains("requires"));
    }

    #[tokio::test]
    async fn mail_connector_validation_requires_account_and_app_password() {
        let _lock = CONNECTOR_TEST_ENV_LOCK.lock().await;
        let _env = ScopedEnv::set(&[
            ("OMIGA_MAIL_SKIP_NETWORK_CHECK", "1".to_string()),
            ("QQ_MAIL_ADDRESS", "user@qq.com".to_string()),
            ("QQ_MAIL_AUTH_CODE", "qq-auth-code".to_string()),
            ("QQ_MAIL_APP_PASSWORD", String::new()),
        ]);
        let definition = builtin_connector_definitions()
            .into_iter()
            .find(|definition| definition.id == "qq_mail")
            .unwrap();
        let catalog = ConnectorCatalog {
            connectors: vec![connector_info(
                definition,
                ConnectorDefinitionSource::BuiltIn,
                None,
                Vec::new(),
            )],
            scope: "user".to_string(),
            config_path: "connectors.json".to_string(),
            notes: Vec::new(),
        };
        let result = test_connector_connection_from_catalog("qq_mail", &catalog)
            .await
            .unwrap();

        assert!(result.ok);
        assert_eq!(result.check_kind, ConnectorConnectionTestKind::LocalState);
        assert!(result.message.contains("credentials are configured"));
        assert_eq!(result.auth_source.as_deref(), Some("environment"));
    }

    #[test]
    fn imap_login_command_values_are_quoted_without_leaking_control_chars() {
        assert_eq!(imap_quoted(r#"name"test\user"#), r#"name\"test\\user"#);
        assert_eq!(imap_quoted("line\r\nbreak"), "line  break");
    }

    #[test]
    fn imap_tagged_response_reader_skips_untagged_lines() {
        let mut reader =
            BufReader::new(b"* CAPABILITY IMAP4rev1\r\nA0001 OK LOGIN completed\r\n".as_slice());
        let response = read_imap_tagged_response(&mut reader, "A0001").unwrap();
        assert_eq!(response, "A0001 OK LOGIN completed");
    }

    #[tokio::test]
    async fn mail_connector_credentials_are_saved_in_secret_store() {
        let _lock = CONNECTOR_TEST_ENV_LOCK.lock().await;
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("connectors.json");
        let secret_dir = dir.path().join("secrets");
        let _env = ScopedEnv::set(&[
            (
                "OMIGA_CONNECTORS_CONFIG_PATH",
                config_path.to_string_lossy().to_string(),
            ),
            (
                "OMIGA_CONNECTOR_SECRET_STORE_DIR",
                secret_dir.to_string_lossy().to_string(),
            ),
            ("OMIGA_MAIL_SKIP_NETWORK_CHECK", "1".to_string()),
            ("QQ_MAIL_ADDRESS", String::new()),
            ("QQ_MAIL_AUTH_CODE", String::new()),
            ("QQ_MAIL_APP_PASSWORD", String::new()),
        ]);

        let connector = save_mail_connector_credentials(MailConnectorCredentialRequest {
            connector_id: "qq_mail".to_string(),
            email_address: "user@qq.com".to_string(),
            authorization_code: "qq-auth-code".to_string(),
        })
        .expect("save mail connector credentials");

        assert!(connector.accessible);
        assert_eq!(connector.account_label.as_deref(), Some("user@qq.com"));
        assert_eq!(connector.auth_source.as_deref(), Some("mail_credentials"));
        assert_eq!(mail_identity("qq_mail").as_deref(), Some("user@qq.com"));
        assert_eq!(mail_secret("qq_mail").as_deref(), Some("qq-auth-code"));

        let result = test_connector_connection("qq_mail", None)
            .await
            .expect("test mail connector");
        assert!(result.ok);
        assert_eq!(result.auth_source.as_deref(), Some("mail_credentials"));

        let config = fs::read_to_string(config_path).expect("read connector config");
        assert!(config.contains("user@qq.com"));
        assert!(!config.contains("qq-auth-code"));
    }

    #[tokio::test]
    async fn gmail_can_use_mail_app_password_credentials_without_oauth_config() {
        let _lock = CONNECTOR_TEST_ENV_LOCK.lock().await;
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("connectors.json");
        let secret_dir = dir.path().join("secrets");
        let _env = ScopedEnv::set(&[
            (
                "OMIGA_CONNECTORS_CONFIG_PATH",
                config_path.to_string_lossy().to_string(),
            ),
            (
                "OMIGA_CONNECTOR_SECRET_STORE_DIR",
                secret_dir.to_string_lossy().to_string(),
            ),
            ("OMIGA_MAIL_SKIP_NETWORK_CHECK", "1".to_string()),
            ("OMIGA_GMAIL_OAUTH_CLIENT_ID", String::new()),
            ("OMIGA_GMAIL_OAUTH_CLIENT_SECRET", String::new()),
            ("GMAIL_ACCESS_TOKEN", String::new()),
            ("GOOGLE_OAUTH_ACCESS_TOKEN", String::new()),
            ("GMAIL_ADDRESS", String::new()),
            ("GMAIL_USERNAME", String::new()),
            ("GMAIL_APP_PASSWORD", String::new()),
            ("GMAIL_AUTH_CODE", String::new()),
        ]);

        let connector = save_mail_connector_credentials(MailConnectorCredentialRequest {
            connector_id: "gmail".to_string(),
            email_address: "user@gmail.com".to_string(),
            authorization_code: "gmail-app-password".to_string(),
        })
        .expect("save Gmail app password credentials");

        assert!(connector.accessible);
        assert_eq!(connector.account_label.as_deref(), Some("user@gmail.com"));
        assert_eq!(connector.auth_source.as_deref(), Some("mail_credentials"));
        assert_eq!(mail_identity("gmail").as_deref(), Some("user@gmail.com"));
        assert_eq!(mail_secret("gmail").as_deref(), Some("gmail-app-password"));

        let result = test_connector_connection("gmail", None)
            .await
            .expect("test Gmail mail connector");
        assert!(result.ok);
        assert_eq!(result.auth_source.as_deref(), Some("mail_credentials"));
        assert!(result
            .message
            .contains("mailbox credentials are configured"));

        let config = fs::read_to_string(config_path).expect("read connector config");
        assert!(config.contains("user@gmail.com"));
        assert!(!config.contains("gmail-app-password"));
    }

    #[tokio::test]
    async fn github_oauth_secret_takes_precedence_over_env_token() {
        let _lock = CONNECTOR_TEST_ENV_LOCK.lock().await;
        let dir = tempdir().unwrap();
        let secret_dir = dir.path().join("secrets");
        let _env = ScopedEnv::set(&[
            (
                "OMIGA_CONNECTOR_SECRET_STORE_DIR",
                secret_dir.to_string_lossy().to_string(),
            ),
            ("OMIGA_DISABLE_GITHUB_CLI_AUTH", "1".to_string()),
            ("GITHUB_TOKEN", "github-env-token".to_string()),
            ("GH_TOKEN", String::new()),
        ]);

        secret_store::store_connector_secret("github", "oauth_access_token", "github-oauth-token")
            .unwrap();

        assert_eq!(github_token().as_deref(), Some("github-oauth-token"));

        let definition = builtin_connector_definitions()
            .into_iter()
            .find(|definition| definition.id == "github")
            .unwrap();
        let github = connector_info(
            definition,
            ConnectorDefinitionSource::BuiltIn,
            None,
            Vec::new(),
        );

        assert!(github.connected);
        assert!(github.accessible);
        assert_eq!(github.auth_source.as_deref(), Some("oauth_device"));
    }

    #[tokio::test]
    async fn disconnect_connector_ignores_first_class_external_fallbacks() {
        let _lock = CONNECTOR_TEST_ENV_LOCK.lock().await;
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("connectors.json");
        let secret_dir = dir.path().join("secrets");
        let _env = ScopedEnv::set(&[
            (
                "OMIGA_CONNECTORS_CONFIG_PATH",
                config_path.to_string_lossy().to_string(),
            ),
            (
                "OMIGA_CONNECTOR_SECRET_STORE_DIR",
                secret_dir.to_string_lossy().to_string(),
            ),
            ("OMIGA_DISABLE_GITHUB_CLI_AUTH", "1".to_string()),
            ("GITHUB_TOKEN", "github-env-token".to_string()),
            ("GH_TOKEN", String::new()),
            ("NOTION_TOKEN", "notion-env-token".to_string()),
            ("NOTION_API_KEY", String::new()),
            ("SLACK_BOT_TOKEN", "slack-env-token".to_string()),
        ]);

        for connector_id in ["github", "notion", "slack"] {
            let connector = disconnect_connector(connector_id).unwrap();
            assert_eq!(
                connector.status,
                ConnectorConnectionStatus::NeedsAuth,
                "{connector_id} should require auth after disconnect"
            );
            assert!(
                !connector.connected,
                "{connector_id} should be disconnected"
            );
            assert!(
                !connector.accessible,
                "{connector_id} should be inaccessible"
            );
            assert!(
                !connector.env_configured,
                "{connector_id} should hide ignored external credentials"
            );
            assert_eq!(
                connector.auth_source.as_deref(),
                None,
                "{connector_id} should not report ignored external credentials"
            );
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn github_disconnect_ignores_github_cli_token() {
        let _lock = CONNECTOR_TEST_ENV_LOCK.lock().await;
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("connectors.json");
        let secret_dir = dir.path().join("secrets");
        let gh_path = dir.path().join("gh");
        std::fs::write(
            &gh_path,
            "#!/bin/sh\nif [ \"$1\" = \"auth\" ] && [ \"$2\" = \"token\" ]; then echo gh-cli-token; exit 0; fi\nexit 1\n",
        )
        .unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&gh_path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let _env = ScopedEnv::set(&[
            (
                "OMIGA_CONNECTORS_CONFIG_PATH",
                config_path.to_string_lossy().to_string(),
            ),
            (
                "OMIGA_CONNECTOR_SECRET_STORE_DIR",
                secret_dir.to_string_lossy().to_string(),
            ),
            (
                "OMIGA_GITHUB_CLI_PATH",
                gh_path.to_string_lossy().to_string(),
            ),
            ("OMIGA_TEST_ENABLE_GITHUB_CLI_AUTH", "1".to_string()),
            ("OMIGA_DISABLE_GITHUB_CLI_AUTH", String::new()),
            ("GITHUB_TOKEN", String::new()),
            ("GH_TOKEN", String::new()),
        ]);

        assert_eq!(github_token().as_deref(), Some("gh-cli-token"));

        let github = disconnect_connector("github").unwrap();
        assert_eq!(github.status, ConnectorConnectionStatus::NeedsAuth);
        assert!(!github.connected);
        assert!(!github.accessible);
        assert_eq!(github.auth_source.as_deref(), None);
    }

    #[test]
    fn persisted_connection_test_result_round_trips_through_catalog() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("connectors.json");
        let result = ConnectorConnectionTestResult {
            connector_id: "github".to_string(),
            connector_name: "GitHub".to_string(),
            ok: false,
            status: ConnectorConnectionStatus::NeedsAuth,
            check_kind: ConnectorConnectionTestKind::NativeApi,
            message: "GitHub native API check requires GITHUB_TOKEN or GH_TOKEN.".to_string(),
            checked_at: "2026-05-04T00:00:00Z".to_string(),
            account_label: None,
            auth_source: None,
            http_status: Some(401),
            retryable: false,
            error_code: Some("http_401".to_string()),
            details: Some("Check credentials and endpoint configuration.".to_string()),
        };

        persist_connector_test_result_at_path(&config_path, &result).unwrap();

        let catalog = list_connector_catalog_from_path(&config_path, &[], &HashMap::new());
        let github = catalog
            .connectors
            .iter()
            .find(|connector| connector.definition.id == "github")
            .expect("github connector");
        let persisted = github
            .last_connection_test
            .as_ref()
            .expect("persisted connection test");
        assert_eq!(persisted.http_status, Some(401));
        assert_eq!(persisted.error_code.as_deref(), Some("http_401"));
        assert_eq!(persisted.details.as_deref(), result.details.as_deref());
        assert_eq!(persisted, &result);
        assert_eq!(github.connection_test_history, vec![result.clone()]);
        assert_eq!(github.connection_health.total_checks, 1);
        assert_eq!(github.connection_health.ok_checks, 0);
        assert_eq!(github.connection_health.failed_checks, 1);
        assert_eq!(
            github.connection_health.last_failure_at.as_deref(),
            Some("2026-05-04T00:00:00Z")
        );
        assert_eq!(
            github.connection_health.last_error_code.as_deref(),
            Some("http_401")
        );
    }

    #[test]
    fn persisted_connection_test_history_is_newest_first_and_capped() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("connectors.json");

        for index in 0..(CONNECTOR_TEST_HISTORY_LIMIT + 3) {
            let ok = index % 2 == 0;
            let result = ConnectorConnectionTestResult {
                connector_id: "github".to_string(),
                connector_name: "GitHub".to_string(),
                ok,
                status: ConnectorConnectionStatus::Connected,
                check_kind: ConnectorConnectionTestKind::NativeApi,
                message: format!("check {index}"),
                checked_at: format!("2026-05-04T00:00:{index:02}Z"),
                account_label: None,
                auth_source: Some("manual".to_string()),
                http_status: Some(if ok { 200 } else { 503 }),
                retryable: !ok,
                error_code: (!ok).then(|| "http_503".to_string()),
                details: None,
            };
            persist_connector_test_result_at_path(&config_path, &result).unwrap();
        }

        let catalog = list_connector_catalog_from_path(&config_path, &[], &HashMap::new());
        let github = catalog
            .connectors
            .iter()
            .find(|connector| connector.definition.id == "github")
            .expect("github connector");

        assert_eq!(
            github.connection_test_history.len(),
            CONNECTOR_TEST_HISTORY_LIMIT
        );
        assert_eq!(
            github.connection_test_history[0].checked_at,
            "2026-05-04T00:00:22Z"
        );
        assert_eq!(
            github.connection_test_history[CONNECTOR_TEST_HISTORY_LIMIT - 1].checked_at,
            "2026-05-04T00:00:03Z"
        );
        assert_eq!(github.connection_health.total_checks, 20);
        assert_eq!(github.connection_health.ok_checks, 10);
        assert_eq!(github.connection_health.failed_checks, 10);
        assert_eq!(github.connection_health.retryable_failures, 10);
        assert_eq!(
            github.connection_health.last_ok_at.as_deref(),
            Some("2026-05-04T00:00:22Z")
        );
        assert_eq!(
            github.connection_health.last_failure_at.as_deref(),
            Some("2026-05-04T00:00:21Z")
        );
    }

    #[test]
    fn upserts_and_deletes_custom_connector_from_user_config() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("connectors.json");

        let catalog = upsert_custom_connector_at_path(
            &config_path,
            CustomConnectorRequest {
                id: "Internal Docs".to_string(),
                name: "Internal Docs".to_string(),
                description: "Read internal documentation via a company MCP bridge.".to_string(),
                category: "Knowledge".to_string(),
                auth_type: ConnectorAuthType::EnvToken,
                env_vars: vec![" internal_docs_token ".to_string()],
                install_url: None,
                docs_url: Some("https://docs.example.com/api".to_string()),
                default_enabled: true,
                tools: vec![connector_tool("search_pages", "Search pages.", true)],
            },
        )
        .unwrap();

        let custom = catalog
            .connectors
            .iter()
            .find(|connector| connector.definition.id == "internal_docs")
            .expect("custom connector");
        assert_eq!(custom.source, ConnectorDefinitionSource::Custom);
        assert_eq!(custom.definition.category, "knowledge");
        assert_eq!(
            custom.definition.env_vars,
            vec!["INTERNAL_DOCS_TOKEN".to_string()]
        );
        assert_eq!(custom.definition.tools[0].name, "search_pages");

        let catalog = delete_custom_connector_at_path(&config_path, "internal_docs").unwrap();
        assert!(catalog
            .connectors
            .iter()
            .all(|connector| connector.definition.id != "internal_docs"));
    }

    #[test]
    fn exports_custom_connectors_without_connection_state() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("connectors.json");
        let config = ConnectorConfigFile {
            connectors: HashMap::from([(
                "internal_docs".to_string(),
                ConnectorConfigEntry {
                    enabled: true,
                    connected: true,
                    account_label: Some("docs".to_string()),
                    auth_source: Some("manual".to_string()),
                    connected_at: Some("2026-05-03T00:00:00Z".to_string()),
                    use_env_credentials: true,
                    last_connection_test: None,
                    connection_test_history: Vec::new(),
                },
            )]),
            custom_connectors: vec![ConnectorDefinition {
                id: "internal_docs".to_string(),
                name: "Internal Docs".to_string(),
                description: "Read internal docs.".to_string(),
                category: "knowledge".to_string(),
                auth_type: ConnectorAuthType::EnvToken,
                env_vars: vec!["INTERNAL_DOCS_TOKEN".to_string()],
                install_url: None,
                docs_url: Some("https://docs.example.com/api".to_string()),
                default_enabled: true,
                tools: vec![connector_tool("search_pages", "Search pages.", true)],
            }],
        };
        write_config_to_path(&config_path, &config).unwrap();

        let exported = export_custom_connectors_from_path(&config_path);
        assert_eq!(exported.version, 1);
        assert_eq!(exported.scope, "user");
        assert_eq!(exported.connectors.len(), 1);
        assert_eq!(exported.connectors[0].id, "internal_docs");
    }

    #[test]
    fn imports_custom_connectors_and_can_replace_existing() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("connectors.json");
        upsert_custom_connector_at_path(
            &config_path,
            CustomConnectorRequest {
                id: "old_docs".to_string(),
                name: "Old Docs".to_string(),
                description: "Old docs connector.".to_string(),
                category: "knowledge".to_string(),
                auth_type: ConnectorAuthType::EnvToken,
                env_vars: vec!["OLD_DOCS_TOKEN".to_string()],
                install_url: None,
                docs_url: None,
                default_enabled: true,
                tools: Vec::new(),
            },
        )
        .unwrap();

        let catalog = import_custom_connectors_at_path(
            &config_path,
            CustomConnectorImportRequest {
                replace_existing: true,
                connectors: vec![CustomConnectorRequest {
                    id: "new_docs".to_string(),
                    name: "New Docs".to_string(),
                    description: "New docs connector.".to_string(),
                    category: "knowledge".to_string(),
                    auth_type: ConnectorAuthType::EnvToken,
                    env_vars: vec!["NEW_DOCS_TOKEN".to_string()],
                    install_url: None,
                    docs_url: None,
                    default_enabled: true,
                    tools: vec![connector_tool("search_pages", "Search pages.", true)],
                }],
            },
        )
        .unwrap();

        assert!(catalog
            .connectors
            .iter()
            .any(|connector| connector.definition.id == "new_docs"
                && connector.source == ConnectorDefinitionSource::Custom));
        assert!(catalog
            .connectors
            .iter()
            .all(|connector| connector.definition.id != "old_docs"));
    }

    #[test]
    fn custom_connector_cannot_shadow_builtin() {
        let request = CustomConnectorRequest {
            id: "github".to_string(),
            name: "Custom GitHub".to_string(),
            description: "Should not replace the built-in connector.".to_string(),
            category: "code".to_string(),
            auth_type: ConnectorAuthType::EnvToken,
            env_vars: vec!["TOKEN".to_string()],
            install_url: None,
            docs_url: None,
            default_enabled: true,
            tools: Vec::new(),
        };
        let error = validate_custom_connector_request(request).unwrap_err();
        assert!(error.contains("built-in"));
    }

    #[test]
    fn builtins_include_common_user_level_connectors() {
        let definitions = builtin_connector_definitions();
        let ids = definitions
            .iter()
            .map(|definition| definition.id.as_str())
            .collect::<BTreeSet<_>>();
        for expected in [
            "github",
            "gitlab",
            "bitbucket",
            "azure_devops",
            "linear",
            "jira",
            "confluence",
            "slack",
            "discord",
            "microsoft_teams",
            "figma",
            "notion",
            "sentry",
            "google_drive",
            "google_sheets",
            "gmail",
            "qq_mail",
            "netease_mail",
            "outlook",
            "dropbox",
        ] {
            assert!(
                ids.contains(expected),
                "missing built-in connector {expected}"
            );
        }
        assert_eq!(
            ids.len(),
            definitions.len(),
            "built-in connector ids should be unique"
        );
        let slack = definitions
            .iter()
            .find(|definition| definition.id == "slack")
            .expect("slack connector");
        let post_message = slack
            .tools
            .iter()
            .find(|tool| tool.name == "post_message")
            .expect("slack post_message tool");
        assert_eq!(post_message.execution, ConnectorToolExecution::Native);
        assert!(!post_message.read_only);
        assert!(post_message.confirmation_required);
        assert!(post_message
            .required_scopes
            .iter()
            .any(|scope| scope == "chat:write"));
        let qq_mail = definitions
            .iter()
            .find(|definition| definition.id == "qq_mail")
            .expect("qq mail connector");
        assert_eq!(qq_mail.category, "email");
        assert!(qq_mail
            .tools
            .iter()
            .any(|tool| tool.name == "send_message" && !tool.read_only));
        let gmail = definitions
            .iter()
            .find(|definition| definition.id == "gmail")
            .expect("gmail connector");
        assert_eq!(gmail.auth_type, ConnectorAuthType::OAuth);
        assert!(gmail
            .env_vars
            .iter()
            .any(|name| name == "GMAIL_ACCESS_TOKEN"));
        assert!(gmail
            .env_vars
            .iter()
            .any(|name| name == "GMAIL_APP_PASSWORD"));
    }

    #[test]
    fn system_section_lists_only_accessible_connectors() {
        let catalog = ConnectorCatalog {
            connectors: vec![
                connector_info(
                    builtin_connector_definitions()
                        .into_iter()
                        .find(|definition| definition.id == "github")
                        .unwrap(),
                    ConnectorDefinitionSource::BuiltIn,
                    Some(&ConnectorConfigEntry {
                        enabled: true,
                        connected: true,
                        account_label: None,
                        auth_source: Some("manual".to_string()),
                        connected_at: None,
                        use_env_credentials: true,
                        last_connection_test: None,
                        connection_test_history: Vec::new(),
                    }),
                    Vec::new(),
                ),
                connector_info(
                    plugin_ref_definition("calendar"),
                    ConnectorDefinitionSource::Plugin,
                    None,
                    vec!["Calendar Plugin".to_string()],
                ),
            ],
            scope: "user".to_string(),
            config_path: "connectors.json".to_string(),
            notes: Vec::new(),
        };
        let section = format_connectors_system_section(&catalog).expect("section");
        assert!(section.contains("GitHub"));
        assert!(section.contains("native `connector` operations"));
        assert!(section.contains("read_issue"));
        assert!(!section.contains("Calendar Plugin"));
    }

    #[test]
    fn codex_apps_bridge_detects_connector_from_tool_metadata() {
        let connector = connector_info(
            builtin_connector_definitions()
                .into_iter()
                .find(|definition| definition.id == "slack")
                .unwrap(),
            ConnectorDefinitionSource::BuiltIn,
            None,
            Vec::new(),
        );

        let bridge = codex_apps_bridge_from_mcp_tools(
            "codex_apps",
            vec![
                codex_app_tool(
                    "read_thread",
                    "connector_slack_workspace",
                    "Slack",
                    Some("Slack workspace connector"),
                ),
                codex_app_tool("read_page", "connector_notion_workspace", "Notion", None),
            ],
            &connector,
        )
        .expect("slack bridge should be detected");

        assert_eq!(bridge.connector_name.as_deref(), Some("Slack"));
        assert_eq!(
            bridge.connector_description.as_deref(),
            Some("Slack workspace connector")
        );
        assert_eq!(bridge.tool_names, vec!["mcp__codex_apps__read_thread"]);
    }

    #[tokio::test]
    async fn connection_test_accepts_codex_apps_bridge_without_native_token() {
        let slack = builtin_connector_definitions()
            .into_iter()
            .find(|definition| definition.id == "slack")
            .unwrap();
        let catalog = ConnectorCatalog {
            connectors: vec![connector_info(
                slack,
                ConnectorDefinitionSource::BuiltIn,
                Some(&ConnectorConfigEntry {
                    enabled: true,
                    connected: true,
                    account_label: Some("Slack".to_string()),
                    auth_source: Some("codex_apps".to_string()),
                    connected_at: Some("2026-05-06T00:00:00Z".to_string()),
                    use_env_credentials: false,
                    last_connection_test: None,
                    connection_test_history: Vec::new(),
                }),
                Vec::new(),
            )],
            scope: "user".to_string(),
            config_path: "connectors.json".to_string(),
            notes: Vec::new(),
        };

        let result = test_connector_connection_from_catalog("slack", &catalog)
            .await
            .unwrap();

        assert!(result.ok);
        assert_eq!(result.check_kind, ConnectorConnectionTestKind::LocalState);
        assert_eq!(result.error_code.as_deref(), Some("codex_apps_bridge"));
        assert!(result.message.contains("Codex/OpenAI apps MCP bridge"));
    }

    #[tokio::test]
    async fn connection_test_reports_disabled_without_network() {
        let github = builtin_connector_definitions()
            .into_iter()
            .find(|definition| definition.id == "github")
            .unwrap();
        let catalog = ConnectorCatalog {
            connectors: vec![connector_info(
                github,
                ConnectorDefinitionSource::BuiltIn,
                Some(&ConnectorConfigEntry {
                    enabled: false,
                    connected: false,
                    account_label: None,
                    auth_source: None,
                    connected_at: None,
                    use_env_credentials: false,
                    last_connection_test: None,
                    connection_test_history: Vec::new(),
                }),
                Vec::new(),
            )],
            scope: "user".to_string(),
            config_path: "connectors.json".to_string(),
            notes: Vec::new(),
        };

        let result = test_connector_connection_from_catalog("github", &catalog)
            .await
            .unwrap();
        assert!(!result.ok);
        assert_eq!(result.status, ConnectorConnectionStatus::Disabled);
        assert_eq!(result.check_kind, ConnectorConnectionTestKind::LocalState);
        assert!(result.message.contains("disabled"));
    }

    #[tokio::test]
    async fn connection_test_reports_metadata_only_without_network() {
        let catalog = ConnectorCatalog {
            connectors: vec![connector_info(
                plugin_ref_definition("calendar"),
                ConnectorDefinitionSource::Plugin,
                None,
                vec!["Calendar Plugin".to_string()],
            )],
            scope: "user".to_string(),
            config_path: "connectors.json".to_string(),
            notes: Vec::new(),
        };

        let result = test_connector_connection_from_catalog("calendar", &catalog)
            .await
            .unwrap();
        assert!(!result.ok);
        assert_eq!(result.status, ConnectorConnectionStatus::MetadataOnly);
        assert_eq!(result.check_kind, ConnectorConnectionTestKind::LocalState);
        assert!(result.message.contains("metadata"));
    }

    #[tokio::test]
    async fn connection_test_accepts_accessible_non_native_local_state() {
        let asana = builtin_connector_definitions()
            .into_iter()
            .find(|definition| definition.id == "asana")
            .unwrap();
        let catalog = ConnectorCatalog {
            connectors: vec![connector_info(
                asana,
                ConnectorDefinitionSource::BuiltIn,
                Some(&ConnectorConfigEntry {
                    enabled: true,
                    connected: true,
                    account_label: Some("docs".to_string()),
                    auth_source: Some("manual".to_string()),
                    connected_at: Some("2026-05-03T00:00:00Z".to_string()),
                    use_env_credentials: true,
                    last_connection_test: None,
                    connection_test_history: Vec::new(),
                }),
                Vec::new(),
            )],
            scope: "user".to_string(),
            config_path: "connectors.json".to_string(),
            notes: Vec::new(),
        };

        let result = test_connector_connection_from_catalog("asana", &catalog)
            .await
            .unwrap();
        assert!(result.ok);
        assert_eq!(result.status, ConnectorConnectionStatus::Connected);
        assert_eq!(result.check_kind, ConnectorConnectionTestKind::LocalState);
        assert!(result.message.contains("no native live API test"));
    }

    #[tokio::test]
    async fn connection_test_supports_common_native_service_checks_offline() {
        let _lock = CONNECTOR_TEST_ENV_LOCK.lock().await;
        let Some(server) = start_connector_mock_server().await else {
            return;
        };
        let jira_auth = basic_auth_header("jira@example.com", "jira-token");
        let confluence_auth = basic_auth_header("docs@example.com", "confluence-token");
        let _env = ScopedEnv::set(&[
            (
                "OMIGA_SLACK_API_BASE_URL",
                format!("{}/slack", server.uri()),
            ),
            ("SLACK_BOT_TOKEN", "slack-test-token".to_string()),
            (
                "OMIGA_DISCORD_API_BASE_URL",
                format!("{}/discord", server.uri()),
            ),
            ("DISCORD_BOT_TOKEN", "discord-test-token".to_string()),
            (
                "OMIGA_JIRA_API_BASE_URL",
                format!("{}/jira/rest/api/3", server.uri()),
            ),
            ("JIRA_EMAIL", "jira@example.com".to_string()),
            ("JIRA_API_TOKEN", "jira-token".to_string()),
            (
                "OMIGA_CONFLUENCE_API_BASE_URL",
                format!("{}/confluence/rest/api", server.uri()),
            ),
            ("CONFLUENCE_EMAIL", "docs@example.com".to_string()),
            ("CONFLUENCE_API_TOKEN", "confluence-token".to_string()),
            ("NO_PROXY", local_no_proxy()),
            ("no_proxy", local_no_proxy()),
        ]);

        Mock::given(method("GET"))
            .and(path("/slack/auth.test"))
            .and(header("authorization", "Bearer slack-test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "ok": true,
                "team": "Acme",
                "user": "omiga"
            })))
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/discord/users/@me"))
            .and(header("authorization", "Bot discord-test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "123",
                "username": "omiga"
            })))
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/jira/rest/api/3/myself"))
            .and(header("authorization", jira_auth.as_str()))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "accountId": "jira-user"
            })))
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/confluence/rest/api/user/current"))
            .and(header("authorization", confluence_auth.as_str()))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "accountId": "docs-user"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let catalog = connected_catalog_for(&["slack", "discord", "jira", "confluence"]);
        for connector_id in ["slack", "discord", "jira", "confluence"] {
            let result = test_connector_connection_from_catalog(connector_id, &catalog)
                .await
                .unwrap();
            assert!(result.ok, "{connector_id}: {result:?}");
            assert_eq!(result.check_kind, ConnectorConnectionTestKind::NativeApi);
            assert_eq!(result.http_status, Some(200));
        }
    }

    #[tokio::test]
    async fn connection_test_reports_slack_application_auth_error() {
        let _lock = CONNECTOR_TEST_ENV_LOCK.lock().await;
        let Some(server) = start_connector_mock_server().await else {
            return;
        };
        let _env = ScopedEnv::set(&[
            (
                "OMIGA_SLACK_API_BASE_URL",
                format!("{}/slack", server.uri()),
            ),
            ("SLACK_BOT_TOKEN", "bad-slack-token".to_string()),
            ("NO_PROXY", local_no_proxy()),
            ("no_proxy", local_no_proxy()),
        ]);

        Mock::given(method("GET"))
            .and(path("/slack/auth.test"))
            .and(header("authorization", "Bearer bad-slack-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "ok": false,
                "error": "invalid_auth"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let catalog = connected_catalog_for(&["slack"]);
        let result = test_connector_connection_from_catalog("slack", &catalog)
            .await
            .unwrap();
        assert!(!result.ok);
        assert_eq!(result.check_kind, ConnectorConnectionTestKind::NativeApi);
        assert_eq!(result.http_status, Some(200));
        assert_eq!(result.error_code.as_deref(), Some("invalid_auth"));
        assert!(result
            .details
            .as_deref()
            .unwrap_or_default()
            .contains("SLACK_BOT_TOKEN"));
    }
}
