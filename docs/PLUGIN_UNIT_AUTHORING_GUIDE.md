# Plugin Unit Authoring Guide

Date: 2026-05-09

This guide captures the V4 authoring rules for Omiga plugins, Operators,
Templates, Skills, and Environment profiles.

## Choose the right abstraction

Use a **Plugin** when you need a package/distribution boundary. A plugin may
contribute operators, templates, skills, agents, environments, scripts, docs,
fixtures, MCP servers, apps, and hooks.

Use an **Operator** when the unit is atomic and reproducible:

- fixed inputs / params / outputs
- one execution boundary
- deterministic given the same runtime and external data version
- suitable for `operator__*` tools

Use a **Template** when the workflow is a parameterized analysis skeleton:

- stable analysis logic
- variable groups, thresholds, plot style, output choices, and summaries
- rendered into an executable script before execution
- suitable for `template_execute`

Use a **Skill** for task-level guidance or judgment-heavy workflows. Do not
reimplement the Skill runtime inside plugins.

## Default plugin layout

The built-in convention is readable but not mandatory for external plugins:

```text
plugin.json
operators/<operator-id>/operator.yaml
templates/<template-id>/template.yaml
templates/<template-id>/template.R.j2
environments/<env-id>/environment.yaml
scripts/<shared helper files>
```

Only `plugin.json` and declared contribution paths are semantic. External
plugins may use different internal layout if their paths are safe and manifest-
declared.

## Template migration pattern

For an analysis currently backed by an Operator:

1. Add a Template with `execution.interpreter` and rendered `template.entry`.
2. Keep `migrationTarget` as the trusted baseline.
3. Set `fallbackToMigrationTarget: true` while parity is being proven.
4. Copy or refactor the analysis body into the Template so it no longer shells
   out to the legacy operator script.
5. Keep shared helper libraries under `scripts/` when they are intentionally
   common code.
6. Add contract snapshots and fixture parity tests before changing defaults.

## Environment profiles

A unit references runtime requirements through `runtime.envRef`:

```yaml
runtime:
  envRef: r-bioc
```

The provider plugin should declare an Environment profile:

```yaml
apiVersion: omiga.ai/environment/v1alpha1
kind: Environment
metadata:
  id: r-bioc
  version: 0.1.0
runtime:
  type: system
  command: Rscript
requirements:
  system: [Rscript]
  rPackages: [DESeq2, edgeR, limma]
diagnostics:
  checkCommand: [Rscript, --version]
  installHint: Install R/Rscript and required Bioconductor packages before running.
```

V4 resolution and checks are diagnostic-only. They record the resolved profile,
requirements, check command, and install hint in Template execution metadata and
can optionally probe a safe version/check command. They do not
install packages, create conda environments, pull containers, or load HPC
modules automatically.

Use `environment_profile_check` to validate a declared profile without changing
runtime state:

```json
{
  "envRef": "r-bioc",
  "providerPlugin": "operator-pca-r@omiga-curated",
  "runCheck": false
}
```

## Retrieval migration rule

Keep existing retrieval tools stable. New thin API wrappers such as PubMed,
UniProt, or GEO should be added as Operators in an additive path with offline
fixtures and explicit `external_network` semantics before changing routing
defaults.

The V4 PubMed pilot follows this pattern:

```text
operator-pubmed-search/
  plugin.json
  operators/pubmed-search/operator.yaml
  scripts/pubmed_search.py
  examples/pubmed_fixture.json
```

Authoring rules for API-wrapper Operators:

- keep the existing retrieval tool as fallback during migration
- declare `permissions.sideEffects: [external_network]`
- provide deterministic offline fixtures for tests and parity checks
- write structured files plus `outputs.json`
- do not pass secrets as params or argv; use credential refs or runtime
  environment variables when credential support is added
- keep live network behavior opt-in and timeout-bounded

## Authoring validation

Run `unit_authoring_validate` after adding or editing plugin contributions. It
checks installed Operator, Template, and Environment manifests and returns a
compact count/diagnostics report:

```json
{
  "includeOk": true
}
```

Use `execution_lineage_report` when validating runtime behavior across
Template/Operator boundaries. It summarizes parent/child ExecutionRecords and
fallback execution modes without reading raw SQLite rows manually.

Use `execution_archive_advisor` after a task finishes to let the agent analyze
recent ExecutionRecords and propose what to archive, fix before archiving,
promote into reusable defaults/examples, inspect as fallback lineage, or clean
after the parent result has been preserved. The advisor is read-only: it does
not delete runs, move artifacts, or auto-register new units.

## Visualization template pattern

For static figures, prefer Template units over ad-hoc plotting Operators.
`visualization-r` is the bundled V4 pattern:

- organize by visual grammar (`scatter`, `distribution`, `bar`, `heatmap`,
  `line`) and use tags for domain presets such as `omics-preset`
- keep each template human-editable: `template.yaml`, `template.R.j2`, and a
  small `example.tsv`
- emit `figure.*`, `plot-script.R`, `rerun.sh`, and optional summaries
- support one-off customization by editing `plot-script.R`
- promote reusable styles only when explicitly requested, without modifying
  built-in templates

Do not invent a JSON-to-plot DSL. Stable inputs and output contracts belong in
`template.yaml`; visual details belong in editable R code.
