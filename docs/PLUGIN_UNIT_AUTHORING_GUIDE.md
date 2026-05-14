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
environments/<env-id>/conda.yaml | Dockerfile | singularity.def
scripts/<shared helper files>
```

Only `plugin.json` and declared contribution paths are semantic. External
plugins may use different internal layout if their paths are safe and manifest-
declared.

## Versioning, changelog, and marketplace sync

Plugin packages should declare a semantic `version` and may expose a Markdown
changelog:

```json
{
  "name": "ngs-alignment",
  "version": "0.1.0",
  "changelog": "./CHANGELOG.md"
}
```

Omiga copies installed plugins into the user plugin directory. Marketplace
updates are explicit:

- **Sync** applies non-conflicting source changes.
- **Force overwrite** replaces the user plugin copy from the marketplace source
  and discards local edits.
- Local edits are never overwritten by background refresh.

Marketplaces can declare a remote manifest for update checks:

```json
{
  "name": "omiga-curated",
  "remote": {
    "provider": "github",
    "url": "https://raw.githubusercontent.com/org/omiga-plugins/main/marketplace.json",
    "repositoryUrl": "https://github.com/org/omiga-plugins",
    "changelogUrl": "https://github.com/org/omiga-plugins/releases"
  },
  "plugins": []
}
```

A GitHub repository is sufficient for the first marketplace generation when it
contains `marketplace.json`, plugin folders, changelogs, and releases. Use an
independent website/API only when Omiga needs centralized search, ratings,
payments, authentication, telemetry, compatibility matrices, or signed update
channels beyond what a static repository can provide.

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

Profile resolution and standalone checks are side-effect free. Operator
execution may consume the resolved profile to prepare an isolated runtime when
the unit declares `runtime.envRef`:

- conda/mamba/micromamba profiles must use `runtime.condaEnvFile` pointing to
  `conda.yaml` or `conda.yml`; the executor detects `OMIGA_MICROMAMBA`,
  `$HOME/.omiga/bin/micromamba`, `micromamba`, `mamba`, then `conda` in the
  active PATH/base environment/virtual environment.
- Docker profiles use `runtime.image` or a standard `Dockerfile` next to
  `environment.yaml`; local runs can auto-build the image before `docker run`.
- Singularity/Apptainer profiles use `runtime.image` or a standard
  `singularity.def` next to `environment.yaml`; local runs can auto-build a
  cached `.sif`.
- Missing runtime managers produce install guidance instead of falling back to
  the host shell.

Use `environment_profile_check` to validate a declared profile without changing
runtime state. Its output includes `runtimeAvailability`, which reports whether
the current local base/virtual environment can find the required conda manager,
Docker CLI, or Singularity/Apptainer executable and includes install guidance
when missing:

```json
{
  "envRef": "r-bioc",
  "providerPlugin": "operator-pca-r@omiga-curated",
  "runCheck": false
}
```

Use `environment_profile_prepare_plan` when the resolved environment needs a
human-readable preparation checklist. It can write Markdown and JSON under
`.omiga/environments/prepare-plans/`, but remains plan-only.

## Retrieval migration rule

Keep existing retrieval tools stable. New thin API wrappers such as PubMed,
UniProt, or GEO should be added as Operators in an additive path with offline
fixtures and explicit `external_network` semantics before changing routing
defaults.

The V4 PubMed/GEO/UniProt pilots follow this pattern:

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
- add an enabled cache policy, for example
  `cache.enabled: true` and `cache.policyVersion: external-network/v1`
- provide deterministic offline fixtures for tests and parity checks
- expose `mode` with `offline_fixture` plus a `fixture_json` param so
  `unit_authoring_validate` can verify deterministic offline validation support
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

Use `execution_record_detail` for a single run audit. It reads one
ExecutionRecord by id, optionally includes direct children, and parses
`metadataJson`, `runtimeJson`, and `outputSummaryJson` into structured fields
without mutating runs or artifacts.

Use `execution_archive_advisor` after a task finishes to let the agent analyze
recent ExecutionRecords and propose what to archive, fix before archiving,
promote into reusable defaults/examples, inspect as fallback lineage, or clean
after the parent result has been preserved. The advisor is read-only: it does
not delete runs, move artifacts, or auto-register new units.

Use `execution_archive_suggestion_write` when those recommendations should be
persisted for human review. It writes a Markdown report plus JSON snapshot under
`.omiga/execution/archive-suggestions/` and is intentionally non-destructive:
it does not delete, move, or modify recorded artifacts.

Use `learning_self_evolution_report` to turn recent Operator/Template
ExecutionRecord lineage into a review-only crystallization report. It writes
candidate Template, project-preference, and archive follow-ups under
`.omiga/learning/self-evolution-reports/`, but never creates or modifies
Operators, Templates, Skills, defaults, or archives by itself.

Use `learning_self_evolution_draft_write` when a report candidate should become
reviewable scaffold files. Drafts are written under
`.omiga/learning/self-evolution-drafts/` and stay inert until a separate,
reviewed change moves them into real plugin or project configuration paths.

Use `learning_self_evolution_creator` to bootstrap the project Skill
`self-evolution-unit-creator`. That Skill is the dedicated authoring surface for
turning self-evolution evidence into higher-quality Operator or Template drafts:
it chooses the unit kind, asks for deterministic fixtures/validation, and hands
off to the existing promotion gates instead of writing active targets directly.
When called with `unitKind=operator` or `unitKind=template`, the tool can also
seed an inert draft package under `.omiga/learning/self-evolution-drafts/`
containing `candidate.json`, `DRAFT.md`, the relevant manifest `.draft`, and
fixture/script placeholders. The generated manifest drafts follow the current
Operator/Template authoring shape (`interface`, `runtime`, `execution`, and
review metadata) so promotion review starts from a schema-aligned scaffold.
The Draft Browser treats these as creator packages, previews companion
`*.draft` scripts/fixtures/examples separately, and requires companion-file
handling confirmation before the later single-file apply gate can proceed.
When a promotion review artifact is saved, those companion drafts are also
copied as inert `companion-payloads/` with sha256 evidence; this preserves the
full review context even though the apply command remains intentionally
single-file.
For a complete reviewed change, create a multi-file promotion plan from the
saved artifact, assign explicit active target paths for each companion payload,
and save the resulting `MULTI_FILE_PROMOTION_PLAN.md` evidence. Apply those
companion files only through a separate reviewed patch; the built-in apply path
still writes only the confirmed manifest payload.
Use the Draft Browser's copyable reviewed patch guidance as a checklist for the
separate patch: review each companion diff, copy only approved payloads to the
approved targets, then validate the final Operator/Template before changing any
registry/default state.

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
