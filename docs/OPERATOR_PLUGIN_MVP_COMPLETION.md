# Operator Plugin MVP Completion

Date: 2026-05-07

## Completion status

The first Operator Plugin MVP is implemented and manually validated for the stable single-operator path.

Implemented:

- Plugin-provided `operator.yaml` manifests with `apiVersion: omiga.ai/operator/v1alpha1`.
- User-scoped operator registry at `~/.omiga/operators/registry.json`.
- Session workspace run state at `.omiga/runs/{run_id}` for local execution and the selected remote workspace for SSH execution; SSH runs never fall back to remote `~/.omiga/runs`.
- Dynamic Agent tools named `operator__{alias}` plus read-only `operator_list` and `operator_describe`.
- Tauri operator commands with generic names: `list_operators`, `describe_operator`, `set_operator_enabled`, `run_operator`, `list_operator_runs`, `read_operator_run`, `read_operator_run_log`, and `verify_operator_run`.
- Manifest-declared `smokeTests[]` with static validation and UI selection.
- Local/SSH no-container execution, direct Docker/Singularity command wrapping, run status, logs, output collection, provenance, and read-only verification.
- Structured failed-run diagnostics: `kind`, `retryable`, `message`, `suggestedAction`, `stdoutTail`, and `stderrTail`.
- Explicit retry policy for retryable infrastructure failures with `attempt`, `maxAttempts`, and `previousErrors` recorded in status/provenance and Agent-facing failures.
- Strong path-like input fingerprints: local file inputs persist sha256/size/mtime, and remote file inputs best-effort checksum on the selected execution surface with stat/reference fallback.
- Session-bounded output collection: output globs are relative to the operator `${outdir}`, and absolute or parent-directory output globs are rejected so collected results stay under the active session run workspace.
- Structured output metadata: operators can write a bounded `${outdir}/outputs.json` JSON object that is persisted as `structuredOutputs` in provenance, validated against declared non-artifact `interface.outputs` fields, and summarized in the UI while large files remain artifact refs.
- Explicit opt-in cache policy: cache-enabled operators reuse prior succeeded runs only within the active local/SSH/sandbox workspace `.omiga/runs`, verify cached artifact refs in place, and write cache-hit provenance without copying outputs. Smoke runs bypass cache.
- Cache observability in the operator UI: run summaries, operator cards, detail dialogs, and copyable diagnosis payloads expose cache hit/miss counts plus source run ids/directories.
- Workspace-scoped run cleanup: the settings UI previews old/cache run cleanup globally or for a single operator, preserves the latest matching runs, estimates candidate size, asks for confirmation, and deletes only under the active local/SSH/sandbox `.omiga/runs` root.
- Operator settings UI with cards, run counts, success/failure/smoke statistics, details dialog, failed-run diagnosis, copyable diagnosis payload, run detail/log/verify actions, and smoke-run launcher.
- Plugin catalog cards suppress redundant role suffixes such as `Operator`/`Retrieval Source`; operator cards keep titles focused on the tool/operator name and use Iconify-backed implementation icons for R, C/C++, Python, or shell tools instead of a generic plugin icon.
- Built-in validation plugin `operator-smoke@omiga-curated` exposing `write_text_report@0.1.0` and `container_text_report@0.1.0`.
- Built-in practical atomic operator plugins: `operator-pca-r@omiga-curated`, `operator-differential-expression-r@omiga-curated`, `operator-enrichment-r@omiga-curated`, and `operator-seqtk@omiga-curated`.
- Practical omics operators include default display artifacts while remaining atomic: PCA writes grouped scatter plus scree plots, differential expression prioritizes DESeq2/edgeR/limma with Wilcoxon/chi-square/t-test fallbacks and writes volcano plus comparison-aware quadrant/beeswarm plots, and functional enrichment writes `clusterProfiler` ORA and `fgsea` GSEA bar, dot, and curve plots where applicable.

## Manual validation evidence

Manual local smoke E2E was verified on 2026-05-07:

- Agent/tool path: `operator__write_text_report`
- Smoke test id: `default`
- Payload:

```json
{
  "inputs": {},
  "params": {
    "message": "hello operator smoke",
    "repeat": 2
  },
  "resources": {}
}
```

Observed result:

- Status: `succeeded`
- Location: `local`
- Output artifact: `.omiga/runs/{run_id}/out/operator-report.txt`
- Content:

```text
hello operator smoke
hello operator smoke
```

Manual local Docker smoke E2E was verified on 2026-05-07:

- Agent/tool path: `operator__container_text_report`
- Smoke test id: `default`
- Runtime: Docker Desktop / Docker Engine via direct container wrapping
- Image: `alpine:3.19`
- Observed result:
  - Status: `succeeded`
  - Location: `local`
  - Enforcement container: `docker`
  - Output artifact: `.omiga/runs/{run_id}/out/container-operator-report.txt`
  - Content included `hello container operator smoke` and a `container smoke runtime:` marker.

Manual SSH smoke E2E was verified on 2026-05-07:

- Agent/tool path: `operator__write_text_report`
- Smoke test id: `default`
- Execution environment: SSH remote GPU node
- Selected remote session workspace: `data/query`
- Observed result:
  - Status: `succeeded`
  - Location: `ssh`
  - Run dir: `data/query/.omiga/runs/oprun_20260507091617_16c6dd0f143a4895b53387d37b2f7e9f`
  - Output artifact: `data/query/.omiga/runs/{run_id}/out/operator-report.txt`
  - Run files stayed under the remote workspace (`logs/`, `out/`, `plugin/`, `work/`, `provenance.json`, `status.json`)
  - Content:

```text
hello operator smoke
hello operator smoke
```

## Automated validation evidence

Latest verification run:

- `cargo fmt --manifest-path src-tauri/Cargo.toml --all && cargo test --manifest-path src-tauri/Cargo.toml operators --lib`
  - Result: 28 passed, 1 ignored live Docker smoke
- `cargo clippy --manifest-path src-tauri/Cargo.toml --lib -- -D warnings`
  - Result: blocked by unrelated working-tree connector code (`src/domain/connectors.rs::gmail_token` dead code)
- `cargo clippy --manifest-path src-tauri/Cargo.toml --lib -- -D warnings -A dead-code`
  - Result: passed
- `bunx tsc --noEmit`
  - Result: passed
- `bun run test src/state/pluginStore.test.ts src/components/Settings/PluginsPanel.test.tsx`
  - Result: 24 passed
- `git diff --check`
  - Result: passed
- `Rscript` syntax check plus tiny PCA / differential-expression / enrichment smoke over synthetic TSV/GMT inputs
  - Result: passed
- Naming/path invariants:
  - Operator command names stay generic and do not include the product/project name.
  - Run storage stays under the existing session `.omiga` run directory.
  - User registry storage stays under the existing `.omiga` operator registry.
  - Manifest API remains `omiga.ai/operator/v1alpha1`.

## Recommended operator commit boundary

Include these operator/plugin MVP files:

- `docs/OPERATOR_PLUGIN_GRILL_DECISIONS.md`
- `docs/OPERATOR_PLUGIN_MANIFEST.md`
- `docs/OPERATOR_PLUGIN_MVP_COMPLETION.md`
- `src-tauri/bundled_plugins/marketplace.json`
- `src-tauri/bundled_plugins/plugins/operator-smoke/**`
- `src-tauri/src/commands/operators.rs`
- `src-tauri/src/domain/operators/mod.rs`
- `src-tauri/src/domain/tools/operator_describe.rs`
- `src-tauri/src/lib.rs` operator-command registration hunk only
- `src/state/pluginStore.ts`
- `src/state/pluginStore.test.ts`
- `src/components/Settings/PluginsPanel.tsx`
- `src/components/Settings/PluginsPanel.test.tsx`
- `src/components/Settings/NotebookSettingsTab.tsx` only if committing the current plugin details UI that embeds notebook viewer settings.

Keep separate from the operator MVP commit unless intentionally bundled:

- `src-tauri/src/commands/connectors.rs`
- `src-tauri/src/domain/connectors.rs`
- `src/components/Settings/ConnectorsPanel.tsx`
- `src/components/Settings/ConnectorsPanel.test.ts`
- `src/state/connectorStore.ts`
- `src/state/connectorStore.test.ts`
- `src/utils/connectorPermissionIntent.ts`
- `src/components/Settings/index.tsx`
- `src/components/Settings/openSettingsTabMap.ts`

## Next phase

Recommended follow-up after MVP commit:

1. Shared operator environment profiles and preflight resolver:
   - Do not create one execution environment per operator; R and Python operators default to the same shared session analysis environment.
   - Keep `Operator` as the atomic callable unit, but make execution environments reusable capability profiles.
   - Add manifest support for a shared `envRef` plus concrete requirements such as commands, language runtimes, packages, and optional container images.
   - Resolve in this order: active session environment first, user/shared configured environment second, explicit container fallback last.
   - Keep large environments, images, and run outputs out of user-home plugin/package paths; run outputs remain under the active session workspace `.omiga/runs`.
2. Live Singularity smoke validation against an installed Singularity/Apptainer runtime.
3. Richer custom UI renderers for typed `structuredOutputs` values.
4. Richer retention policy presets and saved cleanup preferences.
5. Multi-operator workflow/rule composition.
