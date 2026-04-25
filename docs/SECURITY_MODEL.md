# Omiga Security Model

Last reviewed: 2026-04-25
Status: living document. Update this file whenever a security boundary, permission mode, execution backend, web access rule, or secret-handling behavior changes.

## Security Goals

1. Keep user projects and local files under explicit user control.
2. Prevent tool calls from silently escalating from read-only reasoning to destructive local actions.
3. Make high-risk actions observable, reviewable, and eventually auditable after restart.
4. Keep web access public by default: block local/private/internal network targets unless an explicit future policy allows them.
5. Treat local, Docker, SSH, and future cloud execution backends as different trust boundaries.
6. Avoid presenting experimental backends as production-safe.

## Trust Boundaries

| Boundary | Trust level | Examples | Required controls |
| --- | --- | --- | --- |
| User operating the desktop app | Trusted | Clicking approvals, editing settings, choosing project path | Clear prompts, reversible settings, visible risk labels |
| React/Tauri frontend | Trusted UI, but exposed to rendering/content risks | Chat UI, settings, Monaco, rendered markdown | CSP hardening, output sanitization, narrow Tauri capabilities |
| Tauri backend | Privileged local process | File tools, shell, persistence, LLM calls | Permission gates, audit logs, safe defaults, tests |
| LLM/provider | Untrusted planner/executor suggestions | Tool call JSON, text output, model reasoning | Schema validation, permission checks, output limits |
| Local project workspace | User data | Source files, config, `.omiga`, `.claude` | Path normalization, project-root checks, explicit approvals |
| External web | Untrusted content | `web_fetch`, `web_search`, redirects | SSRF protection, secret-like URL rejection, response limits |
| Local shell | High risk | `bash`, background tasks, execution envs | Dangerous command detection, timeout/cancel, env filtering |
| Containers / SSH / cloud backends | Mixed trust | Docker, SSH, Modal, Daytona, Singularity | Isolation, resource limits, secret boundaries, cleanup |
| MCP servers | Operator-configured trust | Local stdio/HTTP MCP servers | Namespacing, timeout, deny rules, resource limits |

## Current Controls

### Permission manager

The backend has a `PermissionManager` that tracks rules, session approvals, time-window approvals, one-shot approvals, session denials, recent denials, and composer permission stance. Current known limitation: recent denials are process-memory state, not a durable audit log.

### Tool deny rules

Omiga reads Claude-style deny rules and project-level `.omiga/permissions.json` deny entries for built-in and MCP tool filtering. MCP tools are namespaced as `mcp__server__tool` and can be blanket-denied at server level.

### Bash command controls

The bash tool applies timeouts, cancellation, path resolution, output caps, selected destructive-command warnings, and blocks known high-risk patterns such as root filesystem deletion and fork bombs. This is a guardrail, not a complete shell sandbox.

### Web safety controls

`web_fetch`/`web_search` safety code blocks loopback, private, local-only, internal metadata hostnames, secret-like URL credentials/query params, and unsafe redirects. DNS resolution is checked to avoid private-network targets.

### Execution environment controls

Execution backends are separated by type: Local, Docker, SSH, Modal, Daytona, and Singularity. Local/Docker/SSH are the most relevant near-term backends. Modal and Daytona currently return explicit unavailable/not-yet-implemented errors for core API/SDK operations and must remain marked experimental until complete.

### Persistence

Session, message, orchestration event, working memory, and background-agent task persistence exist in SQLite. Security audit records should be added explicitly instead of piggybacking on transient state.

## Known Risks and Required Mitigations

| Risk | Current state | Required mitigation | Priority |
| --- | --- | --- | --- |
| Null Tauri CSP | `src-tauri/tauri.conf.json` currently sets `csp: null`. | Design and test CSP compatible with React/MUI/Monaco/rendered markdown. | P1 |
| Non-durable permission audit | Permission denials/approvals are not yet an authoritative persisted audit trail. | Add schema + commands/tests for permission audit records. | P0 |
| Shell is powerful by design | Bash runs local commands with user privileges. | Continue dangerous pattern tests; add policy matrix and clearer UI risk labels. | P0 |
| Secret leakage through tools | No complete encrypted secret store/leak detector is documented as production-ready. | Design secret store, redaction, and input/output leak scans. | P1 |
| Experimental backends may look usable | Multiple execution backends are registered. | Mark incomplete backends experimental/unavailable in UI/docs. | P1 |
| Web content prompt injection | Fetched content can influence model context. | Wrap external content with provenance and injection warnings; add sanitizer tests. | P1 |
| MCP server trust | User-configured MCP tools can expose broad capabilities. | Maintain deny rules, namespace clarity, timeout/resource limits, and audit MCP tool use. | P1 |

## Secure Change Process

Before changing a security boundary:

1. Update this document with the intended boundary/control change.
2. Add or update regression tests for the boundary.
3. Verify the change with the smallest relevant test command.
4. Include rollback notes in the commit/PR description.
5. Update `FEATURE_STATUS.md` if the feature status changes.

## Verification Matrix

| Control | Minimum test evidence |
| --- | --- |
| Permission deny matching | Unit tests for built-in aliases and MCP blanket denies |
| Permission audit persistence | SQLite migration/unit test showing records survive manager restart |
| Bash dangerous commands | Unit tests for blocked/destructive patterns and allowed safe commands |
| Web SSRF protection | Unit tests for localhost/private IP/metadata host/DNS redirect cases |
| CSP | Frontend build plus app smoke test with Monaco/markdown/settings open |
| Docker backend | Config validation test plus optional integration test gated on Docker availability |
| SSH backend | Config/path escaping tests plus optional fixture integration |

## Out of Scope for Immediate Phase

- Implementing a full WASM tool sandbox.
- Adding public remote gateway exposure.
- Adding encrypted secret store implementation without a separate design/test plan.
- Enforcing CSP without first validating frontend compatibility.
