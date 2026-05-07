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
- Operator settings UI with cards, run counts, success/failure/smoke statistics, details dialog, failed-run diagnosis, copyable diagnosis payload, run detail/log/verify actions, and smoke-run launcher.
- Built-in validation plugin `operator-smoke@omiga-curated` exposing `write_text_report@0.1.0` and `container_text_report@0.1.0`.

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
  - Result: 21 passed, 1 ignored live Docker smoke
- `cargo test --manifest-path src-tauri/Cargo.toml executes_bundled_container_smoke_operator_with_docker_runtime --lib -- --ignored --nocapture`
  - Result: passed
- `cargo clippy --manifest-path src-tauri/Cargo.toml --lib -- -D warnings`
  - Result: passed
- `./node_modules/.bin/tsc --noEmit`
  - Result: passed
- `bun run test src/components/Settings/PluginsPanel.test.tsx src/state/pluginStore.test.ts`
  - Result: 22 passed
- `git diff --check`
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

1. Live Singularity smoke validation against an installed Singularity/Apptainer runtime.
2. Cache policy using strong input fingerprints and run identity.
3. Richer structured output manifest support beyond `outputs.glob`.
4. Multi-operator workflow/rule composition.
