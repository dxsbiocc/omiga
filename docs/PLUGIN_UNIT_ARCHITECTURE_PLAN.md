# Plugin / Unit Architecture Plan

Date: 2026-05-08

## Executive summary

Omiga should stop treating `plugin` as a synonym for `operator`, `retrieval source`, or any other concrete executable unit.

The target model is:

- **Plugin**: a flexible extension package and distribution boundary.
- **Operator**: an atomic, reproducible execution unit.
- **Template**: a parameterized analysis skeleton.
- **Skill**: an existing task-level workflow/guidance mechanism; do not reimplement it.
- **Agent**: an optional plugin-contributed prompt/role definition, indexed later.
- **Environment**: a reusable runtime profile referenced by executable units.

The project should focus on four engineering tracks:

1. Align plugin manifests around root-level `plugin.json` and Codex-style top-level contribution fields.
2. Add a read-only **Unit Index** for routable Operator / Template / Skill units.
3. Add a Template MVP, with bulk differential expression as the first executable migration target.
4. Add minimal project-scoped `ExecutionRecord` persistence as the foundation for later crystallization, optimization, and emergence.

Avoid introducing a new domain term such as "Capability". Use `plugin contribution` for package contents and `unit` for routable Operator / Template / Skill entries.

## Background and current problems

Current implementation problems:

1. **Plugin concept is too low-level**
   - Current built-ins are named like `operator-differential-expression-r`.
   - This makes a plugin look like one operator instead of an extension package.

2. **Template layer is missing**
   - Current analysis operators such as differential expression, PCA, and enrichment are not purely atomic operators.
   - They contain stable analysis skeletons plus highly variable parameters, plots, thresholds, and summaries.
   - These should move toward Template units.

3. **Retrieval is a parallel subsystem**
   - `retrieval_*` plugins are mostly thin wrappers.
   - Real code is hidden in shared `source_runners`.
   - Long-term, thin data-retrieval APIs should become external-network Operators with versioned cache.

4. **Directory layout is noisy but should not become the main project**
   - Built-ins should be cleaned up to `plugin.json` and `scripts/`.
   - External plugin layout should remain flexible and manifest-driven.

5. **Self-evolution lacks execution data**
   - Existing run/provenance files are useful but not enough for systematic crystallization and optimization.
   - We need a small structured execution record store before building lineage analysis.

## Definitions

### Plugin

A plugin is a flexible extension package. It may contribute any mix of:

- operators
- templates
- skills
- agents
- environments
- MCP servers
- apps/connectors
- hooks
- shared scripts, fixtures, examples, and documentation

Plugin is a provider and distribution boundary, not a routable analysis unit.

### Unit

A unit is a routable task unit in the analysis system.

MVP routable unit kinds:

- `operator`
- `template`
- `skill` reference

Agents and environments are plugin contributions, but they are not task units in the MVP Unit Index.

### Operator

An Operator is an atomic execution unit:

- clear inputs, params, outputs
- deterministic within the same runtime and external data version
- all-or-nothing execution boundary
- runtime binding through `envRef` or existing runtime declaration

Examples:

- `seqtk_sample_reads`
- future `pubmed_fetch`
- future `uniprot_search`

### Template

A Template is a parameterized analysis skeleton:

- logic skeleton is stable
- parameters, groups, thresholds, plot style, and output choices vary
- rendered or instantiated before execution
- uses the same run/provenance/output substrate as operators

Examples:

- bulk differential expression
- functional enrichment
- volcano plot
- boxplot / violin / raincloud statistical visualization

### Skill

Skill remains the existing skill system. This project does not reimplement it.

For this project:

- existing `SKILL.md` loading remains authoritative
- plugin `skills` paths continue to be handled by the existing skill loader
- Unit Index may reference skill metadata for routing and catalog display
- no new skill runtime, DAG engine, or skill execution system is introduced

### Environment

An Environment is a runtime profile contributed by a plugin and referenced by Operator / Template units.

A plugin may contribute many environments. Runtime binding belongs to the executable unit, not to the plugin.

Example:

```yaml
runtime:
  envRef: r-bioc
```

The ref is resolved in provider scope, for example:

```text
omics-r@omiga-curated/environment/r-bioc
```

MVP only indexes environment profiles. It does not auto-create conda envs, install Bioconductor packages, pull containers, or manage HPC modules.

## Design principles

1. **Reference Codex plugin semantics, not Codex naming**
   - Use Codex's idea that plugins contribute skills, MCP servers, apps, hooks, and metadata.
   - Do not introduce `.codex-plugin` as the Omiga canonical path.
   - Omiga uses root-level `plugin.json`.

2. **Plugin layout is convention-based, not prescriptive**
   - `plugin.json` declares contribution paths.
   - Built-in Omiga plugins use `scripts/` as a readability convention.
   - External plugins may use any internal layout if declared paths resolve safely.

3. **No `Capability` domain concept**
   - Use `Plugin`, `Contribution`, `Unit`, `Operator`, `Template`, `Skill`.

4. **Keep existing execution paths working**
   - Do not rewrite operator runtime in the first step.
   - Preserve existing `operator__{alias}` tools.
   - Do not add `capability__*` dynamic tools.

5. **Focus on Operator / Template implementation and self-evolution foundation**
   - Directory cleanup is useful, but not the central architecture.
   - The central architecture is Unit Index + Template execution + ExecutionRecord.

## Plugin manifest model

### Canonical manifest path

New and migrated built-in plugins use:

```text
plugin.json
```

Do not create new hidden metadata folders for built-ins.

### Manifest fields

Use top-level contribution fields, following Codex-style manifest semantics:

```json
{
  "name": "omics-r",
  "version": "0.1.0",
  "description": "Omics analysis units backed by R and Bioconductor.",
  "operators": "./operators",
  "templates": "./templates",
  "skills": "./skills",
  "agents": "./agents",
  "environments": "./environments",
  "mcpServers": "./.mcp.json",
  "apps": "./.app.json",
  "hooks": "./hooks/hooks.json",
  "interface": {
    "displayName": "Omics R",
    "category": "Omics",
    "capabilities": ["Operator", "Template", "R", "Bioconductor"]
  }
}
```

Notes:

- Do not use nested `contributes`.
- Omitted paths are allowed.
- Paths are plugin-root relative and must be safe relative paths.
- The default layout may use `operators/`, `templates/`, `skills/`, `agents/`, `environments/`, `scripts/`, and `shared/`, but only `plugin.json` and declared paths are semantically important.

### Built-in cleanup convention

For built-in plugins only:

- migrate `.omiga-plugin/plugin.json` to `plugin.json`
- migrate `bin/` to `scripts/`
- update `operator.yaml` argv paths accordingly

This is not a hard rule for external plugin authors.

## Unit Index

### Purpose

The Unit Index is a read-only routing and discovery view over installed plugin contributions.

It answers:

- Which Operator / Template / Skill units exist?
- Which plugin provides each unit?
- What category, tags, and stage transitions describe each unit?
- Which units should be exposed to the Agent?
- Which full schema should be loaded after narrowing candidates?

### MVP unit fields

```json
{
  "canonicalId": "omics-r@omiga-curated/template/bulk_de_basic",
  "id": "bulk_de_basic",
  "kind": "template",
  "providerPlugin": "omics-r@omiga-curated",
  "aliases": ["bulk_de", "differential_expression_basic"],
  "classification": {
    "category": "omics/transcriptomics/differential",
    "tags": ["rna", "bulk-rnaseq", "differential", "table", "figure"],
    "stageInput": ["count_matrix"],
    "stageOutput": ["diff_results"]
  },
  "exposure": {
    "exposeToAgent": true
  },
  "sourcePath": "templates/differential-expression-basic/template.yaml",
  "migrationTarget": null,
  "status": "available"
}
```

### Identity rules

- Canonical IDs are provider-scoped and never ambiguous.
- Short aliases are allowed for user and Agent convenience.
- Alias conflicts return multiple candidates and can later be pinned in a user registry.

### Classification rules

Use the document's three-dimensional classification from the start:

- `category`
- `tags`
- `stageInput` / `stageOutput`

Stage inference and automatic two-stage routing are later enhancements. MVP only stores and exposes the metadata.

### Read-only tools

Add read-only tools:

- `unit_list`
- `unit_search`
- `unit_describe`

Do not change existing execution tools in MVP:

- keep `operator__*`
- keep existing skill tooling
- keep existing retrieval tooling during compatibility period

## Template MVP

### TemplateSpec draft

```yaml
apiVersion: omiga.ai/unit/v1alpha1
kind: Template
metadata:
  id: bulk_de_basic
  version: 0.1.0
  name: Bulk Differential Expression
  description: Parameterized bulk RNA-seq differential analysis.
classification:
  category: omics/transcriptomics/differential
  tags: [rna, bulk-rnaseq, differential, table, figure]
  stageInput: [count_matrix]
  stageOutput: [diff_results]
exposure:
  exposeToAgent: true
interface:
  inputs: {}
  params: {}
  outputs: {}
runtime:
  envRef: r-bioc
template:
  engine: jinja2
  entry: ./scripts/differential_expression_basic.R.j2
execution:
  interpreter: Rscript
```

### MVP behavior

Step 1:

- parse TemplateSpec
- validate TemplateSpec
- include templates in Unit Index
- describe templates through `unit_describe`

Step 2:

- render template into generated run script
- execute generated script using the existing operator run substrate where possible
- collect outputs, logs, provenance, and status like an operator run

### First migration target

Migrate differential expression first because it is the clearest example of a Template:

- stable analysis skeleton
- variable grouping/comparison/method/threshold/plot outputs
- currently overgrown as a "basic operator"

Initial migration can keep the old operator available until the template path is validated.

## ExecutionRecord

### Purpose

ExecutionRecord is the minimum data foundation for future self-evolution:

- crystallization: temporary scripts becoming Operator / Template candidates
- optimization: default parameter and execution path suggestions
- emergence: high-frequency combinations becoming Template / Skill candidates

MVP records data first. The current implementation also includes read-only
diagnostic/advisor tools, but it still does not automatically register,
delete, or publish derived units without human approval.

### Storage

Project-scoped SQLite:

```text
.omiga/execution/executions.sqlite
```

Do not store project execution lineage globally under user home.

### Minimal schema

```sql
create table executions (
  id text primary key,
  kind text not null,
  unit_id text,
  canonical_id text,
  provider_plugin text,
  status text not null,
  session_id text,
  parent_execution_id text,
  started_at text,
  ended_at text,
  input_hash text,
  param_hash text,
  output_summary_json text,
  runtime_json text,
  metadata_json text
);
```

MVP write points:

- operator run completion/failure
- template run completion/failure once Template execution exists
- optional skill reference records later, without changing skill runtime

## Retrieval direction

MVP keeps existing retrieval execution for compatibility.

Future direction:

- Thin API wrappers become Operators under `utility/data_retrieval`.
- They declare `sideEffects: [external_network]`.
- They use versioned cache where possible.
- Thick multi-step queries become Templates.
- Full research / literature / knowledge workflows remain Skills.

This is a later migration track, not an MVP blocker.

### Retrieval-to-Operator migration strategy

The migration should be additive and low-risk. Do **not** rewrite the existing
retrieval / built-in tool-function logic as part of the Operator / Template MVP.

Rationale:

- Existing retrieval tools already provide useful behavior and are a stable
  compatibility path for current users.
- API wrapper behavior is network-sensitive and easy to regress through
  seemingly small changes to query normalization, credentials, pagination, rate
  limits, caching, and output formatting.
- The new Operator / Template system should prove itself by running in parallel
  before it replaces existing routes.

Preferred path:

1. Keep current retrieval and built-in tool functions unchanged except for
   compatibility fixes required by `plugin.json` loading.
2. Add new API wrapper Operators for thin public-data-source calls:
   - GEO
   - PubMed
   - UniProt
3. Register those Operators through the Unit Index with clear category/tags such
   as `utility/data_retrieval`, `literature`, `knowledge`, `external_network`.
4. Give each API wrapper Operator an explicit contract:
   - inputs / params / outputs
   - credential refs, if any
   - `sideEffects: [external_network]`
   - rate-limit and timeout expectations
   - cache/version policy where possible
   - structured output files suitable for downstream Templates
5. Route new Agent tasks toward the Operator path once the Unit Index can expose
   them cleanly, while keeping retrieval tools available as fallback.
6. Compare old retrieval outputs and new Operator outputs on a small offline /
   recorded fixture set before changing defaults.
7. Only after parity and stability are demonstrated, consider weakening or
   deprecating the old retrieval-specific paths.

Mapping guidance:

- Thin deterministic API wrappers -> Operator.
- Multi-step query / filter / normalize / summarize flows -> Template.
- Exploratory research workflows, literature review, and judgment-heavy source
  selection -> Skill.

Non-goal:

- Do not collapse existing retrieval functions into the new Operator system in a
  single rewrite. The migration should happen source-by-source and remain
  reversible.

## Phased delivery plan

### Phase 1: Plugin manifest and contribution loading

Deliverables:

- Add root `plugin.json` support as the canonical built-in manifest path.
- Add top-level optional fields:
  - `operators`
  - `templates`
  - `agents`
  - `environments`
  - existing `skills`, `mcpServers`, `apps`, `hooks`, `interface`
- Migrate bundled plugin manifests from `.omiga-plugin/plugin.json` to `plugin.json`.
- Migrate bundled `bin/` directories to `scripts/` and update references.

Acceptance criteria:

- Existing bundled operators still discover and run.
- Existing plugin settings UI still lists built-ins.
- No new hidden plugin metadata directories are introduced.

### Phase 2: Unit Index

Deliverables:

- Add Unit Index data model.
- Index existing operators.
- Index existing skill references through current skill metadata.
- Index read-only template declarations if present.
- Add `unit_list`, `unit_search`, `unit_describe`.
- Add minimal read-only UI under plugin/settings catalog if cheap; otherwise expose backend tools first.

Acceptance criteria:

- Unit Index can list current operators with provider plugin, category/tags/stage where available.
- Existing `operator__*` tool injection is unchanged.
- Search can return a small candidate set by category/tag/stage metadata.

### Phase 3: TemplateSpec read-only support

Deliverables:

- Add TemplateSpec parser and validator.
- Add template source discovery through plugin `templates` path.
- Include templates in Unit Index.
- Mark current DE/PCA/enrichment operators as template migration candidates.

Acceptance criteria:

- Invalid template manifests produce diagnostics.
- `unit_describe` returns full TemplateSpec.
- No template execution is required yet.

First implementation slice:

- backend TemplateSpec parser/validator/discovery through manifest `templates`
  paths
- backend Unit Index model over Operator / Template / Skill entries
- read-only agent tools: `unit_list`, `unit_search`, `unit_describe`
- one bundled differential-expression TemplateSpec that marks the existing
  operator as its migration target

This slice intentionally does **not** execute templates, rewrite existing
operators, or change retrieval internals. It proves the routing/catalog layer
before moving execution into generated template runs.

### Phase 4: Executable Template MVP

Deliverables:

- Render template into generated run script.
- Execute through the existing run workspace / logs / outputs / provenance substrate.
- Migrate bulk differential expression as the first executable Template.
- Keep old operator path available until parity is verified.
- Add `template_execute` as the explicit execution surface for Template units.

Acceptance criteria:

- V1: DE template can execute through `migrationTarget` and therefore preserves
  current operator outputs while the rendered R-template path is validated.
- Later parity gate: rendered DE template produces equivalent output artifacts
  to current DE operator for the same smoke input.
- Template run status, logs, outputs, and provenance are visible through the same run inspection surface.
- Template execution writes ExecutionRecord.

### Phase 5: ExecutionRecord

Deliverables:

- Add project-level SQLite execution store.
- Write records for operator runs.
- Write records for template runs after Phase 4.
- Add a small read/debug command for recent execution records.
- Add `execution_record_list` as a read-only diagnostic tool.

Acceptance criteria:

- Successful and failed operator runs create execution rows.
- Records include unit/provider/status/session/time/hash/output summary fields.
- Existing run behavior is unchanged if record writing fails; record failure is diagnostic, not fatal.

### First-version implementation status: 2026-05-09

Completed in the first version:

- root-level `plugin.json` loading and bundled manifest/script cleanup
- read-only Unit Index over Operator / Template / Skill references
- `unit_list`, `unit_search`, `unit_describe`
- TemplateSpec parsing, validation, discovery, aliases, and diagnostics
- bundled differential-expression / PCA / enrichment TemplateSpecs with
  migration targets:
  - `omics_differential_expression_basic`
  - `omics_pca_matrix`
  - `omics_functional_enrichment_basic`
- `template_execute`
  - delegates migration-target templates to the existing operator runtime
  - supports a minimal rendered-script path for simple local templates
  - reuses operator run workspaces, logs, output collection, provenance, and
    runtime checks instead of creating a parallel runtime
- project-scoped ExecutionRecord SQLite store at
  `.omiga/execution/executions.sqlite`
- best-effort ExecutionRecord writes for successful/failed operator runs and
  template runs
- `execution_record_list` read-only diagnostic tool
- `execution_archive_advisor` read-only advisor tool that classifies recent
  ExecutionRecords into archive, fix, cleanup, lineage-inspection, and reusable
  choice promotion recommendations

Deferred from first version:

- full rendered R implementation for bulk DE, PCA, and enrichment templates
- fixture-based DE parity comparison between rendered Template and legacy
  operator output
- parent/child linking between a Template ExecutionRecord and the delegated
  Operator ExecutionRecord
- environment profile resolver and automatic environment preparation
- retrieval-to-Operator migration for GEO / PubMed / UniProt
- self-evolution graph mining and auto-registration

### Second-version implementation status: 2026-05-09

Completed in the second version:

- ExecutionRecord parent/child lineage:
  - Template execution creates a parent record at start and updates it on
    success/failure.
  - Delegated or rendered backing Operator execution receives
    `runContext.parentExecutionId`.
  - `execution_record_list` can filter by `parentExecutionId` and can include a
    `childrenByParent` diagnostic map.
- Rendered R Template path for bundled analysis workflows:
  - bulk differential expression
  - PCA matrix overview
  - functional enrichment
- Parity-safe fallback:
  - rendered Templates keep `migrationTarget` as the curated baseline.
  - `fallbackToMigrationTarget: true` allows the runtime to fall back to the
    existing Operator result when the rendered path errors or marks itself as an
    error.
  - rendered wrappers call the existing curated R scripts from the plugin root,
    so the first rendered slice is intentionally parity-preserving rather than
    a divergent reimplementation.
- Contract-level parity tests:
  - rendered Template argv mirrors the backing Operator argv after the script
    slot.
  - empty Template interfaces inherit the migration-target Operator interface.
  - rendered templates inject source context such as `template.pluginRoot`.
- Fixture-level parity hardening:
  - PCA has an executable fixture that runs both the rendered Template wrapper
    and the migration-target Operator, then compares structured outputs and key
    TSV artifacts.
  - DE and enrichment stay at contract/wrapper parity until their Bioconductor
    dependencies are available in a deterministic test environment.
- Template-preferred routing and ask/preflight:
  - `template_execute` is described as the preferred high-level workflow surface
    for DE/PCA/enrichment-style analyses.
  - Template execution inherits backing Operator preflight questions where a
    migration target provides them, so the agent can ask focused recommended and
    custom-choice questions before execution instead of guessing analysis
    parameters.
- Lineage diagnostics:
  - ExecutionRecord child filtering and `childrenByParent` output are covered by
    tool-level tests, not only storage tests.
  - `execution_record_list` now also returns a compact `lineageSummary` with
    returned root/child counts, included child counts, status/kind buckets,
    execution-mode buckets such as `renderedTemplate` or
    `fallbackMigrationTarget`, and per-parent child counts.
  - The project-scoped SQLite store has a parent-execution index so
    child-record inspection remains cheap as the local run history grows.

### Third-version implementation status: 2026-05-09

Completed in the third version:

- Unit-level Environment resolver MVP:
  - Added plugin-contributed `Environment` manifests under declared
    `environments` paths.
  - Added environment profile parsing, validation, discovery, and provider-
    scoped resolution for `runtime.envRef`.
  - Template execution records now include environment-resolution diagnostics in
    the Template metadata payload.
  - The resolver is intentionally diagnostic-only: it records requirements,
    check commands, and install hints, but it does not create environments or
    install packages automatically.
- Bundled R environment profiles:
  - differential expression, PCA, and enrichment plugins now declare
    `environments: "./environments"`.
  - each bundled R Template resolves `runtime.envRef: r-bioc` to a
    plugin-local Environment profile with `Rscript` command requirements and
    R/Bioconductor package notes.
- Independent rendered R Template bodies:
  - bulk differential expression
  - PCA matrix overview
  - functional enrichment
  - these rendered Template bodies no longer shell out to the legacy curated
    operator scripts.
  - shared helper sourcing from plugin `scripts/omics_common.R` remains allowed
    because it is a shared library boundary, not the legacy executable entry.
- Parity and contract hardening:
  - PCA fixture parity still compares rendered Template output with the
    migration-target Operator.
  - DE and enrichment retain migration-target fallback for dependency-sensitive
    Bioconductor environments.
  - Template contract snapshot tests now lock version, envRef, fallback,
    migration target, inherited inputs/params/outputs, preflight params, and
    rendered argv shape for the three bundled analysis Templates.
  - Tests assert bundled Template bodies do not reference or shell out to the
    legacy operator scripts.
- Authoring guidance:
  - `docs/PLUGIN_UNIT_AUTHORING_GUIDE.md` documents when to choose Plugin,
    Operator, Template, Skill, and Environment profiles, plus the V3 Template
    migration and retrieval migration rules.

Deferred beyond the third version:

- Full fixture-based numeric/artifact parity for DE and enrichment independent
  rendered bodies once deterministic Bioconductor test environments are
  available.
- Automatic environment preparation for conda/docker/singularity/module/system
  profiles.
- retrieval-to-Operator migration for GEO / PubMed / UniProt.
- self-evolution graph mining and auto-registration.

### Fourth-version implementation status: 2026-05-09

Completed in the fourth version:

- Environment profile validation entrypoint:
  - added `environment_profile_check` as a read-only diagnostic tool.
  - it resolves `runtime.envRef` with provider-plugin disambiguation and can
    optionally run a safe `diagnostics.checkCommand`.
  - V4 checks are intentionally allowlisted version/probe commands only; they do
    not install packages, create conda environments, pull containers, or mutate
    runtime state.
- Authoring validation entrypoint:
  - added `unit_authoring_validate` to validate installed Operator, Template,
    and Environment manifests from one tool call.
  - the output is a compact manifest-health summary for plugin authors and
    future self-evolution review loops.
- Execution lineage report:
  - added `execution_lineage_report` as a higher-level read-only summary over
    project-scoped ExecutionRecords.
  - it reports root/child counts, parent coverage, status/kind buckets,
    execution-mode buckets, fallback counts, and optional per-root child
    summaries.
- Retrieval-as-Operator pilot:
  - added bundled `operator-pubmed-search` as an additive PubMed API wrapper
    Operator.
  - the pilot supports deterministic `offline_fixture` mode and live NCBI
    E-utilities mode with explicit `external_network` permissions.
  - this does not replace the existing PubMed retrieval tool; it proves the
    Operator path in parallel with the compatibility retrieval path.
- Visualization Template library pilot:
  - added bundled `visualization-r` as a Template-first static figure library.
  - it contributes `$visualize-r`, thirteen visual-grammar Templates, a shared R
    helper library, deterministic examples, smoke/index scripts, and a
    preference-template promotion helper.
  - `$visualize` remains a router skill; the plotting implementation lives in
    Template units and editable rendered R scripts, not in a new plotting DSL.
- Flexible plugin/MCP groundwork:
  - added a `computer-use` optional plugin tracer bullet and documented its
    phased implementation plan.
  - plugin-provided stdio MCP servers now resolve relative `cwd` from plugin
    root so bundled sidecars can be packaged without hardcoding project paths.
  - raw `mcp__computer__*` backend tools are hidden from model-visible MCP tool
    discovery and rejected at execution time so they cannot bypass the guarded
    `computer_*` facade policy layer.
  - added the explicit Computer Use `off` / `task` / `session` gate through the
    composer state, request payload, backend request parser, and runtime
    metadata. The gate controls schema injection and execution rejection; task
    scope resets after send, and session scope can be preserved for resumed
    turns.
  - added model-visible `computer_*` facade schemas, backend MCP bridging,
    action policy checks, stop/budget handling, target-window revalidation,
    project-local audit logging, and secret redaction around the mock backend.
  - the current `computer-use` sidecar remains a mock MCP backend for transport
    validation; full run-history browsing and native macOS automation remain
    future work.
- Safety boundaries:
  - the PubMed pilot avoids passing API keys as Operator params/argv. Live mode
    may use `NCBI_API_KEY` from the runtime environment later, but secrets are
    not part of the unit contract.
  - environment checks are diagnostic-only and conservative.

Deferred beyond the fourth version:

- GEO and UniProt retrieval-as-Operator pilots.
- versioned cache policy and recorded live-output fixtures for external-network
  Operators.
- deterministic Bioconductor-backed parity fixtures for DE and enrichment.
- opt-in environment preparation after profile checks are stable.
- Computer Use run-history browser hardening and native macOS backend progression.
- self-evolution reports that propose new Operator / Template candidates from
  ExecutionRecord lineage.

### Focused follow-up status: ask/preflight hardening, 2026-05-09

Current-window focus after V4 is the Operator/Template ask/preflight path, not
Computer Use or visualization Template expansion.

Completed in this focused follow-up:

- Constrained `ask_user_question` and Operator preflight manifests to **1–4
  focused questions** so agents can adapt question count by task without
  falling back to long forms.
- Added an explicit preflight ask state for Operator/Template-backed params:
  callers may omit a preflight param or set it to `ask` / `{"state":"ask"}` /
  `{"status":"ask"}` to force Omiga to collect the user's choice before
  execution.
- Exposed the ask state in generated Operator parameter schemas via a `oneOf`
  branch, while preserving the real value schema for normal execution.
- Kept the bundled differential-expression Operator at four manifest-driven
  decisions: input data type, DE method, FDR threshold, and log2FC threshold;
  these are recommended/customizable analysis choices rather than only dataset
  or grouping questions.
- Added preflight provenance: applied Operator/Template preflight answers now
  carry `metadata.preflight`, run result/provenance JSON includes
  `paramSources`, and ExecutionRecords preserve which params came from
  `user_preflight` versus caller/default/system sources.
- Extended `unit_authoring_validate` with authoring diagnostics that warn when
  an Operator preflight asks only data/grouping questions and omits method,
  threshold, or filtering decisions.
- Documented the authoring rule in `docs/OPERATOR_PLUGIN_MANIFEST.md`.

Next focused improvements:

- Add a small UI/trace affordance that renders `paramSources` and
  `metadata.preflight` in the task/record detail pane.
- Extend GEO/UniProt API-wrapper pilots as Operators without changing existing
  built-in retrieval tool behavior.

### Focused follow-up status: execution archive advisor, 2026-05-09

Completed in this focused follow-up:

- Added `execution_archive_advisor`, a read-only agent-facing tool that scans
  recent project-scoped ExecutionRecords from
  `.omiga/execution/executions.sqlite`.
- The advisor emits actionable recommendations:
  - `archive_result` for successful runs with outputs, run directories, or
    provenance.
  - `fix_before_archive` for failed runs that should be inspected before
    deletion or result packaging.
  - `cleanup_candidate` for successful child runs that can be pruned after the
    parent lineage has been archived.
  - `inspect_lineage` for fallback migration-target paths that should be kept
    together until parity is verified.
  - `promote_reusable_choice` when `paramSources` / `metadata.preflight`
    indicate reusable user-selected analysis choices.
- The advisor intentionally does not mutate the workspace: no deletion, no
  artifact moves, and no automatic Operator/Template registration.

Next focused improvements:

- Add a UI/trace affordance that renders advisor recommendations alongside
  `paramSources` and `metadata.preflight` in the task/record detail pane.
- Add an explicit report-writing command that saves human-reviewable archive
  suggestions under `.omiga/execution/archive-suggestions/` after confirmation.

## Non-goals for MVP

- Do not reimplement skills.
- Do not create a new skill runtime.
- Do not force external plugins into one fixed directory layout.
- Do not rewrite operator runtime.
- Do not rename existing `operator__*` tools.
- Do not implement `capability__*` tools.
- Do not fully migrate retrieval to operators in MVP.
- Do not implement automatic environment creation or package installation.
- Do not implement full LineageGraph, graph mining, or auto-registration.
- Do not automatically apply self-evolution suggestions.

## Follow-up roadmap

After MVP:

1. **Retrieval-as-Operator migration**
   - Use an additive migration, not a rewrite of existing retrieval internals.
   - GEO, PubMed, and UniProt are good first API wrapper Operator candidates.
   - Add `external_network` and versioned cache support.
   - Keep old retrieval tools as compatibility fallback until fixture-based
     parity is demonstrated.

2. **Stage inference**
   - infer data stage from file types and file internals
   - use stage to narrow Unit Index candidates

3. **Two-stage Agent routing**
   - inject a small category/tag/stage index first
   - load full schemas only for 3-8 narrowed candidates

4. **Environment resolver**
   - build on the V3 diagnostic `envRef` resolver
   - support conda/docker/singularity/module/system profiles
   - add explicit, opt-in automatic preparation and package checks
   - keep runtime binding unit-level; do not create a plugin-wide environment

5. **Crystallization reports**
   - group repeated temporary scripts
   - infer parameter slots
   - generate Operator / Template candidates
   - require human approval

6. **LineageGraph**
   - aggregate ExecutionRecords into dependency graph
   - identify common paths, unused outputs, and repeated parameter sets

7. **Optimization and emergence**
   - propose default updates and route variants
   - propose new Template / Skill candidates
   - keep humans responsible for approval and publication

## Testing strategy

Backend:

- manifest path loading tests for `plugin.json`
- safe path resolution tests for top-level contribution fields
- Unit Index indexing/search/describe tests
- TemplateSpec parse/validation tests
- ExecutionRecord SQLite write/read tests
- regression tests proving existing operator discovery still works

Frontend:

- plugin/catalog display tests for new manifest fields
- read-only Unit Index UI tests if UI is included in MVP

Runtime:

- existing operator smoke tests remain green
- rendered Template contract parity tests remain green for DE/PCA/enrichment
- rendered PCA fixture parity test compares actual Template wrapper and
  migration-target Operator outputs
- numeric/artifact fixture parity becomes required once rendered templates stop
  delegating internally to the curated legacy R scripts

Docs:

- update operator/plugin manifest docs after schema is implemented
- include one example flexible plugin layout and one built-in convention layout

## Commit and migration guidance

Keep PRs small:

1. `plugin.json` loader + bundled manifest/script path migration
2. Unit Index read-only model and tools
3. TemplateSpec read-only parser
4. Executable Template substrate + DE migration
5. ExecutionRecord SQLite write path

Do not combine retrieval migration or environment resolver with the MVP PRs.
