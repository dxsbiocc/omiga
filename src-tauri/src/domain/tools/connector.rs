//! Connector-backed external service tool.
//!
//! This is the native execution bridge for Omiga connectors. Native connectors are intentionally
//! read-only until each service gets a reviewed write-permission model.

use super::{ToolContext, ToolError, ToolSchema};
use crate::domain::connectors::http::{
    send_connector_json, ConnectorHttpError, ConnectorHttpRequest,
};
use crate::domain::connectors::{self, ConnectorConnectionStatus};
use crate::infrastructure::streaming::{stream_single, StreamOutputItem};
use async_trait::async_trait;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};

pub const DESCRIPTION: &str = r#"Call an enabled Omiga connector for external service context.

Use this tool only when the user asks for data from a configured external service. Current native implementation supports these operations:

GitHub: `connector="github"`
- `operation="list_issues"` with `repo="owner/name"`, optional `state` (`open`, `closed`, `all`) and `max_results`.
- `operation="read_issue"` with `repo="owner/name"` and `number`.
- `operation="list_pull_requests"` with `repo="owner/name"`, optional `state` and `max_results`.
- `operation="read_pull_request"` with `repo="owner/name"` and `number`.

GitLab: `connector="gitlab"`
- `operation="list_issues"` with `repo="group/project"`, optional `state` (`open`, `closed`, `all`) and `max_results`.
- `operation="read_issue"` with `repo="group/project"` and `number` (GitLab IID).
- `operation="list_merge_requests"` with `repo="group/project"`, optional `state` (`open`, `closed`, `merged`, `all`) and `max_results`.
- `operation="read_merge_request"` with `repo="group/project"` and `number` (GitLab IID).

Linear: `connector="linear"`
- `operation="list_issues"` with optional `max_results`.
- `operation="read_issue"` with `id` (Linear issue UUID or identifier such as `ENG-123`).

Notion: `connector="notion"`
- `operation="search_pages"` with optional `query` and `max_results`.
- `operation="read_page"` with `id` / `page_id`, optional `max_results` block budget and `max_depth` (0-3) for nested content.

Sentry: `connector="sentry"`
- `operation="list_issues"` with `repo="org/project"`, optional `query`, `state` (`open`, `resolved`, `all`) and `max_results`.
- `operation="read_issue"` with `repo="org/project"` or `org`, plus `id` / `issue_id`.

Slack: `connector="slack"`
- `operation="read_thread"` with `channel` and `thread_ts` / `id`, optional `max_results`.
- `operation="post_message"` with `channel`, `text`, optional `thread_ts`, and `confirm_write=true`.

Credentials are read from connector-specific secure storage, real local software, or advanced external credential providers. GitHub uses Omiga OAuth login, the local GitHub CLI login (`gh auth login` / `gh auth token`), or advanced `GITHUB_TOKEN`/`GH_TOKEN` fallbacks; Notion uses Omiga browser OAuth or advanced `NOTION_TOKEN`/`NOTION_API_KEY` fallbacks; Slack uses Omiga browser OAuth through an HTTPS callback bridge or advanced `SLACK_BOT_TOKEN`; GitLab uses `GITLAB_TOKEN`; Linear uses `LINEAR_API_KEY` or `LINEAR_ACCESS_TOKEN`; Sentry uses `SENTRY_AUTH_TOKEN`. Public unauthenticated reads are allowed only for GitHub/GitLab after the connector is connected through Settings → Connectors. This tool returns JSON; Slack post_message writes only when `confirm_write=true` is supplied after explicit user intent."#;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectorArgs {
    pub connector: String,
    pub operation: String,
    #[serde(default, alias = "repository")]
    pub repo: Option<String>,
    #[serde(
        default,
        alias = "issue_id",
        alias = "page_id",
        alias = "key",
        alias = "identifier"
    )]
    pub id: Option<String>,
    #[serde(default, alias = "search", alias = "term")]
    pub query: Option<String>,
    #[serde(default, alias = "organization", alias = "organization_slug")]
    pub org: Option<String>,
    #[serde(default, alias = "project_slug")]
    pub project: Option<String>,
    #[serde(
        default,
        alias = "issue",
        alias = "issue_number",
        alias = "pr",
        alias = "pull_number"
    )]
    pub number: Option<u64>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default, alias = "limit", alias = "per_page")]
    pub max_results: Option<u32>,
    #[serde(default)]
    pub max_depth: Option<u32>,
    #[serde(default)]
    pub channel: Option<String>,
    #[serde(default, alias = "thread", alias = "threadTs", alias = "thread_ts")]
    pub thread_ts: Option<String>,
    #[serde(default, alias = "message")]
    pub text: Option<String>,
    #[serde(default, alias = "confirmWrite")]
    pub confirm_write: bool,
}

pub struct ConnectorTool;

#[async_trait]
impl super::ToolImpl for ConnectorTool {
    type Args = ConnectorArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        let value = execute_connector_json(ctx, args).await?;
        let text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
        Ok(stream_single(StreamOutputItem::Text(text)))
    }
}

pub(crate) async fn execute_connector_json(
    ctx: &ToolContext,
    args: ConnectorArgs,
) -> Result<JsonValue, ToolError> {
    let connector = normalize_id(&args.connector);
    let operation = canonical_connector_operation(&connector, &normalize_id(&args.operation));
    let audit = connector_tool_audit_context(ctx, &connector, &operation, &args);
    let result = match connector.as_str() {
        "github" => execute_github_json(args).await,
        "gitlab" => execute_gitlab_json(args).await,
        "linear" => execute_linear_json(args).await,
        "notion" => execute_notion_json(args).await,
        "sentry" => execute_sentry_json(args).await,
        "slack" => execute_slack_json(args).await,
        other => Err(ToolError::InvalidArguments {
            message: format!(
                "Connector `{other}` does not have a native Omiga tool yet. Configure/use a matching MCP server or plugin-provided tool instead."
            ),
        }),
    };
    if let Some(audit) = audit {
        record_connector_tool_audit(audit, &result);
    }
    result
}

fn normalize_id(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace([' ', '-'], "_")
}

fn canonical_connector_operation(connector: &str, operation: &str) -> String {
    match connector {
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
        "linear" => match operation {
            "issues" => "list_issues",
            "get_issue" | "issue" => "read_issue",
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
        "slack" => match operation {
            "thread" | "replies" | "conversation_replies" => "read_thread",
            "send_message" | "reply" => "post_message",
            other => other,
        },
        _ => operation,
    }
    .to_string()
}

#[derive(Debug, Clone)]
struct ConnectorToolAuditContext {
    connector_id: String,
    operation: String,
    access: connectors::ConnectorAuditAccess,
    confirmation_required: bool,
    confirmed: bool,
    target: Option<String>,
    session_id: Option<String>,
    project_root: Option<String>,
}

fn connector_tool_audit_context(
    ctx: &ToolContext,
    connector_id: &str,
    operation: &str,
    args: &ConnectorArgs,
) -> Option<ConnectorToolAuditContext> {
    if !matches!(
        connector_id,
        "github" | "gitlab" | "linear" | "notion" | "sentry" | "slack"
    ) {
        return None;
    }
    let access = if matches!((connector_id, operation), ("slack", "post_message")) {
        connectors::ConnectorAuditAccess::Write
    } else {
        connectors::ConnectorAuditAccess::Read
    };
    let confirmation_required = matches!(access, connectors::ConnectorAuditAccess::Write);
    Some(ConnectorToolAuditContext {
        connector_id: connector_id.to_string(),
        operation: operation.to_string(),
        access,
        confirmation_required,
        confirmed: args.confirm_write,
        target: connector_audit_target(connector_id, operation, args),
        session_id: ctx.session_id.clone(),
        project_root: Some(ctx.project_root.to_string_lossy().into_owned()),
    })
}

fn connector_audit_target(
    connector_id: &str,
    operation: &str,
    args: &ConnectorArgs,
) -> Option<String> {
    match connector_id {
        "github" | "gitlab" => args
            .repo
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|repo| {
                if let Some(number) = args.number {
                    format!("{repo}#{number}")
                } else {
                    repo.to_string()
                }
            }),
        "linear" => args
            .id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| args.number.map(|number| number.to_string())),
        "notion" => args
            .id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| {
                args.query
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(|query| format!("search:{query}"))
            }),
        "sentry" => args
            .repo
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| match (args.org.as_deref(), args.project.as_deref()) {
                (Some(org), Some(project)) => Some(format!("{org}/{project}")),
                _ => args.id.clone(),
            }),
        "slack" => {
            let channel = args
                .channel
                .as_deref()
                .or(args.repo.as_deref())
                .map(str::trim)
                .filter(|value| !value.is_empty())?;
            let thread_ts = args
                .thread_ts
                .as_deref()
                .or(args.id.as_deref())
                .map(str::trim)
                .filter(|value| !value.is_empty());
            Some(match (operation, thread_ts) {
                ("post_message", Some(thread_ts)) | ("read_thread", Some(thread_ts)) => {
                    format!("{channel} thread {thread_ts}")
                }
                _ => channel.to_string(),
            })
        }
        _ => None,
    }
}

fn record_connector_tool_audit(
    audit: ConnectorToolAuditContext,
    result: &Result<JsonValue, ToolError>,
) {
    let (outcome, error_code, message) = match result {
        Ok(_) => (connectors::ConnectorAuditOutcome::Ok, None, None),
        Err(ToolError::PermissionDenied { action })
            if audit.confirmation_required && !audit.confirmed =>
        {
            (
                connectors::ConnectorAuditOutcome::Blocked,
                Some("confirmation_required".to_string()),
                Some(action.clone()),
            )
        }
        Err(err) => (
            connectors::ConnectorAuditOutcome::Error,
            Some(tool_error_code(err).to_string()),
            Some(err.to_string()),
        ),
    };
    if let Err(err) =
        connectors::append_connector_audit_event(connectors::ConnectorAuditRecordRequest {
            connector_id: audit.connector_id,
            operation: audit.operation,
            access: audit.access,
            confirmation_required: audit.confirmation_required,
            confirmed: audit.confirmed,
            target: audit.target,
            session_id: audit.session_id,
            project_root: audit.project_root,
            outcome,
            error_code,
            message,
        })
    {
        tracing::warn!(
            target: "omiga::connectors",
            error = %err,
            "failed to append connector tool audit event"
        );
    }
}

fn tool_error_code(err: &ToolError) -> &'static str {
    match err {
        ToolError::UnknownTool { .. } => "unknown_tool",
        ToolError::InvalidArguments { .. } => "invalid_arguments",
        ToolError::ExecutionFailed { .. } => "execution_failed",
        ToolError::Cancelled => "cancelled",
        ToolError::Timeout { .. } => "timeout",
        ToolError::PermissionDenied { .. } => "permission_denied",
    }
}

fn ensure_connector_accessible(connector_id: &str) -> Result<(), ToolError> {
    let catalog = connectors::list_connector_catalog();
    let Some(connector) = catalog
        .connectors
        .iter()
        .find(|item| item.definition.id == connector_id)
    else {
        return Err(ToolError::InvalidArguments {
            message: format!("Connector `{connector_id}` is not known."),
        });
    };

    if !connector.enabled || connector.status == ConnectorConnectionStatus::Disabled {
        return Err(ToolError::PermissionDenied {
            action: format!(
                "Connector `{}` is disabled in Settings → Connectors.",
                connector.definition.name
            ),
        });
    }

    if !connector.accessible {
        let hint = if connector.definition.env_vars.is_empty() {
            "Complete the connector's browser/software login flow in Settings → Connectors."
                .to_string()
        } else {
            format!(
                "Complete the connector's browser/software login flow, or configure an advanced credential provider such as {} outside Omiga config.",
                connector.definition.env_vars.join(", ")
            )
        };
        return Err(ToolError::PermissionDenied {
            action: format!(
                "Connector `{}` is not connected. {hint}",
                connector.definition.name
            ),
        });
    }

    Ok(())
}

async fn execute_github_json(args: ConnectorArgs) -> Result<JsonValue, ToolError> {
    ensure_connector_accessible("github")?;
    let operation = normalize_id(&args.operation);
    let (owner, repo) = parse_github_repo(
        args.repo
            .as_deref()
            .ok_or_else(|| invalid_args("GitHub connector requires `repo` as `owner/name`."))?,
    )?;
    let state = normalize_state(args.state.as_deref())?;
    let limit = args.max_results.unwrap_or(10).clamp(1, 25);
    let base_url = github_api_base_url();
    let token = github_token();

    match operation.as_str() {
        "list_issues" | "issues" => {
            let url = format!(
                "{base_url}/repos/{owner}/{repo}/issues?state={state}&per_page={limit}"
            );
            let value = github_get_json(&url, token.as_deref()).await?;
            let results = summarize_github_issue_list(&value);
            let result_count = results.len();
            Ok(json!({
                "connector": "github",
                "operation": "list_issues",
                "repo": format!("{owner}/{repo}"),
                "results": results,
                "result_count": result_count,
                "raw_count": value.as_array().map(|items| items.len()).unwrap_or(0),
            }))
        }
        "read_issue" | "get_issue" | "issue" => {
            let number = args
                .number
                .ok_or_else(|| invalid_args("GitHub read_issue requires `number`."))?;
            let url = format!("{base_url}/repos/{owner}/{repo}/issues/{number}");
            let value = github_get_json(&url, token.as_deref()).await?;
            Ok(json!({
                "connector": "github",
                "operation": "read_issue",
                "repo": format!("{owner}/{repo}"),
                "issue": summarize_github_issue(&value),
            }))
        }
        "list_pull_requests" | "list_pulls" | "pull_requests" | "prs" => {
            let url = format!("{base_url}/repos/{owner}/{repo}/pulls?state={state}&per_page={limit}");
            let value = github_get_json(&url, token.as_deref()).await?;
            let results = summarize_github_pull_list(&value);
            let result_count = results.len();
            Ok(json!({
                "connector": "github",
                "operation": "list_pull_requests",
                "repo": format!("{owner}/{repo}"),
                "results": results,
                "result_count": result_count,
                "raw_count": value.as_array().map(|items| items.len()).unwrap_or(0),
            }))
        }
        "read_pull_request" | "get_pull_request" | "read_pr" | "get_pr" | "pr" => {
            let number = args
                .number
                .ok_or_else(|| invalid_args("GitHub read_pull_request requires `number`."))?;
            let url = format!("{base_url}/repos/{owner}/{repo}/pulls/{number}");
            let value = github_get_json(&url, token.as_deref()).await?;
            Ok(json!({
                "connector": "github",
                "operation": "read_pull_request",
                "repo": format!("{owner}/{repo}"),
                "pull_request": summarize_github_pull(&value),
            }))
        }
        other => Err(ToolError::InvalidArguments {
            message: format!(
                "Unsupported GitHub connector operation `{other}`. Use list_issues, read_issue, list_pull_requests, or read_pull_request."
            ),
        }),
    }
}

async fn execute_gitlab_json(args: ConnectorArgs) -> Result<JsonValue, ToolError> {
    ensure_connector_accessible("gitlab")?;
    let operation = normalize_id(&args.operation);
    let project =
        parse_gitlab_project_path(args.repo.as_deref().ok_or_else(|| {
            invalid_args("GitLab connector requires `repo` as `group/project`.")
        })?)?;
    let encoded_project = percent_encode_gitlab_project_path(&project);
    let state = normalize_gitlab_state(args.state.as_deref(), operation.as_str())?;
    let limit = args.max_results.unwrap_or(10).clamp(1, 25);
    let base_url = gitlab_api_base_url();
    let token = gitlab_token();

    match operation.as_str() {
        "list_issues" | "issues" => {
            let url = format!(
                "{base_url}/projects/{encoded_project}/issues?state={state}&per_page={limit}"
            );
            let value = gitlab_get_json(&url, token.as_deref()).await?;
            let results = summarize_gitlab_issue_list(&value);
            let result_count = results.len();
            Ok(json!({
                "connector": "gitlab",
                "operation": "list_issues",
                "repo": project,
                "results": results,
                "result_count": result_count,
                "raw_count": value.as_array().map(|items| items.len()).unwrap_or(0),
            }))
        }
        "read_issue" | "get_issue" | "issue" => {
            let number = args
                .number
                .ok_or_else(|| invalid_args("GitLab read_issue requires `number` (issue IID)."))?;
            let url = format!("{base_url}/projects/{encoded_project}/issues/{number}");
            let value = gitlab_get_json(&url, token.as_deref()).await?;
            Ok(json!({
                "connector": "gitlab",
                "operation": "read_issue",
                "repo": project,
                "issue": summarize_gitlab_issue(&value),
            }))
        }
        "list_merge_requests" | "list_mrs" | "merge_requests" | "mrs" | "list_pull_requests"
        | "pull_requests" | "prs" => {
            let url = format!(
                "{base_url}/projects/{encoded_project}/merge_requests?state={state}&per_page={limit}"
            );
            let value = gitlab_get_json(&url, token.as_deref()).await?;
            let results = summarize_gitlab_merge_request_list(&value);
            let result_count = results.len();
            Ok(json!({
                "connector": "gitlab",
                "operation": "list_merge_requests",
                "repo": project,
                "results": results,
                "result_count": result_count,
                "raw_count": value.as_array().map(|items| items.len()).unwrap_or(0),
            }))
        }
        "read_merge_request" | "get_merge_request" | "read_mr" | "get_mr" | "mr"
        | "read_pull_request" | "get_pull_request" | "read_pr" | "get_pr" | "pr" => {
            let number = args.number.ok_or_else(|| {
                invalid_args("GitLab read_merge_request requires `number` (merge request IID).")
            })?;
            let url = format!("{base_url}/projects/{encoded_project}/merge_requests/{number}");
            let value = gitlab_get_json(&url, token.as_deref()).await?;
            Ok(json!({
                "connector": "gitlab",
                "operation": "read_merge_request",
                "repo": project,
                "merge_request": summarize_gitlab_merge_request(&value),
            }))
        }
        other => Err(ToolError::InvalidArguments {
            message: format!(
                "Unsupported GitLab connector operation `{other}`. Use list_issues, read_issue, list_merge_requests, or read_merge_request."
            ),
        }),
    }
}

async fn execute_linear_json(args: ConnectorArgs) -> Result<JsonValue, ToolError> {
    ensure_connector_accessible("linear")?;
    let operation = normalize_id(&args.operation);
    let limit = args.max_results.unwrap_or(10).clamp(1, 25);

    match operation.as_str() {
        "list_issues" | "issues" => {
            let value = linear_graphql_json(
                r#"
                query OmigaLinearIssues($first: Int!) {
                  issues(first: $first, orderBy: updatedAt) {
                    nodes {
                      id
                      identifier
                      title
                      description
                      url
                      priority
                      estimate
                      createdAt
                      updatedAt
                      archivedAt
                      state { name type }
                      assignee { name email }
                      team { key name }
                      project { name url }
                    }
                  }
                }
                "#,
                json!({ "first": limit }),
            )
            .await?;
            let nodes = value
                .get("data")
                .and_then(|data| data.get("issues"))
                .and_then(|issues| issues.get("nodes"))
                .cloned()
                .unwrap_or_else(|| json!([]));
            let results = summarize_linear_issue_list(&nodes);
            Ok(json!({
                "connector": "linear",
                "operation": "list_issues",
                "results": results,
                "result_count": results.len(),
            }))
        }
        "read_issue" | "get_issue" | "issue" => {
            let issue_id = connector_string_id(&args, "Linear read_issue requires `id`.")?;
            let value = linear_graphql_json(
                r#"
                query OmigaLinearIssue($id: String!) {
                  issue(id: $id) {
                    id
                    identifier
                    title
                    description
                    url
                    priority
                    estimate
                    createdAt
                    updatedAt
                    archivedAt
                    state { name type }
                    assignee { name email }
                    team { key name }
                    project { name url }
                  }
                }
                "#,
                json!({ "id": issue_id }),
            )
            .await?;
            Ok(json!({
                "connector": "linear",
                "operation": "read_issue",
                "issue": summarize_linear_issue(value.get("data").and_then(|data| data.get("issue")).unwrap_or(&JsonValue::Null)),
            }))
        }
        other => Err(ToolError::InvalidArguments {
            message: format!(
                "Unsupported Linear connector operation `{other}`. Use list_issues or read_issue."
            ),
        }),
    }
}

async fn execute_notion_json(args: ConnectorArgs) -> Result<JsonValue, ToolError> {
    ensure_connector_accessible("notion")?;
    let operation = normalize_id(&args.operation);

    match operation.as_str() {
        "search_pages" | "search" | "list_pages" | "pages" => {
            let limit = args.max_results.unwrap_or(10).clamp(1, 25);
            let query = args
                .query
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty());
            let mut body = json!({
                "page_size": limit,
                "filter": { "property": "object", "value": "page" },
                "sort": { "direction": "descending", "timestamp": "last_edited_time" }
            });
            if let Some(query) = query {
                body["query"] = json!(query);
            }
            let value =
                notion_post_json(&format!("{}/search", notion_api_base_url()), body).await?;
            let results =
                summarize_notion_page_list(value.get("results").unwrap_or(&JsonValue::Null));
            Ok(json!({
                "connector": "notion",
                "operation": "search_pages",
                "query": query,
                "results": results,
                "result_count": results.len(),
            }))
        }
        "read_page" | "get_page" | "page" => {
            let block_budget = args.max_results.unwrap_or(50).clamp(1, 200) as usize;
            let max_depth = args.max_depth.unwrap_or(1).min(3);
            let page_id =
                connector_string_id(&args, "Notion read_page requires `id` or `page_id`.")?;
            let url = format!("{}/pages/{page_id}", notion_api_base_url());
            let page = notion_get_json(&url).await?;
            let read_result = read_notion_blocks_bounded(&page_id, block_budget, max_depth).await?;
            let block_summaries = read_result.blocks;
            let content_markdown = render_notion_blocks_markdown(&block_summaries);
            Ok(json!({
                "connector": "notion",
                "operation": "read_page",
                "page": summarize_notion_page(&page),
                "blocks": block_summaries,
                "content_markdown": content_markdown,
                "block_count": read_result.block_count,
                "has_more_blocks": read_result.has_more,
                "truncated": read_result.truncated,
                "max_depth": max_depth,
            }))
        }
        other => Err(ToolError::InvalidArguments {
            message: format!(
                "Unsupported Notion connector operation `{other}`. Use search_pages or read_page."
            ),
        }),
    }
}

async fn execute_sentry_json(args: ConnectorArgs) -> Result<JsonValue, ToolError> {
    ensure_connector_accessible("sentry")?;
    let operation = normalize_id(&args.operation);
    let limit = args.max_results.unwrap_or(10).clamp(1, 25) as usize;

    match operation.as_str() {
        "list_issues" | "issues" => {
            let (org, project) = sentry_org_project(&args)?;
            let query = sentry_query(args.query.as_deref(), args.state.as_deref())?;
            let url = format!("{}/projects/{org}/{project}/issues/", sentry_api_base_url());
            let value = sentry_get_json_with_query(&url, query.as_deref()).await?;
            let mut results = summarize_sentry_issue_list(&value);
            results.truncate(limit);
            Ok(json!({
                "connector": "sentry",
                "operation": "list_issues",
                "repo": format!("{org}/{project}"),
                "query": query,
                "results": results,
                "result_count": results.len(),
                "raw_count": value.as_array().map(|items| items.len()).unwrap_or(0),
            }))
        }
        "read_issue" | "get_issue" | "issue" => {
            let org = sentry_org(&args)?;
            let issue_id =
                connector_string_id(&args, "Sentry read_issue requires `id` or `issue_id`.")?;
            let url = format!("{}/issues/{issue_id}/", sentry_api_base_url());
            let value = sentry_get_json_with_org(&url, &org).await?;
            Ok(json!({
                "connector": "sentry",
                "operation": "read_issue",
                "organization": org,
                "issue": summarize_sentry_issue(&value),
            }))
        }
        other => Err(ToolError::InvalidArguments {
            message: format!(
                "Unsupported Sentry connector operation `{other}`. Use list_issues or read_issue."
            ),
        }),
    }
}

async fn execute_slack_json(args: ConnectorArgs) -> Result<JsonValue, ToolError> {
    ensure_connector_accessible("slack")?;
    let operation = normalize_id(&args.operation);

    match operation.as_str() {
        "read_thread" | "thread" | "replies" | "conversation_replies" => {
            let channel = slack_channel(&args)?;
            let thread_ts = slack_thread_ts(&args)?;
            let limit = args.max_results.unwrap_or(20).clamp(1, 100);
            let limit_string = limit.to_string();
            let value = slack_get_json(
                "conversations.replies",
                &[
                    ("channel", channel.as_str()),
                    ("ts", thread_ts.as_str()),
                    ("limit", limit_string.as_str()),
                ],
            )
            .await?;
            let messages =
                summarize_slack_message_list(value.get("messages").unwrap_or(&JsonValue::Null));
            Ok(json!({
                "connector": "slack",
                "operation": "read_thread",
                "channel": channel,
                "thread_ts": thread_ts,
                "messages": messages,
                "result_count": messages.len(),
                "has_more": value.get("has_more").and_then(JsonValue::as_bool).unwrap_or(false),
            }))
        }
        "post_message" | "send_message" | "reply" => {
            if !args.confirm_write {
                return Err(ToolError::PermissionDenied {
                    action: "Slack post_message writes to an external workspace. Re-run only after explicit user intent with confirm_write=true.".to_string(),
                });
            }
            let channel = slack_channel(&args)?;
            let text = slack_message_text(&args)?;
            let mut body = json!({
                "channel": channel,
                "text": text,
            });
            if let Some(thread_ts) = args
                .thread_ts
                .as_deref()
                .or(args.id.as_deref())
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                validate_slack_ts(thread_ts)?;
                body["thread_ts"] = json!(thread_ts);
            }
            let value = slack_post_json("chat.postMessage", body).await?;
            Ok(json!({
                "connector": "slack",
                "operation": "post_message",
                "channel": value.get("channel").and_then(JsonValue::as_str),
                "ts": value.get("ts").and_then(JsonValue::as_str),
                "message": summarize_slack_message(value.get("message").unwrap_or(&JsonValue::Null)),
                "ok": true,
            }))
        }
        other => Err(ToolError::InvalidArguments {
            message: format!(
                "Unsupported Slack connector operation `{other}`. Use read_thread or post_message."
            ),
        }),
    }
}

fn invalid_args(message: impl Into<String>) -> ToolError {
    ToolError::InvalidArguments {
        message: message.into(),
    }
}

fn normalize_state(value: Option<&str>) -> Result<String, ToolError> {
    let state = value.unwrap_or("open").trim().to_ascii_lowercase();
    match state.as_str() {
        "open" | "closed" | "all" => Ok(state),
        other => Err(invalid_args(format!(
            "Unsupported GitHub state `{other}`. Use open, closed, or all."
        ))),
    }
}

fn github_api_base_url() -> String {
    std::env::var("OMIGA_GITHUB_API_BASE_URL")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "https://api.github.com".to_string())
}

fn github_token() -> Option<String> {
    connectors::github_token()
}

fn gitlab_api_base_url() -> String {
    std::env::var("OMIGA_GITLAB_API_BASE_URL")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "https://gitlab.com/api/v4".to_string())
}

fn gitlab_token() -> Option<String> {
    std::env::var("GITLAB_TOKEN")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn linear_graphql_url() -> String {
    std::env::var("OMIGA_LINEAR_GRAPHQL_URL")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "https://api.linear.app/graphql".to_string())
}

fn linear_authorization_header() -> Option<String> {
    std::env::var("LINEAR_ACCESS_TOKEN")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(|token| format!("Bearer {token}"))
        .or_else(|| {
            std::env::var("LINEAR_API_KEY")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        })
}

fn notion_api_base_url() -> String {
    std::env::var("OMIGA_NOTION_API_BASE_URL")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "https://api.notion.com/v1".to_string())
}

fn notion_version() -> String {
    std::env::var("OMIGA_NOTION_VERSION")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "2022-06-28".to_string())
}

fn notion_token() -> Option<String> {
    connectors::oauth::notion_oauth_token().or_else(|| {
        ["NOTION_TOKEN", "NOTION_API_KEY"].iter().find_map(|name| {
            std::env::var(name)
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        })
    })
}

fn sentry_api_base_url() -> String {
    std::env::var("OMIGA_SENTRY_API_BASE_URL")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "https://sentry.io/api/0".to_string())
}

fn sentry_token() -> Option<String> {
    std::env::var("SENTRY_AUTH_TOKEN")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn slack_api_base_url() -> String {
    std::env::var("OMIGA_SLACK_API_BASE_URL")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "https://slack.com/api".to_string())
}

fn slack_token() -> Option<String> {
    connectors::oauth::slack_oauth_token().or_else(|| {
        std::env::var("SLACK_BOT_TOKEN")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

fn required_secret(value: Option<String>, message: &str) -> Result<String, ToolError> {
    value.ok_or_else(|| ToolError::PermissionDenied {
        action: message.to_string(),
    })
}

fn connector_string_id(args: &ConnectorArgs, message: &str) -> Result<String, ToolError> {
    args.id
        .as_deref()
        .or(args.repo.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| args.number.map(|number| number.to_string()))
        .ok_or_else(|| invalid_args(message))
}

fn slack_channel(args: &ConnectorArgs) -> Result<String, ToolError> {
    let channel = args
        .channel
        .as_deref()
        .or(args.repo.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            invalid_args("Slack connector requires `channel` as a channel/conversation ID.")
        })?;
    let channel = channel.strip_prefix('#').unwrap_or(channel);
    if !is_safe_slack_channel(channel) {
        return Err(invalid_args(
            "Slack channel must be a channel/conversation ID containing only letters, numbers, '_' or '-'.",
        ));
    }
    Ok(channel.to_string())
}

fn slack_thread_ts(args: &ConnectorArgs) -> Result<String, ToolError> {
    let thread_ts = args
        .thread_ts
        .as_deref()
        .or(args.id.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| invalid_args("Slack read_thread requires `thread_ts` or `id`."))?;
    validate_slack_ts(thread_ts)?;
    Ok(thread_ts.to_string())
}

fn validate_slack_ts(value: &str) -> Result<(), ToolError> {
    let dot_count = value.chars().filter(|ch| *ch == '.').count();
    let valid = !value.is_empty()
        && value.chars().all(|ch| ch.is_ascii_digit() || ch == '.')
        && dot_count <= 1
        && (dot_count == 0 || value.split('.').all(|part| !part.is_empty()));
    if !valid {
        return Err(invalid_args(
            "Slack timestamp must contain only digits and an optional decimal point, e.g. 1712345678.123456.",
        ));
    }
    Ok(())
}

fn slack_message_text(args: &ConnectorArgs) -> Result<String, ToolError> {
    let text = args
        .text
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| invalid_args("Slack post_message requires non-empty `text`."))?;
    if text.chars().count() > 4_000 {
        return Err(invalid_args(
            "Slack post_message text is capped at 4000 characters by Omiga to avoid accidental long posts.",
        ));
    }
    Ok(text.to_string())
}

pub(crate) fn parse_github_repo(value: &str) -> Result<(String, String), ToolError> {
    let mut raw = value.trim().trim_end_matches('/').to_string();
    for prefix in ["https://github.com/", "http://github.com/", "github.com/"] {
        if let Some(rest) = raw.strip_prefix(prefix) {
            raw = rest.to_string();
            break;
        }
    }
    if let Some(stripped) = raw.strip_suffix(".git") {
        raw = stripped.to_string();
    }
    let mut parts = raw.split('/').filter(|part| !part.trim().is_empty());
    let owner = parts
        .next()
        .ok_or_else(|| invalid_args("GitHub repo must be `owner/name`."))?
        .trim();
    let repo = parts
        .next()
        .ok_or_else(|| invalid_args("GitHub repo must include a repository name."))?
        .trim();
    if owner.is_empty() || repo.is_empty() || parts.next().is_some() {
        return Err(invalid_args("GitHub repo must be exactly `owner/name`."));
    }
    if !is_safe_github_segment(owner) || !is_safe_github_segment(repo) {
        return Err(invalid_args(
            "GitHub owner/repo may contain only letters, numbers, '.', '_' or '-'.",
        ));
    }
    Ok((owner.to_string(), repo.to_string()))
}

pub(crate) fn parse_gitlab_project_path(value: &str) -> Result<String, ToolError> {
    let mut raw = value.trim().trim_end_matches('/').to_string();
    for prefix in ["https://gitlab.com/", "http://gitlab.com/", "gitlab.com/"] {
        if let Some(rest) = raw.strip_prefix(prefix) {
            raw = rest.to_string();
            break;
        }
    }
    if let Some(stripped) = raw.strip_suffix(".git") {
        raw = stripped.to_string();
    }
    let parts = raw
        .split('/')
        .filter(|part| !part.trim().is_empty())
        .map(str::trim)
        .collect::<Vec<_>>();
    if parts.len() < 2 {
        return Err(invalid_args("GitLab project must be `group/project`."));
    }
    if parts.iter().any(|part| *part == "-" || part.contains(".."))
        || !parts
            .iter()
            .all(|part| is_safe_gitlab_project_segment(part))
    {
        return Err(invalid_args(
            "GitLab project path may contain only letters, numbers, '.', '_' or '-' segments.",
        ));
    }
    Ok(parts.join("/"))
}

pub(crate) fn parse_sentry_org_project(value: &str) -> Result<(String, String), ToolError> {
    let mut raw = value.trim().trim_end_matches('/').to_string();
    for prefix in ["https://sentry.io/", "http://sentry.io/", "sentry.io/"] {
        if let Some(rest) = raw.strip_prefix(prefix) {
            raw = rest.to_string();
            break;
        }
    }
    let parts = raw
        .split('/')
        .filter(|part| !part.trim().is_empty())
        .map(str::trim)
        .collect::<Vec<_>>();
    let (org, project) = if parts.first() == Some(&"organizations") && parts.len() >= 3 {
        (parts[1], parts[2])
    } else if parts.len() >= 2 {
        (parts[0], parts[1])
    } else {
        return Err(invalid_args("Sentry repo must be `org/project`."));
    };
    if !is_safe_service_slug(org) || !is_safe_service_slug(project) {
        return Err(invalid_args(
            "Sentry organization/project slugs may contain only letters, numbers, '.', '_' or '-'.",
        ));
    }
    Ok((org.to_string(), project.to_string()))
}

fn sentry_org_project(args: &ConnectorArgs) -> Result<(String, String), ToolError> {
    if let (Some(org), Some(project)) = (args.org.as_deref(), args.project.as_deref()) {
        let org = org.trim();
        let project = project.trim();
        if is_safe_service_slug(org) && is_safe_service_slug(project) {
            return Ok((org.to_string(), project.to_string()));
        }
        return Err(invalid_args(
            "Sentry org/project slugs may contain only letters, numbers, '.', '_' or '-'.",
        ));
    }
    parse_sentry_org_project(
        args.repo
            .as_deref()
            .ok_or_else(|| invalid_args("Sentry connector requires `repo` as `org/project`."))?,
    )
}

fn sentry_org(args: &ConnectorArgs) -> Result<String, ToolError> {
    if let Some(org) = args
        .org
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if is_safe_service_slug(org) {
            return Ok(org.to_string());
        }
        return Err(invalid_args(
            "Sentry organization slug may contain only letters, numbers, '.', '_' or '-'.",
        ));
    }
    args.repo
        .as_deref()
        .map(parse_sentry_org_project)
        .transpose()
        .map(|project| project.map(|(org, _)| org))
        .and_then(|org| {
            org.ok_or_else(|| invalid_args("Sentry read_issue requires `org` or `repo`."))
        })
}

fn sentry_query(query: Option<&str>, state: Option<&str>) -> Result<Option<String>, ToolError> {
    let query = query
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    if query.is_some() {
        return Ok(query);
    }
    match state.unwrap_or("open").trim().to_ascii_lowercase().as_str() {
        "open" | "unresolved" => Ok(Some("is:unresolved".to_string())),
        "resolved" | "closed" => Ok(Some("is:resolved".to_string())),
        "all" => Ok(None),
        other => Err(invalid_args(format!(
            "Unsupported Sentry state `{other}`. Use open, resolved, or all."
        ))),
    }
}

fn is_safe_github_segment(value: &str) -> bool {
    !value.is_empty()
        && !value.contains("..")
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.')
}

fn is_safe_gitlab_project_segment(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.')
}

fn is_safe_service_slug(value: &str) -> bool {
    !value.is_empty()
        && !value.contains("..")
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.')
}

fn is_safe_slack_channel(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
}

fn percent_encode_gitlab_project_path(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}

fn percent_encode_url_component(value: &str) -> String {
    percent_encode_gitlab_project_path(value)
}

async fn github_get_json(url: &str, token: Option<&str>) -> Result<JsonValue, ToolError> {
    let mut request = ConnectorHttpRequest::new("GitHub", Method::GET, url)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28");
    if let Some(token) = token {
        request = request.bearer_token(token);
    }
    send_connector_json(request)
        .await
        .map_err(connector_http_tool_error)
}

async fn gitlab_get_json(url: &str, token: Option<&str>) -> Result<JsonValue, ToolError> {
    let mut request = ConnectorHttpRequest::new("GitLab", Method::GET, url);
    if let Some(token) = token {
        request = request.header("PRIVATE-TOKEN", token);
    }
    send_connector_json(request)
        .await
        .map_err(connector_http_tool_error)
}

async fn linear_graphql_json(query: &str, variables: JsonValue) -> Result<JsonValue, ToolError> {
    let authorization = required_secret(
        linear_authorization_header(),
        "Linear native connector requires LINEAR_API_KEY or LINEAR_ACCESS_TOKEN.",
    )?;
    let value = send_connector_json(
        ConnectorHttpRequest::new("Linear", Method::POST, linear_graphql_url())
            .header("Authorization", authorization)
            .json_body(json!({
                "query": query,
                "variables": variables,
            })),
    )
    .await
    .map_err(connector_http_tool_error)?;
    if let Some(errors) = value.get("errors").and_then(JsonValue::as_array) {
        if !errors.is_empty() {
            return Err(ToolError::ExecutionFailed {
                message: format!(
                    "Linear GraphQL returned errors: {}",
                    truncate_for_output(
                        &errors
                            .iter()
                            .map(JsonValue::to_string)
                            .collect::<Vec<_>>()
                            .join("; "),
                        600
                    )
                ),
            });
        }
    }
    Ok(value)
}

async fn notion_get_json(url: &str) -> Result<JsonValue, ToolError> {
    notion_request(Method::GET, url, None).await
}

async fn notion_post_json(url: &str, body: JsonValue) -> Result<JsonValue, ToolError> {
    notion_request(Method::POST, url, Some(body)).await
}

async fn notion_request(
    method: Method,
    url: &str,
    body: Option<JsonValue>,
) -> Result<JsonValue, ToolError> {
    let token = required_secret(
        notion_token(),
        "Notion native connector requires browser OAuth login or NOTION_TOKEN/NOTION_API_KEY advanced credentials.",
    )?;
    let mut request = ConnectorHttpRequest::new("Notion", method, url)
        .header("Notion-Version", notion_version())
        .bearer_token(token);
    if let Some(body) = body {
        request = request.json_body(body);
    }
    send_connector_json(request)
        .await
        .map_err(connector_http_tool_error)
}

#[derive(Debug, Default)]
struct NotionBlockReadResult {
    blocks: Vec<JsonValue>,
    block_count: usize,
    has_more: bool,
    truncated: bool,
}

#[derive(Debug, Default)]
struct NotionChildReadOutcome {
    direct_truncated: bool,
}

async fn read_notion_blocks_bounded(
    root_block_id: &str,
    block_budget: usize,
    max_depth: u32,
) -> Result<NotionBlockReadResult, ToolError> {
    let mut result = NotionBlockReadResult::default();
    append_notion_block_children(root_block_id, 0, block_budget, max_depth, &mut result).await?;
    Ok(result)
}

#[async_recursion::async_recursion]
async fn append_notion_block_children(
    block_id: &str,
    depth: u32,
    block_budget: usize,
    max_depth: u32,
    result: &mut NotionBlockReadResult,
) -> Result<NotionChildReadOutcome, ToolError> {
    if result.block_count >= block_budget {
        result.truncated = true;
        return Ok(NotionChildReadOutcome {
            direct_truncated: true,
        });
    }

    let remaining = block_budget.saturating_sub(result.block_count);
    let page_size = remaining.clamp(1, 100);
    let children = notion_read_direct_block_children(block_id, page_size).await?;
    result.has_more |= children.has_more;
    let mut direct_truncated = children.has_more;

    for block in children.blocks {
        if result.block_count >= block_budget {
            result.truncated = true;
            direct_truncated = true;
            break;
        }

        let has_children = block
            .get("has_children")
            .and_then(JsonValue::as_bool)
            .unwrap_or(false);
        let child_id = block
            .get("id")
            .and_then(JsonValue::as_str)
            .unwrap_or("")
            .to_string();
        let mut summary = summarize_notion_block(&block);
        summary["depth"] = json!(depth);
        summary["children_loaded"] = json!(false);
        summary["children_truncated"] = json!(false);

        result.block_count += 1;
        result.blocks.push(summary);
        let index = result.blocks.len() - 1;

        if !has_children {
            continue;
        }

        if depth < max_depth && result.block_count < block_budget && !child_id.is_empty() {
            result.blocks[index]["children_loaded"] = json!(true);
            let child_outcome =
                append_notion_block_children(&child_id, depth + 1, block_budget, max_depth, result)
                    .await?;
            if child_outcome.direct_truncated {
                result.blocks[index]["children_truncated"] = json!(true);
            }
        } else {
            result.blocks[index]["children_truncated"] = json!(true);
            result.truncated = true;
        }
    }

    if direct_truncated {
        result.truncated = true;
    }

    Ok(NotionChildReadOutcome { direct_truncated })
}

#[derive(Debug, Default)]
struct DirectNotionBlockChildren {
    blocks: Vec<JsonValue>,
    has_more: bool,
}

async fn notion_read_direct_block_children(
    block_id: &str,
    page_size: usize,
) -> Result<DirectNotionBlockChildren, ToolError> {
    let mut blocks = Vec::new();
    let page_size = page_size.clamp(1, 100);
    let mut cursor: Option<String> = None;

    let final_has_more = loop {
        let mut url = format!(
            "{}/blocks/{block_id}/children?page_size={page_size}",
            notion_api_base_url()
        );
        if let Some(cursor) = cursor.as_deref() {
            url.push_str("&start_cursor=");
            url.push_str(&percent_encode_url_component(cursor));
        }
        let value = notion_get_json(&url).await?;
        if let Some(items) = value.get("results").and_then(JsonValue::as_array) {
            blocks.extend(items.iter().cloned());
        }
        let has_more = value
            .get("has_more")
            .and_then(JsonValue::as_bool)
            .unwrap_or(false);
        if !has_more || blocks.len() >= page_size {
            break has_more;
        }
        cursor = value
            .get("next_cursor")
            .and_then(JsonValue::as_str)
            .map(str::to_string);
        if cursor.is_none() {
            break has_more;
        }
    };

    Ok(DirectNotionBlockChildren {
        blocks,
        has_more: final_has_more,
    })
}

async fn sentry_get_json_with_query(
    url: &str,
    query: Option<&str>,
) -> Result<JsonValue, ToolError> {
    sentry_get_json(url, &[], query).await
}

async fn sentry_get_json_with_org(url: &str, org: &str) -> Result<JsonValue, ToolError> {
    sentry_get_json(url, &[("organizationSlug", org)], None).await
}

async fn sentry_get_json(
    url: &str,
    extra_query: &[(&str, &str)],
    query: Option<&str>,
) -> Result<JsonValue, ToolError> {
    let token = required_secret(
        sentry_token(),
        "Sentry native connector requires SENTRY_AUTH_TOKEN.",
    )?;
    let mut request = ConnectorHttpRequest::new("Sentry", Method::GET, url).bearer_token(token);
    if let Some(query) = query {
        request = request.query("query", query);
    }
    for (name, value) in extra_query {
        request = request.query(*name, *value);
    }
    send_connector_json(request)
        .await
        .map_err(connector_http_tool_error)
}

async fn slack_get_json(method_name: &str, query: &[(&str, &str)]) -> Result<JsonValue, ToolError> {
    slack_request(Method::GET, method_name, query, None).await
}

async fn slack_post_json(method_name: &str, body: JsonValue) -> Result<JsonValue, ToolError> {
    slack_request(Method::POST, method_name, &[], Some(body)).await
}

async fn slack_request(
    method: Method,
    method_name: &str,
    query: &[(&str, &str)],
    body: Option<JsonValue>,
) -> Result<JsonValue, ToolError> {
    let token = required_secret(
        slack_token(),
        "Slack native connector requires browser OAuth login or SLACK_BOT_TOKEN advanced credentials.",
    )?;
    let mut request = ConnectorHttpRequest::new(
        "Slack",
        method,
        format!("{}/{}", slack_api_base_url(), method_name),
    )
    .bearer_token(token);
    for (name, value) in query {
        request = request.query(*name, *value);
    }
    if let Some(body) = body {
        request = request.json_body(body);
    }
    let value = send_connector_json(request)
        .await
        .map_err(connector_http_tool_error)?;
    if value.get("ok").and_then(JsonValue::as_bool) == Some(false) {
        let error = value
            .get("error")
            .and_then(JsonValue::as_str)
            .unwrap_or("slack_api_error");
        return Err(ToolError::ExecutionFailed {
            message: format!("Slack {method_name} returned ok=false: {error}"),
        });
    }
    Ok(value)
}

fn connector_http_tool_error(err: ConnectorHttpError) -> ToolError {
    let retry_hint = if err.retryable {
        " Retrying later may succeed."
    } else {
        ""
    };
    ToolError::ExecutionFailed {
        message: format!("{}{retry_hint}", err.user_message()),
    }
}

fn normalize_gitlab_state(value: Option<&str>, operation: &str) -> Result<String, ToolError> {
    let state = value.unwrap_or("open").trim().to_ascii_lowercase();
    match state.as_str() {
        "open" | "opened" => Ok("opened".to_string()),
        "closed" => Ok("closed".to_string()),
        "all" => Ok("all".to_string()),
        "merged" if operation.contains("merge") || operation.contains("mr") || operation.contains("pull") => {
            Ok("merged".to_string())
        }
        other => Err(invalid_args(format!(
            "Unsupported GitLab state `{other}`. Use open, closed, all, or merged for merge requests."
        ))),
    }
}

fn summarize_github_issue_list(value: &JsonValue) -> Vec<JsonValue> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter(|item| item.get("pull_request").is_none())
                .map(summarize_github_issue)
                .collect()
        })
        .unwrap_or_default()
}

fn summarize_github_pull_list(value: &JsonValue) -> Vec<JsonValue> {
    value
        .as_array()
        .map(|items| items.iter().map(summarize_github_pull).collect())
        .unwrap_or_default()
}

fn summarize_gitlab_issue_list(value: &JsonValue) -> Vec<JsonValue> {
    value
        .as_array()
        .map(|items| items.iter().map(summarize_gitlab_issue).collect())
        .unwrap_or_default()
}

fn summarize_gitlab_merge_request_list(value: &JsonValue) -> Vec<JsonValue> {
    value
        .as_array()
        .map(|items| items.iter().map(summarize_gitlab_merge_request).collect())
        .unwrap_or_default()
}

fn summarize_github_issue(value: &JsonValue) -> JsonValue {
    json!({
        "number": value.get("number").and_then(JsonValue::as_u64),
        "title": value.get("title").and_then(JsonValue::as_str),
        "state": value.get("state").and_then(JsonValue::as_str),
        "url": value.get("html_url").and_then(JsonValue::as_str),
        "author": value.get("user").and_then(|user| user.get("login")).and_then(JsonValue::as_str),
        "labels": value.get("labels").and_then(JsonValue::as_array).map(|labels| {
            labels.iter()
                .filter_map(|label| label.get("name").and_then(JsonValue::as_str))
                .collect::<Vec<_>>()
        }).unwrap_or_default(),
        "created_at": value.get("created_at").and_then(JsonValue::as_str),
        "updated_at": value.get("updated_at").and_then(JsonValue::as_str),
        "is_pull_request": value.get("pull_request").is_some(),
        "body": value.get("body").and_then(JsonValue::as_str).map(|body| truncate_for_output(body, 8_000)),
    })
}

fn summarize_gitlab_issue(value: &JsonValue) -> JsonValue {
    json!({
        "id": value.get("id").and_then(JsonValue::as_u64),
        "number": value.get("iid").and_then(JsonValue::as_u64),
        "iid": value.get("iid").and_then(JsonValue::as_u64),
        "title": value.get("title").and_then(JsonValue::as_str),
        "state": value.get("state").and_then(JsonValue::as_str),
        "url": value.get("web_url").and_then(JsonValue::as_str),
        "author": value.get("author").and_then(|user| user.get("username")).and_then(JsonValue::as_str),
        "labels": value.get("labels").and_then(JsonValue::as_array).map(|labels| {
            labels.iter()
                .filter_map(JsonValue::as_str)
                .collect::<Vec<_>>()
        }).unwrap_or_default(),
        "created_at": value.get("created_at").and_then(JsonValue::as_str),
        "updated_at": value.get("updated_at").and_then(JsonValue::as_str),
        "body": value.get("description").and_then(JsonValue::as_str).map(|body| truncate_for_output(body, 8_000)),
    })
}

fn summarize_github_pull(value: &JsonValue) -> JsonValue {
    json!({
        "number": value.get("number").and_then(JsonValue::as_u64),
        "title": value.get("title").and_then(JsonValue::as_str),
        "state": value.get("state").and_then(JsonValue::as_str),
        "url": value.get("html_url").and_then(JsonValue::as_str),
        "author": value.get("user").and_then(|user| user.get("login")).and_then(JsonValue::as_str),
        "draft": value.get("draft").and_then(JsonValue::as_bool),
        "merged": value.get("merged").and_then(JsonValue::as_bool),
        "base": value.get("base").and_then(|base| base.get("ref")).and_then(JsonValue::as_str),
        "head": value.get("head").and_then(|head| head.get("ref")).and_then(JsonValue::as_str),
        "created_at": value.get("created_at").and_then(JsonValue::as_str),
        "updated_at": value.get("updated_at").and_then(JsonValue::as_str),
        "body": value.get("body").and_then(JsonValue::as_str).map(|body| truncate_for_output(body, 8_000)),
    })
}

fn summarize_gitlab_merge_request(value: &JsonValue) -> JsonValue {
    json!({
        "id": value.get("id").and_then(JsonValue::as_u64),
        "number": value.get("iid").and_then(JsonValue::as_u64),
        "iid": value.get("iid").and_then(JsonValue::as_u64),
        "title": value.get("title").and_then(JsonValue::as_str),
        "state": value.get("state").and_then(JsonValue::as_str),
        "url": value.get("web_url").and_then(JsonValue::as_str),
        "author": value.get("author").and_then(|user| user.get("username")).and_then(JsonValue::as_str),
        "draft": value.get("draft").and_then(JsonValue::as_bool)
            .or_else(|| value.get("work_in_progress").and_then(JsonValue::as_bool)),
        "source_branch": value.get("source_branch").and_then(JsonValue::as_str),
        "target_branch": value.get("target_branch").and_then(JsonValue::as_str),
        "merge_status": value.get("detailed_merge_status").and_then(JsonValue::as_str)
            .or_else(|| value.get("merge_status").and_then(JsonValue::as_str)),
        "created_at": value.get("created_at").and_then(JsonValue::as_str),
        "updated_at": value.get("updated_at").and_then(JsonValue::as_str),
        "merged_at": value.get("merged_at").and_then(JsonValue::as_str),
        "body": value.get("description").and_then(JsonValue::as_str).map(|body| truncate_for_output(body, 8_000)),
    })
}

fn summarize_linear_issue_list(value: &JsonValue) -> Vec<JsonValue> {
    value
        .as_array()
        .map(|items| items.iter().map(summarize_linear_issue).collect())
        .unwrap_or_default()
}

fn summarize_linear_issue(value: &JsonValue) -> JsonValue {
    json!({
        "id": value.get("id").and_then(JsonValue::as_str),
        "identifier": value.get("identifier").and_then(JsonValue::as_str),
        "title": value.get("title").and_then(JsonValue::as_str),
        "state": value.get("state").and_then(|state| state.get("name")).and_then(JsonValue::as_str),
        "state_type": value.get("state").and_then(|state| state.get("type")).and_then(JsonValue::as_str),
        "url": value.get("url").and_then(JsonValue::as_str),
        "assignee": value.get("assignee").and_then(|user| user.get("name")).and_then(JsonValue::as_str),
        "team": value.get("team").and_then(|team| team.get("key")).and_then(JsonValue::as_str),
        "team_name": value.get("team").and_then(|team| team.get("name")).and_then(JsonValue::as_str),
        "project": value.get("project").and_then(|project| project.get("name")).and_then(JsonValue::as_str),
        "project_url": value.get("project").and_then(|project| project.get("url")).and_then(JsonValue::as_str),
        "priority": value.get("priority").and_then(JsonValue::as_i64),
        "estimate": value.get("estimate").and_then(JsonValue::as_f64),
        "created_at": value.get("createdAt").and_then(JsonValue::as_str),
        "updated_at": value.get("updatedAt").and_then(JsonValue::as_str),
        "archived_at": value.get("archivedAt").and_then(JsonValue::as_str),
        "body": value.get("description").and_then(JsonValue::as_str).map(|body| truncate_for_output(body, 8_000)),
    })
}

fn summarize_notion_page_list(value: &JsonValue) -> Vec<JsonValue> {
    value
        .as_array()
        .map(|items| items.iter().map(summarize_notion_page).collect())
        .unwrap_or_default()
}

fn summarize_notion_page(value: &JsonValue) -> JsonValue {
    json!({
        "id": value.get("id").and_then(JsonValue::as_str),
        "title": notion_page_title(value),
        "url": value.get("url").and_then(JsonValue::as_str),
        "created_time": value.get("created_time").and_then(JsonValue::as_str),
        "last_edited_time": value.get("last_edited_time").and_then(JsonValue::as_str),
        "archived": value.get("archived").and_then(JsonValue::as_bool),
        "in_trash": value.get("in_trash").and_then(JsonValue::as_bool),
        "parent": value.get("parent").cloned().unwrap_or(JsonValue::Null),
    })
}

fn notion_page_title(value: &JsonValue) -> Option<String> {
    let properties = value.get("properties")?.as_object()?;
    for property in properties.values() {
        if property.get("type").and_then(JsonValue::as_str) == Some("title") {
            let title = property
                .get("title")
                .and_then(JsonValue::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.get("plain_text").and_then(JsonValue::as_str))
                        .collect::<Vec<_>>()
                        .join("")
                })
                .filter(|title| !title.is_empty());
            if title.is_some() {
                return title;
            }
        }
    }
    None
}

#[cfg(test)]
fn summarize_notion_block_list(value: &JsonValue) -> Vec<JsonValue> {
    value
        .as_array()
        .map(|items| items.iter().map(summarize_notion_block).collect())
        .unwrap_or_default()
}

fn summarize_notion_block(value: &JsonValue) -> JsonValue {
    let block_type = value
        .get("type")
        .and_then(JsonValue::as_str)
        .unwrap_or("unknown");
    let content = value.get(block_type).unwrap_or(&JsonValue::Null);
    let text = notion_block_text(block_type, content);
    json!({
        "id": value.get("id").and_then(JsonValue::as_str),
        "type": block_type,
        "text": text,
        "has_children": value.get("has_children").and_then(JsonValue::as_bool).unwrap_or(false),
        "created_time": value.get("created_time").and_then(JsonValue::as_str),
        "last_edited_time": value.get("last_edited_time").and_then(JsonValue::as_str),
    })
}

fn notion_block_text(block_type: &str, content: &JsonValue) -> String {
    match block_type {
        "paragraph" | "heading_1" | "heading_2" | "heading_3" | "bulleted_list_item"
        | "numbered_list_item" | "quote" | "callout" | "to_do" => {
            notion_rich_text_plain(content.get("rich_text"))
        }
        "code" => notion_rich_text_plain(content.get("rich_text")),
        "child_page" | "child_database" => content
            .get("title")
            .and_then(JsonValue::as_str)
            .unwrap_or("")
            .to_string(),
        "bookmark" | "embed" | "link_preview" => content
            .get("url")
            .and_then(JsonValue::as_str)
            .unwrap_or("")
            .to_string(),
        "image" | "video" | "file" | "pdf" | "audio" => notion_file_like_url(content),
        _ => String::new(),
    }
}

fn notion_rich_text_plain(value: Option<&JsonValue>) -> String {
    value
        .and_then(JsonValue::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("plain_text").and_then(JsonValue::as_str))
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default()
}

fn notion_file_like_url(content: &JsonValue) -> String {
    for key in ["external", "file"] {
        if let Some(url) = content
            .get(key)
            .and_then(|value| value.get("url"))
            .and_then(JsonValue::as_str)
        {
            return url.to_string();
        }
    }
    String::new()
}

fn render_notion_blocks_markdown(blocks: &[JsonValue]) -> String {
    let mut lines = Vec::new();
    for block in blocks {
        let depth = block.get("depth").and_then(JsonValue::as_u64).unwrap_or(0) as usize;
        let indent = "  ".repeat(depth);
        let block_type = block
            .get("type")
            .and_then(JsonValue::as_str)
            .unwrap_or("unknown");
        let text = block
            .get("text")
            .and_then(JsonValue::as_str)
            .unwrap_or("")
            .trim();
        let children_truncated = block
            .get("children_truncated")
            .and_then(JsonValue::as_bool)
            .unwrap_or(false);
        let line = match block_type {
            "paragraph" => text.to_string(),
            "heading_1" => format!("# {text}"),
            "heading_2" => format!("## {text}"),
            "heading_3" => format!("### {text}"),
            "bulleted_list_item" => format!("- {text}"),
            "numbered_list_item" => format!("1. {text}"),
            "to_do" => format!("- [ ] {text}"),
            "quote" => format!("> {text}"),
            "callout" => format!("> {text}"),
            "code" => format!("```\n{text}\n```"),
            "divider" => "---".to_string(),
            "child_page" => format!("[Child page] {text}"),
            "child_database" => format!("[Child database] {text}"),
            "bookmark" | "embed" | "link_preview" => {
                if text.is_empty() {
                    String::new()
                } else {
                    format!("<{text}>")
                }
            }
            "image" | "video" | "file" | "pdf" | "audio" => {
                if text.is_empty() {
                    format!("[{block_type}]")
                } else {
                    format!("[{block_type}] {text}")
                }
            }
            _ => text.to_string(),
        };
        let line = if children_truncated && !line.is_empty() {
            format!("{line}\n  _(nested blocks truncated)_")
        } else {
            line
        };
        if !line.trim().is_empty() {
            let line = line
                .lines()
                .map(|line| format!("{indent}{line}"))
                .collect::<Vec<_>>()
                .join("\n");
            lines.push(line);
        }
    }
    truncate_for_output(&lines.join("\n\n"), 16_000)
}

fn summarize_sentry_issue_list(value: &JsonValue) -> Vec<JsonValue> {
    value
        .as_array()
        .map(|items| items.iter().map(summarize_sentry_issue).collect())
        .unwrap_or_default()
}

fn summarize_sentry_issue(value: &JsonValue) -> JsonValue {
    json!({
        "id": json_string_or_u64(value.get("id")),
        "short_id": value.get("shortId").and_then(JsonValue::as_str),
        "title": value.get("title").and_then(JsonValue::as_str),
        "culprit": value.get("culprit").and_then(JsonValue::as_str),
        "permalink": value.get("permalink").and_then(JsonValue::as_str),
        "level": value.get("level").and_then(JsonValue::as_str),
        "status": value.get("status").and_then(JsonValue::as_str),
        "status_details": value.get("statusDetails").cloned().unwrap_or(JsonValue::Null),
        "count": json_string_or_u64(value.get("count")),
        "user_count": value.get("userCount").and_then(JsonValue::as_u64)
            .or_else(|| value.get("userCount").and_then(JsonValue::as_str).and_then(|value| value.parse::<u64>().ok())),
        "first_seen": value.get("firstSeen").and_then(JsonValue::as_str),
        "last_seen": value.get("lastSeen").and_then(JsonValue::as_str),
        "project": value.get("project").and_then(|project| project.get("slug")).and_then(JsonValue::as_str),
        "metadata": value.get("metadata").cloned().unwrap_or(JsonValue::Null),
    })
}

fn summarize_slack_message_list(value: &JsonValue) -> Vec<JsonValue> {
    value
        .as_array()
        .map(|items| items.iter().map(summarize_slack_message).collect())
        .unwrap_or_default()
}

fn summarize_slack_message(value: &JsonValue) -> JsonValue {
    json!({
        "type": value.get("type").and_then(JsonValue::as_str),
        "subtype": value.get("subtype").and_then(JsonValue::as_str),
        "user": value.get("user").and_then(JsonValue::as_str),
        "bot_id": value.get("bot_id").and_then(JsonValue::as_str),
        "username": value.get("username").and_then(JsonValue::as_str),
        "ts": value.get("ts").and_then(JsonValue::as_str),
        "thread_ts": value.get("thread_ts").and_then(JsonValue::as_str),
        "reply_count": value.get("reply_count").and_then(JsonValue::as_u64),
        "text": value.get("text").and_then(JsonValue::as_str).map(|text| truncate_for_output(text, 6_000)),
    })
}

fn json_string_or_u64(value: Option<&JsonValue>) -> Option<String> {
    value
        .and_then(JsonValue::as_str)
        .map(str::to_string)
        .or_else(|| {
            value
                .and_then(JsonValue::as_u64)
                .map(|value| value.to_string())
        })
}

fn truncate_for_output(value: &str, max_chars: usize) -> String {
    let mut out = value.chars().take(max_chars).collect::<String>();
    if value.chars().count() > max_chars {
        out.push('…');
    }
    out
}

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "connector",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "connector": {
                    "type": "string",
                    "description": "Connector id. Currently supports github, gitlab, linear, notion, sentry, and slack."
                },
                "operation": {
                    "type": "string",
                    "description": "Operation to run. GitHub supports list_issues, read_issue, list_pull_requests, read_pull_request. GitLab supports list_issues, read_issue, list_merge_requests, read_merge_request. Linear supports list_issues, read_issue. Notion supports search_pages, read_page. Sentry supports list_issues, read_issue. Slack supports read_thread and post_message."
                },
                "repo": {
                    "type": "string",
                    "description": "Repository/project in owner/name, group/project, or org/project form, a matching service URL, or a Slack channel fallback. Required for GitHub/GitLab/Sentry list operations."
                },
                "id": {
                    "type": "string",
                    "description": "Service-specific string id, such as a Linear issue identifier, Notion page id, Sentry issue id, or Slack thread timestamp."
                },
                "query": {
                    "type": "string",
                    "description": "Optional search query for Notion search_pages or Sentry list_issues."
                },
                "org": {
                    "type": "string",
                    "description": "Optional organization slug for Sentry read_issue."
                },
                "project": {
                    "type": "string",
                    "description": "Optional project slug for Sentry list_issues when repo is not provided."
                },
                "number": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Issue, pull request, or merge request number/IID for read operations."
                },
                "state": {
                    "type": "string",
                    "description": "List state filter: open, closed, or all. Defaults to open."
                },
                "max_results": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 200,
                    "description": "Maximum items for list operations (capped at 25), Slack thread replies (capped at 100), or Notion read_page block budget (capped at 200). Defaults to 10 for lists, 20 for Slack threads, and 50 for Notion page content."
                },
                "max_depth": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 3,
                    "description": "Notion read_page nested block recursion depth. Defaults to 1 and is capped at 3."
                },
                "channel": {
                    "type": "string",
                    "description": "Slack channel/conversation ID for read_thread and post_message, e.g. C123456 or D123456."
                },
                "thread_ts": {
                    "type": "string",
                    "description": "Slack parent message timestamp for read_thread or threaded post_message replies."
                },
                "text": {
                    "type": "string",
                    "description": "Slack message text for post_message. Omiga caps this at 4000 characters."
                },
                "confirm_write": {
                    "type": "boolean",
                    "description": "Required true for Slack post_message because it writes to an external workspace after explicit user intent."
                }
            },
            "required": ["connector", "operation"]
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::connectors::CONNECTOR_TEST_ENV_LOCK;
    use std::net::TcpListener;
    use tempfile::tempdir;
    use wiremock::matchers::{
        body_partial_json, header, method, path, query_param, query_param_is_missing,
    };
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

    fn base_connector_args(connector: &str, operation: &str) -> ConnectorArgs {
        ConnectorArgs {
            connector: connector.to_string(),
            operation: operation.to_string(),
            repo: None,
            id: None,
            query: None,
            org: None,
            project: None,
            number: None,
            state: None,
            max_results: None,
            max_depth: None,
            channel: None,
            thread_ts: None,
            text: None,
            confirm_write: false,
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

    #[test]
    fn parses_github_repo_forms() {
        assert_eq!(
            parse_github_repo("openai/codex").unwrap(),
            ("openai".to_string(), "codex".to_string())
        );
        assert_eq!(
            parse_github_repo("https://github.com/openai/codex.git").unwrap(),
            ("openai".to_string(), "codex".to_string())
        );
        assert!(parse_github_repo("openai/codex/issues/1").is_err());
        assert!(parse_github_repo("../codex").is_err());
    }

    #[test]
    fn parses_gitlab_project_paths() {
        assert_eq!(
            parse_gitlab_project_path("open-source/subgroup/project").unwrap(),
            "open-source/subgroup/project"
        );
        assert_eq!(
            parse_gitlab_project_path("https://gitlab.com/open-source/project.git").unwrap(),
            "open-source/project"
        );
        assert_eq!(
            percent_encode_gitlab_project_path("open-source/subgroup/project"),
            "open-source%2Fsubgroup%2Fproject"
        );
        assert_eq!(
            percent_encode_url_component("cursor/with space+plus"),
            "cursor%2Fwith%20space%2Bplus"
        );
        assert!(parse_gitlab_project_path("single-segment").is_err());
        assert!(parse_gitlab_project_path("open-source/project/-/issues/1").is_err());
        assert!(parse_gitlab_project_path("../project").is_err());
    }

    #[test]
    fn parses_sentry_org_project_forms() {
        assert_eq!(
            parse_sentry_org_project("acme/frontend").unwrap(),
            ("acme".to_string(), "frontend".to_string())
        );
        assert_eq!(
            parse_sentry_org_project("https://sentry.io/acme/frontend").unwrap(),
            ("acme".to_string(), "frontend".to_string())
        );
        assert!(parse_sentry_org_project("single-segment").is_err());
        assert!(parse_sentry_org_project("../frontend").is_err());
    }

    #[test]
    fn validates_slack_connector_identifiers() {
        let mut args = base_connector_args("slack", "read_thread");
        args.channel = Some("#C123ABC".to_string());
        args.thread_ts = Some("1712345678.123456".to_string());
        assert_eq!(slack_channel(&args).unwrap(), "C123ABC");
        assert_eq!(slack_thread_ts(&args).unwrap(), "1712345678.123456");

        args.channel = Some("C123/invalid".to_string());
        assert!(slack_channel(&args).is_err());

        args.channel = Some("C123ABC".to_string());
        args.thread_ts = Some("1712345678..123456".to_string());
        assert!(slack_thread_ts(&args).is_err());
    }

    #[tokio::test]
    async fn notion_read_page_fetches_paginated_nested_blocks_offline() {
        let _lock = CONNECTOR_TEST_ENV_LOCK.lock().await;
        let dir = tempdir().unwrap();
        let Some(server) = start_connector_mock_server().await else {
            return;
        };
        let _env = ScopedEnv::set(&[
            (
                "OMIGA_CONNECTORS_CONFIG_PATH",
                dir.path()
                    .join("connectors.json")
                    .to_string_lossy()
                    .into_owned(),
            ),
            ("OMIGA_NOTION_API_BASE_URL", format!("{}/v1", server.uri())),
            ("NOTION_TOKEN", "notion-test-token".to_string()),
            ("NOTION_API_KEY", String::new()),
            ("NO_PROXY", local_no_proxy()),
            ("no_proxy", local_no_proxy()),
        ]);

        Mock::given(method("GET"))
            .and(path("/v1/pages/page-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "object": "page",
                "id": "page-1",
                "url": "https://notion.so/page-1",
                "properties": {
                    "Name": {
                        "type": "title",
                        "title": [{"plain_text": "Connector Runbook"}]
                    }
                }
            })))
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/v1/blocks/page-1/children"))
            .and(query_param("page_size", "4"))
            .and(query_param_is_missing("start_cursor"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "object": "list",
                "results": [
                    {
                        "id": "parent-block",
                        "type": "paragraph",
                        "paragraph": {
                            "rich_text": [{"plain_text": "Parent"}]
                        },
                        "has_children": true
                    },
                    {
                        "id": "after-block",
                        "type": "paragraph",
                        "paragraph": {
                            "rich_text": [{"plain_text": "After"}]
                        },
                        "has_children": false
                    }
                ],
                "has_more": false
            })))
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/v1/blocks/parent-block/children"))
            .and(query_param("page_size", "3"))
            .and(query_param_is_missing("start_cursor"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "object": "list",
                "results": [
                    {
                        "id": "child-a",
                        "type": "bulleted_list_item",
                        "bulleted_list_item": {
                            "rich_text": [{"plain_text": "Child A"}]
                        },
                        "has_children": false
                    }
                ],
                "has_more": true,
                "next_cursor": "cursor/with space+plus"
            })))
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/v1/blocks/parent-block/children"))
            .and(query_param("page_size", "3"))
            .and(query_param("start_cursor", "cursor/with space+plus"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "object": "list",
                "results": [
                    {
                        "id": "child-b",
                        "type": "bulleted_list_item",
                        "bulleted_list_item": {
                            "rich_text": [{"plain_text": "Child B"}]
                        },
                        "has_children": true
                    }
                ],
                "has_more": false
            })))
            .expect(1)
            .mount(&server)
            .await;

        let ctx = ToolContext::new(dir.path());
        let mut args = base_connector_args("notion", "read_page");
        args.id = Some("page-1".to_string());
        args.max_results = Some(4);
        args.max_depth = Some(1);

        let output = match execute_connector_json(&ctx, args).await {
            Ok(output) => output,
            Err(err) => {
                let received = server
                    .received_requests()
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .map(|request| request.url.to_string())
                    .collect::<Vec<_>>();
                panic!("Notion mock connector request failed: {err:?}; received={received:?}");
            }
        };
        assert_eq!(output["connector"], "notion");
        assert_eq!(output["page"]["title"], "Connector Runbook");
        assert_eq!(output["block_count"], 4);
        assert_eq!(output["max_depth"], 1);
        assert_eq!(output["has_more_blocks"], false);
        assert_eq!(output["truncated"], true);
        assert_eq!(output["blocks"][0]["text"], "Parent");
        assert_eq!(output["blocks"][1]["depth"], 1);
        assert_eq!(output["blocks"][2]["children_truncated"], true);
        assert!(output["content_markdown"]
            .as_str()
            .unwrap()
            .contains("\n  - Child A"));
        assert!(output["content_markdown"]
            .as_str()
            .unwrap()
            .contains("nested blocks truncated"));

        let received = server.received_requests().await.unwrap_or_default();
        let page_request = received
            .iter()
            .find(|request| request.url.path() == "/v1/pages/page-1")
            .expect("page request");
        assert_eq!(
            page_request
                .headers
                .get("authorization")
                .and_then(|value| value.to_str().ok()),
            Some("Bearer notion-test-token")
        );
        assert_eq!(
            page_request
                .headers
                .get("notion-version")
                .and_then(|value| value.to_str().ok()),
            Some("2022-06-28")
        );
    }

    #[tokio::test]
    async fn native_connector_smoke_tests_use_mock_http_endpoints() {
        let _lock = CONNECTOR_TEST_ENV_LOCK.lock().await;
        let dir = tempdir().unwrap();
        let Some(server) = start_connector_mock_server().await else {
            return;
        };
        let _env = ScopedEnv::set(&[
            (
                "OMIGA_CONNECTORS_CONFIG_PATH",
                dir.path()
                    .join("connectors.json")
                    .to_string_lossy()
                    .into_owned(),
            ),
            (
                "OMIGA_GITHUB_API_BASE_URL",
                format!("{}/github", server.uri()),
            ),
            ("GITHUB_TOKEN", "gh-test-token".to_string()),
            ("GH_TOKEN", String::new()),
            (
                "OMIGA_GITLAB_API_BASE_URL",
                format!("{}/gitlab", server.uri()),
            ),
            ("GITLAB_TOKEN", "gitlab-test-token".to_string()),
            (
                "OMIGA_LINEAR_GRAPHQL_URL",
                format!("{}/linear/graphql", server.uri()),
            ),
            ("LINEAR_ACCESS_TOKEN", String::new()),
            ("LINEAR_API_KEY", "linear-test-key".to_string()),
            (
                "OMIGA_SENTRY_API_BASE_URL",
                format!("{}/sentry", server.uri()),
            ),
            ("SENTRY_AUTH_TOKEN", "sentry-test-token".to_string()),
            ("NO_PROXY", local_no_proxy()),
            ("no_proxy", local_no_proxy()),
        ]);

        Mock::given(method("GET"))
            .and(path("/github/repos/openai/codex/issues"))
            .and(query_param("state", "open"))
            .and(query_param("per_page", "2"))
            .and(header("authorization", "Bearer gh-test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {
                    "number": 1,
                    "title": "Issue",
                    "state": "open",
                    "html_url": "https://github.com/openai/codex/issues/1",
                    "user": {"login": "octo"},
                    "labels": [{"name": "bug"}],
                    "body": "body"
                },
                {
                    "number": 2,
                    "title": "PR",
                    "state": "open",
                    "pull_request": {}
                }
            ])))
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/gitlab/projects/group%2Fproject/issues"))
            .and(query_param("state", "opened"))
            .and(query_param("per_page", "2"))
            .and(header("private-token", "gitlab-test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {
                    "id": 10,
                    "iid": 3,
                    "title": "GitLab issue",
                    "state": "opened",
                    "web_url": "https://gitlab.com/group/project/-/issues/3",
                    "author": {"username": "alice"},
                    "labels": ["bug"],
                    "description": "body"
                }
            ])))
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path("/linear/graphql"))
            .and(header("authorization", "linear-test-key"))
            .and(body_partial_json(json!({
                "variables": {"first": 2}
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {
                    "issues": {
                        "nodes": [
                            {
                                "id": "lin-1",
                                "identifier": "ENG-1",
                                "title": "Linear issue",
                                "description": "body",
                                "url": "https://linear.app/acme/issue/ENG-1",
                                "state": {"name": "Todo", "type": "unstarted"},
                                "assignee": {"name": "Alice"},
                                "team": {"key": "ENG", "name": "Engineering"}
                            }
                        ]
                    }
                }
            })))
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/sentry/projects/acme/frontend/issues/"))
            .and(query_param("query", "is:unresolved"))
            .and(header("authorization", "Bearer sentry-test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {
                    "id": "123",
                    "shortId": "ACME-1",
                    "title": "TypeError",
                    "status": "unresolved",
                    "permalink": "https://sentry.io/issues/123",
                    "project": {"slug": "frontend"},
                    "metadata": {"type": "TypeError"}
                }
            ])))
            .expect(1)
            .mount(&server)
            .await;

        let ctx = ToolContext::new(dir.path());

        let mut github_args = base_connector_args("github", "list_issues");
        github_args.repo = Some("openai/codex".to_string());
        github_args.max_results = Some(2);
        let github = execute_connector_json(&ctx, github_args).await.unwrap();
        assert_eq!(github["result_count"], 1);
        assert_eq!(github["raw_count"], 2);
        assert_eq!(github["results"][0]["author"], "octo");

        let mut gitlab_args = base_connector_args("gitlab", "list_issues");
        gitlab_args.repo = Some("group/project".to_string());
        gitlab_args.max_results = Some(2);
        let gitlab = execute_connector_json(&ctx, gitlab_args).await.unwrap();
        assert_eq!(gitlab["result_count"], 1);
        assert_eq!(gitlab["results"][0]["number"], 3);

        let mut linear_args = base_connector_args("linear", "list_issues");
        linear_args.max_results = Some(2);
        let linear = execute_connector_json(&ctx, linear_args).await.unwrap();
        assert_eq!(linear["result_count"], 1);
        assert_eq!(linear["results"][0]["identifier"], "ENG-1");

        let mut sentry_args = base_connector_args("sentry", "list_issues");
        sentry_args.repo = Some("acme/frontend".to_string());
        sentry_args.max_results = Some(2);
        let sentry = execute_connector_json(&ctx, sentry_args).await.unwrap();
        assert_eq!(sentry["result_count"], 1);
        assert_eq!(sentry["results"][0]["short_id"], "ACME-1");
    }

    #[tokio::test]
    async fn slack_connector_reads_threads_and_posts_only_with_confirmation() {
        let _lock = CONNECTOR_TEST_ENV_LOCK.lock().await;
        let dir = tempdir().unwrap();
        let secrets_dir = tempdir().unwrap();
        let Some(server) = start_connector_mock_server().await else {
            return;
        };
        let _env = ScopedEnv::set(&[
            (
                "OMIGA_CONNECTORS_CONFIG_PATH",
                dir.path()
                    .join("connectors.json")
                    .to_string_lossy()
                    .into_owned(),
            ),
            (
                "OMIGA_CONNECTOR_SECRET_STORE_DIR",
                secrets_dir.path().to_string_lossy().into_owned(),
            ),
            (
                "OMIGA_SLACK_API_BASE_URL",
                format!("{}/slack", server.uri()),
            ),
            ("SLACK_BOT_TOKEN", "slack-tool-token".to_string()),
            ("NO_PROXY", local_no_proxy()),
            ("no_proxy", local_no_proxy()),
        ]);

        Mock::given(method("GET"))
            .and(path("/slack/conversations.replies"))
            .and(query_param("channel", "C123ABC"))
            .and(query_param("ts", "1712345678.123456"))
            .and(query_param("limit", "2"))
            .and(header("authorization", "Bearer slack-tool-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "ok": true,
                "messages": [
                    {
                        "type": "message",
                        "user": "U111",
                        "text": "Root message",
                        "ts": "1712345678.123456",
                        "thread_ts": "1712345678.123456",
                        "reply_count": 1,
                        "private_field": "hidden"
                    },
                    {
                        "type": "message",
                        "user": "U222",
                        "text": "Reply",
                        "ts": "1712345680.000100",
                        "thread_ts": "1712345678.123456"
                    }
                ],
                "has_more": false
            })))
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path("/slack/chat.postMessage"))
            .and(header("authorization", "Bearer slack-tool-token"))
            .and(body_partial_json(json!({
                "channel": "C123ABC",
                "text": "Ship it",
                "thread_ts": "1712345678.123456"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "ok": true,
                "channel": "C123ABC",
                "ts": "1712345690.000200",
                "message": {
                    "type": "message",
                    "bot_id": "B111",
                    "text": "Ship it",
                    "ts": "1712345690.000200",
                    "thread_ts": "1712345678.123456"
                }
            })))
            .expect(1)
            .mount(&server)
            .await;

        let ctx = ToolContext::new(dir.path());

        let mut read_args = base_connector_args("slack", "read_thread");
        read_args.channel = Some("C123ABC".to_string());
        read_args.thread_ts = Some("1712345678.123456".to_string());
        read_args.max_results = Some(2);
        let thread = execute_connector_json(&ctx, read_args).await.unwrap();
        assert_eq!(thread["connector"], "slack");
        assert_eq!(thread["operation"], "read_thread");
        assert_eq!(thread["result_count"], 2);
        assert_eq!(thread["messages"][0]["user"], "U111");
        assert_eq!(thread["messages"][1]["text"], "Reply");
        assert!(thread["messages"][0].get("private_field").is_none());

        let mut blocked_write = base_connector_args("slack", "post_message");
        blocked_write.channel = Some("C123ABC".to_string());
        blocked_write.thread_ts = Some("1712345678.123456".to_string());
        blocked_write.text = Some("Ship it".to_string());
        match execute_connector_json(&ctx, blocked_write).await {
            Err(ToolError::PermissionDenied { action }) => {
                assert!(action.contains("confirm_write=true"));
            }
            other => panic!("expected Slack write confirmation gate, got {other:?}"),
        }

        let mut post_args = base_connector_args("slack", "post_message");
        post_args.channel = Some("C123ABC".to_string());
        post_args.thread_ts = Some("1712345678.123456".to_string());
        post_args.text = Some("Ship it".to_string());
        post_args.confirm_write = true;
        let posted = execute_connector_json(&ctx, post_args).await.unwrap();
        assert_eq!(posted["connector"], "slack");
        assert_eq!(posted["operation"], "post_message");
        assert_eq!(posted["channel"], "C123ABC");
        assert_eq!(posted["ts"], "1712345690.000200");
        assert_eq!(posted["message"]["bot_id"], "B111");

        let audit_events = connectors::list_connector_audit_events(Some("slack"), Some(10))
            .expect("slack audit events");
        assert_eq!(audit_events.len(), 3);
        assert!(audit_events.iter().any(|event| {
            event.operation == "read_thread"
                && event.access == connectors::ConnectorAuditAccess::Read
                && event.outcome == connectors::ConnectorAuditOutcome::Ok
                && event.target.as_deref() == Some("C123ABC thread 1712345678.123456")
        }));
        assert!(audit_events.iter().any(|event| {
            event.operation == "post_message"
                && event.access == connectors::ConnectorAuditAccess::Write
                && event.outcome == connectors::ConnectorAuditOutcome::Blocked
                && event.confirmation_required
                && !event.confirmed
                && event.error_code.as_deref() == Some("confirmation_required")
        }));
        assert!(audit_events.iter().any(|event| {
            event.operation == "post_message"
                && event.access == connectors::ConnectorAuditAccess::Write
                && event.outcome == connectors::ConnectorAuditOutcome::Ok
                && event.confirmed
        }));
    }

    #[test]
    fn summarizes_issue_without_leaking_extra_fields() {
        let issue = json!({
            "number": 7,
            "title": "Bug",
            "state": "open",
            "html_url": "https://github.com/o/r/issues/7",
            "user": {"login": "alice", "token": "secret"},
            "labels": [{"name": "bug"}],
            "body": "body",
            "private_field": "hidden"
        });
        let summary = summarize_github_issue(&issue);
        assert_eq!(summary["number"], 7);
        assert_eq!(summary["author"], "alice");
        assert_eq!(summary["labels"][0], "bug");
        assert!(summary.get("private_field").is_none());
        assert!(summary.get("token").is_none());
    }

    #[test]
    fn summarizes_gitlab_merge_request_without_leaking_extra_fields() {
        let merge_request = json!({
            "id": 42,
            "iid": 5,
            "title": "Improve connector",
            "state": "opened",
            "web_url": "https://gitlab.com/o/r/-/merge_requests/5",
            "author": {"username": "alice", "private_token": "secret"},
            "source_branch": "feature",
            "target_branch": "main",
            "draft": false,
            "description": "body",
            "private_field": "hidden"
        });
        let summary = summarize_gitlab_merge_request(&merge_request);
        assert_eq!(summary["number"], 5);
        assert_eq!(summary["author"], "alice");
        assert_eq!(summary["source_branch"], "feature");
        assert!(summary.get("private_field").is_none());
        assert!(summary.get("private_token").is_none());
    }

    #[test]
    fn summarizes_linear_issue_without_leaking_extra_fields() {
        let issue = json!({
            "id": "abc",
            "identifier": "ENG-123",
            "title": "Improve connector",
            "url": "https://linear.app/acme/issue/ENG-123",
            "state": {"name": "Todo", "type": "unstarted", "secret": "hidden"},
            "assignee": {"name": "Alice", "token": "secret"},
            "team": {"key": "ENG", "name": "Engineering"},
            "description": "body",
            "private_field": "hidden"
        });
        let summary = summarize_linear_issue(&issue);
        assert_eq!(summary["identifier"], "ENG-123");
        assert_eq!(summary["state"], "Todo");
        assert_eq!(summary["assignee"], "Alice");
        assert!(summary.get("private_field").is_none());
        assert!(summary.get("token").is_none());
    }

    #[test]
    fn summarizes_notion_page_title() {
        let page = json!({
            "id": "page-id",
            "url": "https://notion.so/page-id",
            "properties": {
                "Name": {
                    "type": "title",
                    "title": [
                        {"plain_text": "Connector"},
                        {"plain_text": " Notes"}
                    ]
                }
            },
            "private_field": "hidden"
        });
        let summary = summarize_notion_page(&page);
        assert_eq!(summary["id"], "page-id");
        assert_eq!(summary["title"], "Connector Notes");
        assert!(summary.get("private_field").is_none());
    }

    #[test]
    fn renders_notion_blocks_as_markdown_without_leaking_extra_fields() {
        let blocks = json!([
            {
                "id": "heading",
                "type": "heading_1",
                "heading_1": {
                    "rich_text": [{"plain_text": "Project Notes", "href": "secret"}]
                },
                "has_children": false,
                "private_field": "hidden"
            },
            {
                "id": "para",
                "type": "paragraph",
                "paragraph": {
                    "rich_text": [{"plain_text": "Ship connector blocks."}]
                },
                "has_children": true,
                "children_truncated": true
            },
            {
                "id": "code",
                "type": "code",
                "code": {
                    "rich_text": [{"plain_text": "cargo test"}],
                    "language": "rust"
                },
                "has_children": false
            }
        ]);
        let mut summaries = summarize_notion_block_list(&blocks);
        summaries[1]["children_truncated"] = json!(true);
        assert_eq!(summaries[0]["type"], "heading_1");
        assert_eq!(summaries[0]["text"], "Project Notes");
        assert!(summaries[0].get("private_field").is_none());

        let markdown = render_notion_blocks_markdown(&summaries);
        assert!(markdown.contains("# Project Notes"));
        assert!(markdown.contains("Ship connector blocks."));
        assert!(markdown.contains("nested blocks truncated"));
        assert!(markdown.contains("cargo test"));
        assert!(!markdown.contains("secret"));
    }

    #[test]
    fn renders_notion_nested_blocks_with_depth_indentation() {
        let blocks = vec![
            json!({
                "type": "paragraph",
                "text": "Parent",
                "has_children": true,
                "depth": 0,
                "children_loaded": true,
                "children_truncated": false
            }),
            json!({
                "type": "bulleted_list_item",
                "text": "Child",
                "has_children": true,
                "depth": 1,
                "children_loaded": false,
                "children_truncated": true
            }),
        ];

        let markdown = render_notion_blocks_markdown(&blocks);
        assert!(markdown.contains("Parent"));
        assert!(markdown.contains("\n  - Child"));
        assert!(markdown.contains("\n    _(nested blocks truncated)_"));
    }

    #[test]
    fn summarizes_sentry_issue_without_leaking_extra_fields() {
        let issue = json!({
            "id": "123",
            "shortId": "ACME-1",
            "title": "TypeError",
            "status": "unresolved",
            "permalink": "https://sentry.io/issues/123",
            "project": {"slug": "frontend", "token": "secret"},
            "metadata": {"type": "TypeError"},
            "private_field": "hidden"
        });
        let summary = summarize_sentry_issue(&issue);
        assert_eq!(summary["id"], "123");
        assert_eq!(summary["short_id"], "ACME-1");
        assert_eq!(summary["project"], "frontend");
        assert!(summary.get("private_field").is_none());
        assert!(summary.get("token").is_none());
    }

    #[test]
    fn issue_list_filters_pull_requests() {
        let items = json!([
            {"number": 1, "title": "Issue", "state": "open"},
            {"number": 2, "title": "PR", "state": "open", "pull_request": {}}
        ]);
        let summaries = summarize_github_issue_list(&items);
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0]["number"], 1);
    }

    #[test]
    fn schema_is_named_connector() {
        let schema = schema();
        assert_eq!(schema.name, "connector");
        assert!(schema.description.contains("GitHub"));
        assert!(schema.description.contains("GitLab"));
        assert!(schema.description.contains("Linear"));
        assert!(schema.description.contains("Notion"));
        assert!(schema.description.contains("Sentry"));
        assert!(schema.description.contains("Slack"));
        assert_eq!(schema.parameters["properties"]["max_depth"]["maximum"], 3);
        assert_eq!(
            schema.parameters["properties"]["confirm_write"]["type"],
            "boolean"
        );
    }
}
