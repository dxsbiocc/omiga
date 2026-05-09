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

MVP records data only. It does not implement lineage graph mining or automatic registration.

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
- bundled differential-expression TemplateSpec with `migrationTarget:
  omics_differential_expression_basic`
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

Deferred from first version:

- full rendered R implementation for bulk DE, PCA, and enrichment templates
- fixture-based DE parity comparison between rendered Template and legacy
  operator output
- parent/child linking between a Template ExecutionRecord and the delegated
  Operator ExecutionRecord
- environment profile resolver and automatic environment preparation
- retrieval-to-Operator migration for GEO / PubMed / UniProt
- self-evolution graph mining and auto-registration

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
   - preflight `envRef`
   - support conda/docker/singularity/module/system profiles
   - do not create a plugin-wide environment

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
- DE template smoke parity test after executable Template MVP

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
