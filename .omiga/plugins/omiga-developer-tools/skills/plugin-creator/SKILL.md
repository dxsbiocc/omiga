---
name: plugin-creator
description: Create or update Omiga-native plugins under .omiga/plugins, including marketplace entries, Operator units, Template units, plugin-carried Skills, environments, and UI taxonomy alignment.
---

# Omiga Plugin Creator

Use this skill when building Omiga-native plugins, not Codex `.codex-plugin` bundles.

## Core rules

- Preserve atomicity: a plugin may bundle a coherent domain, but each Operator/Template stays independently callable and toggleable.
- Prefer clear product names: plugin `displayName` should be human-facing (`Alignment`, `Transcriptomics`, `NCBI`), not repeated technical prefixes.
- Put taxonomy in plugin metadata: top-level `interface.category`, capabilities, and retrieval/template/operator manifests should drive UI grouping.
- For executable Operators, prefer `runtime.envRef` plus an Environment profile. Keep file conventions strict: conda/mamba/micromamba use `conda.yaml` or `conda.yml`; Docker uses `Dockerfile`; Singularity/Apptainer uses `singularity.def`.
- The executor detects required managers/runtimes in the active PATH after base/virtual-env activation. Missing micromamba/mamba/conda, Docker, or Singularity/Apptainer should produce actionable install guidance instead of silently falling back to the host shell.
- Do not duplicate public Operator and Template versions of the same workflow unless explicitly keeping a migration fallback; prefer the Template version when scripts are meant to be edited/rendered.
- No new dependencies unless requested. Reuse existing shell/Python/R patterns in nearby project plugins.
- Keep scaffolds small, reviewable, and reversible. Replace TODO placeholders before claiming feature completion.

## Taxonomy guidance

- `Analysis`: high-level analysis workflows grouped by omics domain, usually Template-first.
- `Bioinformatics`: sequence-processing and computational biology workflow stages.
  - `NGS`: FASTQ/FASTA/SAM/BAM/CRAM, QC, alignment, quantification, variant-calling preparation.
  - Example: plugin id `ngs-alignment`, display name `Alignment`, category `Bioinformatics`, capability `NGS`.
- `Visualization`: plotting and figure Template bundles.
- `Retrieval` / Source: provider/database access plugins; aggregate same-provider databases when the provider mental model is stronger than single-route cards.
- `Automation` / Operator: generic automation unrelated to a scientific domain.
- `Tools`: model-callable function surfaces or developer/support bundles.

## Workflow

1. Inspect nearby examples first:
   - `.omiga/plugins/*/.omiga-plugin/plugin.json`
   - `.omiga/plugins/operator-seqtk/operators/*/operator.yaml`
   - `.omiga/plugins/visualization-r/templates/*/*/template.yaml`
2. Decide the plugin boundary:
   - Same provider or same workflow stage goes in one plugin.
   - Atomic unit granularity remains at Operator/Template level.
3. Scaffold with the bundled helper when useful:

   ```sh
   python3 .omiga/skills/plugin-creator/scripts/scaffold_omiga_plugin.py \
     --repo . \
     --plugin-id ngs-alignment \
     --display-name Alignment \
     --category Bioinformatics \
     --subcategory NGS \
     --kind operators \
     --operator bwa-index --operator bwa-mem \
     --operator bowtie2-build --operator bowtie2-align \
     --operator star-index --operator star-align \
     --operator hisat2-index --operator hisat2-align \
     --short-description "BWA, Bowtie2, STAR, and HISAT2 read alignment" \
     --with-environments \
     --env-ref ngs-bwa \
     --env-runtime conda \
     --marketplace
   ```

4. Fill in real implementation:
   - Operator manifests: `apiVersion`, `kind`, metadata, inputs/params/outputs, runtime placement, resources, execution argv.
   - Scripts: validate external tools on PATH, write outputs under `${outdir}`, write small `outputs.json` for structured summary.
   - Environments:
     - Always keep the Omiga profile manifest named `environments/<envRef>/environment.yaml`.
     - Conda/mamba/micromamba: `runtime.type: conda`, `condaEnvFile: ./conda.yaml` or `./conda.yml`; put package lists only in that YAML/YML file.
     - Docker: `runtime.type: docker`; either set `runtime.image` or keep a standard `Dockerfile` next to `environment.yaml` for local auto-build.
     - Singularity/Apptainer: `runtime.type: singularity`; either set `runtime.image` or keep `singularity.def` next to `environment.yaml` for local auto-build.
     - For reusable local micromamba, guide users to install the official binary at `$HOME/.omiga/bin/micromamba` or set `OMIGA_MICROMAMBA=/absolute/path/to/micromamba`.
   - Templates: keep `template.yaml` plus source template file; make outputs declared and verifiable.
   - Skills: put `skills/<skill-name>/SKILL.md` in the plugin and optionally mirror it into `.omiga/skills` for project-local immediate use.
5. Update UI taxonomy only when a new top-level or subcategory is needed:
   - `src/components/Settings/PluginsPanel.tsx`
   - `src/components/Settings/PluginsPanel.test.tsx`
6. Verify before reporting:
   - `python3 -m json.tool .omiga/plugins/marketplace.json >/dev/null`
   - Parse or load new manifests with targeted Rust tests when available.
   - `bun run test src/components/Settings/PluginsPanel.test.tsx src/state/pluginStore.test.ts` for UI grouping changes.
   - `bun run build` for frontend/type integration.
   - `git diff --check`.

## Checklist before completion

- Plugin directory exists under `.omiga/plugins/<plugin-id>`.
- `plugin.json` has concise display name, category, capabilities, prompt, and correct contribution paths.
- Marketplace entry exists in `.omiga/plugins/marketplace.json` only if it should be user-installable.
- Each Operator/Template is atomic and has a unique unit id.
- Executable Operators declare `runtime.envRef` when they need tool isolation.
- Environment files follow one convention per runtime: `conda.yaml|conda.yml`, `Dockerfile`, or `singularity.def`; do not invent mixed names such as `requirements.txt` for conda envs or `docker.yaml` for container builds.
- No obsolete duplicate public units remain visible.
- Tests/builds were run or gaps are explicitly reported.

$ARGUMENTS
