# Plugin Unit Authoring Guide

Date: 2026-05-09

This guide captures the V3 authoring rules for Omiga plugins, Operators,
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

V3 resolution is diagnostic-only. It records the resolved profile, requirements,
check command, and install hint in Template execution metadata. It does not
install packages, create conda environments, pull containers, or load HPC
modules automatically.

## Retrieval migration rule

Keep existing retrieval tools stable. New thin API wrappers such as PubMed,
UniProt, or GEO should be added as Operators in an additive path with offline
fixtures and explicit `external_network` semantics before changing routing
defaults.
