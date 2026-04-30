# Omiga Documentation

This directory contains design, validation, and operator-facing documentation for Omiga. Historical completion reports and one-off migration summaries are intentionally not kept here; release-facing material should stay current and actionable.

## Core documentation

| Document | Purpose |
| --- | --- |
| [`architecture.md`](architecture.md) | System architecture and major runtime boundaries. |
| [`SECURITY_MODEL.md`](SECURITY_MODEL.md) | Security model, trust boundaries, and permission considerations. |
| [`REAL_LLM_VALIDATION.md`](REAL_LLM_VALIDATION.md) | Manual validation path for real provider-backed runs. |
| [`MOCK_LLM_RUNTIME_VALIDATION.md`](MOCK_LLM_RUNTIME_VALIDATION.md) | Deterministic mock LLM validation path. |
| [`agent-card-spec.md`](agent-card-spec.md) | Agent card schema and compatibility rules. |
| [`unified-memory-design.md`](unified-memory-design.md) | Memory architecture and recall model. |
| [`pageindex-wiki-analysis.md`](pageindex-wiki-analysis.md) | PageIndex/wiki analysis notes. |

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
| [`AGENT_SYSTEM_MIGRATION_PLAN.md`](AGENT_SYSTEM_MIGRATION_PLAN.md) | Migration plan retained as design context. |
| [`agent-orchestration-migration-plan.md`](agent-orchestration-migration-plan.md) | Detailed orchestration migration plan. |
| [`AGENT_ORCHESTRATION_MOCK_CHECKLIST.md`](AGENT_ORCHESTRATION_MOCK_CHECKLIST.md) | Mock orchestration validation checklist. |
| [`AGENT_ORCHESTRATION_REAL_RUNTIME_PREP.md`](AGENT_ORCHESTRATION_REAL_RUNTIME_PREP.md) | Preparation notes for real runtime validation. |
| [`AGENT_ORCHESTRATION_SCHEDULE_MANUAL_VALIDATION.md`](AGENT_ORCHESTRATION_SCHEDULE_MANUAL_VALIDATION.md) | Manual validation guide for schedule flows. |

## Permissions and tools

| Document | Purpose |
| --- | --- |
| [`PERMISSION_SYSTEM_DESIGN.md`](PERMISSION_SYSTEM_DESIGN.md) | Permission system design. |
| [`PERMISSION_IMPLEMENTATION_CHECKLIST.md`](PERMISSION_IMPLEMENTATION_CHECKLIST.md) | Permission implementation checklist. |
| [`PERMISSION_INTEGRATION_GUIDE.md`](PERMISSION_INTEGRATION_GUIDE.md) | Permission integration guide. |
| [`TOOLS_PARITY.md`](TOOLS_PARITY.md) | Tool parity tracking. |
| [`SKILL_TOOL_PARITY.md`](SKILL_TOOL_PARITY.md) | Skill/tool parity notes. |

## Planning and reference

| Document | Purpose |
| --- | --- |
| [`OMIGA_ENHANCEMENT_PLAN.md`](OMIGA_ENHANCEMENT_PLAN.md) | Product and capability enhancement plan. |
| [`IMPLEMENTATION_GUIDE.md`](IMPLEMENTATION_GUIDE.md) | Implementation guide and templates. |
| [`QUICK_REFERENCE.md`](QUICK_REFERENCE.md) | General developer quick reference. |
| [`PLAN_AGENT_KERNEL_ENG_REVIEW.md`](PLAN_AGENT_KERNEL_ENG_REVIEW.md) | Agent kernel engineering review plan. |

## Documentation policy

Keep documents that are one of:

- release-facing user/operator guidance;
- current design or architecture reference;
- validation instructions that can still be executed;
- migration plans retained as design context.

Remove or archive documents that are only completion announcements, old execution reports, temporary bug patches, or duplicate summaries.
