# Operator Plugin MVP Completion

Date: 2026-05-07

## Completion status

The first Operator Plugin MVP is implemented and manually validated for the stable single-operator path.

Implemented:

- Plugin-provided `operator.yaml` manifests with `apiVersion: omiga.ai/operator/v1alpha1`.
- User-scoped operator registry at `~/.omiga/operators/registry.json`.
- Session workspace run state at `.omiga/runs/{run_id}` for local execution and the selected remote workspace for SSH/sandbox execution.
- Dynamic Agent tools named `operator__{alias}` plus read-only `operator_list` and `operator_describe`.
- Tauri operator commands with generic names: `list_operators`, `describe_operator`, `set_operator_enabled`, `run_operator`, `list_operator_runs`, `read_operator_run`, `read_operator_run_log`, and `verify_operator_run`.
- Manifest-declared `smokeTests[]` with static validation and UI selection.
- Local/SSH no-container execution, run status, logs, output collection, provenance, and read-only verification.
- Structured failed-run diagnostics: `kind`, `retryable`, `message`, `suggestedAction`, `stdoutTail`, and `stderrTail`.
- Operator settings UI with cards, run counts, success/failure/smoke statistics, details dialog, failed-run diagnosis, copyable diagnosis payload, run detail/log/verify actions, and smoke-run launcher.
- Built-in validation plugin `operator-smoke@omiga-curated` exposing `write_text_report@0.1.0`.

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

## Automated validation evidence

Latest verification run:

- `cargo fmt --manifest-path src-tauri/Cargo.toml --all && cargo test --manifest-path src-tauri/Cargo.toml operators --lib`
  - Result: 13 passed
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

1. Real Docker/Singularity operator execution path.
2. Explicit retry policy for retryable infrastructure failures.
3. Stronger input fingerprinting/checksum cache.
4. Multi-operator workflow/rule composition.
