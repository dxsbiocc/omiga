# Operator Plugin MVP — Grill Decisions

Date: 2026-05-06

## Goal

Build an Omiga-native operator system so an Agent can reliably call a single third-party tool as a typed, reproducible, plugin-provided operator.

Non-goal for MVP: full DAG workflow orchestration, Snakemake/Nextflow parity, scheduler integration, or nested operator composition.

## Locked decisions

1. **MVP scope**: stable single-operator invocation by Agent.
2. **Tool exposure**: each enabled operator is exposed as a dynamic tool named `operator__{id}`.
3. **Execution entrypoint**: all operator tools route internally through one `execute_operator(operator_id, args)` path.
4. **Plugin model**: plugins provide declarative `operator.yaml` manifests plus optional wrapper files; no dynamic plugin code is loaded into the Omiga process.
5. **Runtime model**: manifest uses composable `placement + container + scheduler`; first version supports `local/ssh + none/docker/singularity` with `scheduler=none`.
6. **Runtime selection**: tool calls do not include runtime; the active session execution surface determines runtime.
7. **Arguments**: operator tool arguments are always `{ inputs, params, resources }`.
8. **Versioning**: Agent cannot choose versions in tool args; registry resolves the enabled version and run metadata locks it.
9. **Registry**: operator install/enable/version state is user-scoped and read from the local Omiga app/user registry. The current session execution surface decides where the operator runs; SSH execution does not require a separate remote registry for schema exposure.
10. **Plugin distribution**: existing plugins are the distribution layer; `OperatorRegistry` is the runtime capability index.
11. **Gating**: exposed operator requires source plugin enabled, operator enabled, version resolvable, and no unresolved conflict.
12. **Conflicts**: short flat names are used by default; same-id conflicts are not auto-overridden and require explicit source or alias selection.
13. **Manifest schema**: manifests must include `apiVersion: omiga.ai/operator/v1alpha1` and `kind: Operator`, parsed into stable internal `OperatorSpec`.
14. **Rich schema**: manifest uses Omiga-specific rich input/param/output/resource schema; Agent/UI receive generated JSON Schema.
15. **Inputs**: Agent may pass plain path strings; executor canonicalizes to `FileRef`/`ArtifactRef`.
16. **Input staging**: do not copy operator inputs. Inputs are resolved as references inside the active session execution environment; operators write any derived files under the session run workspace.
17. **Outputs**: MVP collects outputs mainly with `outputs.glob`; optional `outputs.json` is reserved for structured output manifests.
18. **Workspace**: every run uses an isolated workspace under the current session workspace. Local run dirs live under `<session-workspace>/.omiga/runs/{run_id}`; SSH run dirs live under `<remote-session-workspace>/.omiga/runs/{run_id}`, never under remote `~/.omiga/runs`.
19. **SSH artifacts**: SSH outputs/logs/provenance remain remote; local results contain remote references only.
20. **SSH registry/runtime split**: SSH runs use local registry/schema resolution, then execute on the selected remote session environment. Plugin wrapper files referenced by `argv` are staged into the remote run workspace as execution support files; generated artifacts remain remote.
21. **Resources**: manifest defines defaults, ranges, and `exposed`; Agent can override only exposed resources.
22. **Bindings**: MVP supports simple `param <-> resource` equal bindings such as `threads == cpu`; no general expression language.
23. **Command execution**: MVP supports structured `argv` plus plugin wrappers; no arbitrary inline shell templates.
24. **Fingerprinting**: path-like inputs use strong sha256 fingerprints where available; no cache is enabled until cache policy is explicit.
25. **Retries**: MVP retries only execution-infrastructure failures. Tool non-zero exit, invalid inputs, and output validation failures are returned for Agent correction.
26. **Errors**: operator results are structured JSON. Failures include at least `kind`, `retryable`, and `message`, plus field/run/log/action context where possible.
27. **Permissions**: manifest declares permissions. Docker/Singularity enforce boundaries where possible; local/SSH no-container are best-effort/trusted and record enforcement level.
28. **Discovery**: installed operators are not automatically exposed; only enabled operators become dynamic tools.
29. **Meta tools**: MVP should provide read-only `operator_list` and `operator_describe`; Agent cannot enable/install operators.
30. **Catalog source**: catalog/schema source is the local OperatorRegistry. Runtime placement follows the active session execution surface, so the same locally enabled `operator__{id}` can execute on the selected SSH server when the operator manifest supports SSH/no-container.
31. **Tool integration**: operators are Omiga first-class dynamic tools, not MCP-emulated tools.
32. **Nested workflows**: MVP operators are single-step execution units; no manifest-level nested operators/steps/workflows.
33. **Smoke tests**: every operator should include a smoke test manifest. Install does static validation only; users/CI can run tests separately.
34. **Run state**: MVP uses a persisted run state machine even if tool calls are initially synchronous.
35. **Call mode**: `operator__id` synchronously waits until success/failure/timeout for MVP.
36. **Timeouts**: effective timeout comes from manifest walltime, allowed override, and session/global hard limit; infra/run/collection timeouts are distinct.
37. **Run history**: local run history is discovered from `{project}/.omiga/runs/*/status.json|provenance.json`; SSH/sandbox run provenance remains on the selected remote execution environment and is accessed through remote file tooling rather than copied locally.
38. **Run verification**: first-version run QA is read-only and checks run state, status, logs, and declared output artifact references in-place on the selected execution surface.
39. **Smoke tests**: operator manifests may declare `smokeTests[]` with typed `{ inputs, params, resources }` invocation payloads; the UI uses those declarations instead of project-specific hardcoded smoke runners.

## MVP implementation slice

Recommended first vertical slice:

1. Add `domain::operators` with manifest parsing, registry discovery, schema generation, run state, and local execution.
2. Add read-only `operator_list` and `operator_describe` built-in tools.
3. Add dynamic `operator__*` schema assembly for enabled operators.
4. Add `operator__*` dispatch before MCP/static tool dispatch.
5. Implement local/no-container execution first, with isolated run dirs and structured results.
6. Extend to SSH/no-container by using the local registry/schema and reusing the existing session execution environment and remote file access primitives.
7. Add local/SSH Docker/Singularity command wrapping once no-container behavior is stable.

## Built-in validation fixture

- Bundled plugin: `operator-smoke@omiga-curated`.
- Bundled operator: `write_text_report@0.1.0`, exposed as `operator__write_text_report` after installation + enablement.
- Purpose: deterministic end-to-end validation for plugin discovery, registry exposure, plugin wrapper staging, single operator invocation, run dirs, logs, provenance, and required output collection.
- Runtime support: `local+none` and `ssh+none`; registry/schema remain local, while SSH run artifacts remain on the selected remote server.
- Manifest authoring reference: [`docs/OPERATOR_PLUGIN_MANIFEST.md`](./OPERATOR_PLUGIN_MANIFEST.md).

## Current first-version completion notes

- Operator run listing, details, logs, and verification are execution-surface aware: local reads use `{session-workspace}/.omiga/runs`, while SSH/sandbox reads use the active remote workspace `.omiga/runs` through the existing execution environment.
- Remote operator artifacts/logs/provenance are never copied into the local registry or workspace; UI and Agent-facing results keep remote references and read/verify them in place.
- Smoke runs are now manifest-driven via `smokeTests`, so user-added and built-in plugins can expose deterministic validation payloads through the same generic operator runner. When multiple smoke tests are declared, the UI lets the user choose which payload to run and then opens/verifies the resulting run.
- Operator cards summarize calls/successes/failures/latest status from the current execution surface run history; clicking a card opens an operator detail view with manifest identity, aliases, smoke tests, and matching run statuses.
- Smoke run provenance/status now records `runContext.kind=smoke` plus smoke test id/name, so cards can distinguish normal calls from smoke validation runs, including failed smoke runs that only have `status.json`.
- Failed runs now surface structured diagnostics (`kind`, `retryable`, `message`, `suggestedAction`, stdout/stderr tails) in run summaries and details so the user or Agent can inspect/copy a concrete correction payload without moving remote artifacts locally.
- Local/SSH Docker/Singularity runtimes now use direct command wrapping when the manifest and active session backend explicitly select a container. The run directory is writable, path-like inputs are read-only bind mounts, and sandbox backends avoid nested container wrapping.
- Bundled container smoke coverage now includes `container_text_report@0.1.0` with Docker/Singularity runtime declarations, a generic active-backend smoke payload, and a live ignored Docker test for manual validation against the installed container runtime.
- Retry policy now retries only retryable infrastructure failures and records `attempt`, `maxAttempts`, and `previousErrors`; tool exits, validation failures, and output failures remain Agent-correction errors.
- Path-like input provenance now records strong local sha256 fingerprints and best-effort remote sha256 fingerprints, with stat/reference fallback when remote checksum tools are unavailable.
- Output collection is bounded to the active session run workspace: manifests declare relative output globs under `${outdir}`, and absolute or parent-directory output globs are rejected.
- Manual local smoke E2E was verified on 2026-05-07 with `operator__write_text_report`, producing `.omiga/runs/{run_id}/out/operator-report.txt` containing two `hello operator smoke` lines.
- Manual local Docker smoke E2E was verified on 2026-05-07 with `operator__container_text_report`, producing `.omiga/runs/{run_id}/out/container-operator-report.txt` from the `alpine:3.19` image.
- Regression coverage now locks smoke UI visibility, smoke-test selection fallback, store-level smoke run context propagation, failed-run diagnostic preservation, command project-root normalization, invalid smoke test ids, and bundled smoke execution.
