# Omiga Documentation

This directory contains design, validation, and user-facing documentation for Omiga. Keep documents current and actionable; remove completion reports and one-off migration summaries.

## User guides

| Document | Purpose |
| --- | --- |
| [`FIRST_TIME_USER_GUIDE.md`](FIRST_TIME_USER_GUIDE.md) | Getting started: install, configure, first session. |
| [`PLUGIN_DEVELOPER_GUIDE.md`](PLUGIN_DEVELOPER_GUIDE.md) | How to create and publish Omiga plugins and operators. |
| [`MIGRATION_GUIDE.md`](MIGRATION_GUIDE.md) | Upgrading from v0.x to v1.0: schema changes, breaking changes, rollback. |
| [`QUICK_REFERENCE.md`](QUICK_REFERENCE.md) | General developer quick reference. |

## Core documentation

| Document | Purpose |
| --- | --- |
| [`architecture.md`](architecture.md) | System architecture and major runtime boundaries. |
| [`SECURITY_MODEL.md`](SECURITY_MODEL.md) | Security model, trust boundaries, and permission considerations. |
| [`REAL_LLM_VALIDATION.md`](REAL_LLM_VALIDATION.md) | Manual validation path for real provider-backed runs. |
| [`MOCK_LLM_RUNTIME_VALIDATION.md`](MOCK_LLM_RUNTIME_VALIDATION.md) | Deterministic mock LLM validation path. |
| [`agent-card-spec.md`](agent-card-spec.md) | Agent card schema and compatibility rules. |
| [`unified-memory-design.md`](unified-memory-design.md) | Memory architecture and recall model. |
| [`pageindex-wiki-analysis.md`](pageindex-wiki-analysis.md) | PageIndex/wiki analysis and keyword retrieval design. |
| [`retrieval-plugin-protocol.md`](retrieval-plugin-protocol.md) | Retrieval plugin protocol specification. |

## Agent and orchestration

| Document | Purpose |
| --- | --- |
| [`AGENT_SCHEDULER_GUIDE.md`](AGENT_SCHEDULER_GUIDE.md) | Scheduler usage and behavior guide. |
| [`AGENT_QUICK_REFERENCE.md`](AGENT_QUICK_REFERENCE.md) | Agent command and workflow quick reference. |
| [`AGENT_HOT_RELOAD.md`](AGENT_HOT_RELOAD.md) | Agent hot reload design and behavior. |
| [`AGENT_HOT_RELOAD_QUICKSTART.md`](AGENT_HOT_RELOAD_QUICKSTART.md) | Short setup guide for hot reload. |
| [`AGENT_INTEGRATION_EXAMPLE.md`](AGENT_INTEGRATION_EXAMPLE.md) | Example integration patterns. |
| [`AGENT_TEST_PLAN.md`](AGENT_TEST_PLAN.md) | Agent test planning notes. |
| [`AGENT_TS_VS_RUST.md`](AGENT_TS_VS_RUST.md) | TypeScript/Rust agent-runtime tradeoff notes. |
| [`AGENT_ORCHESTRATION_MOCK_CHECKLIST.md`](AGENT_ORCHESTRATION_MOCK_CHECKLIST.md) | Mock orchestration validation checklist. |
| [`AGENT_ORCHESTRATION_REAL_RUNTIME_PREP.md`](AGENT_ORCHESTRATION_REAL_RUNTIME_PREP.md) | Preparation notes for real runtime validation. |
| [`AGENT_ORCHESTRATION_SCHEDULE_MANUAL_VALIDATION.md`](AGENT_ORCHESTRATION_SCHEDULE_MANUAL_VALIDATION.md) | Manual validation guide for schedule flows. |

## Permissions, tools, and operators

| Document | Purpose |
| --- | --- |
| [`PERMISSION_SYSTEM_DESIGN.md`](PERMISSION_SYSTEM_DESIGN.md) | Permission system design. |
| [`PERMISSION_INTEGRATION_GUIDE.md`](PERMISSION_INTEGRATION_GUIDE.md) | Permission integration guide. |
| [`OPERATOR_PLUGIN_MANIFEST.md`](OPERATOR_PLUGIN_MANIFEST.md) | Operator plugin manifest schema reference. |
| [`TOOLS_PARITY.md`](TOOLS_PARITY.md) | Built-in tool parity tracking vs Claude Code. |
| [`SKILL_TOOL_PARITY.md`](SKILL_TOOL_PARITY.md) | Skill/tool parity notes. |
| [`IMPLEMENTATION_GUIDE.md`](IMPLEMENTATION_GUIDE.md) | Implementation guide and code templates. |

## Research system

| Document | Purpose |
| --- | --- |
| [`RESEARCH_GOAL_MIGRATION_PLAN.md`](RESEARCH_GOAL_MIGRATION_PLAN.md) | Research goal feature design (retained as architecture context). |

## Documentation policy

Keep documents that are one of:

- release-facing user or operator guidance;
- current design or architecture reference;
- validation instructions that can still be executed.

Remove: completion announcements, old execution reports, one-off bug patches, duplicate migration summaries.
