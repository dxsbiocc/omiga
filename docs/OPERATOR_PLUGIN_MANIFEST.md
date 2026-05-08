# Operator Plugin Manifest v1alpha1

Operators are plugin-provided, declarative execution units exposed to agents as `operator__{alias}` tools. A plugin contributes operators by placing `operator.yaml` files under its `operators/` directory.

## Required identity

```yaml
apiVersion: omiga.ai/operator/v1alpha1
kind: Operator
metadata:
  id: write_text_report
  version: 0.1.0
  name: Write Text Report
  description: Write a deterministic text artifact.
```

- `apiVersion` must stay `omiga.ai/operator/v1alpha1`.
- `kind` must be `Operator`.
- `metadata.id` is the stable operator id. Enabled aliases become tool names as `operator__{alias}`.

## Interface schema

Operator tools always accept:

```json
{ "inputs": {}, "params": {}, "resources": {} }
```

Declare those fields in `interface`:

```yaml
interface:
  inputs:
    reads:
      kind: file_array
      required: true
      description: FASTQ files.
  params:
    threads:
      kind: integer
      default: 4
      minimum: 1
      maximum: 32
  outputs:
    report:
      kind: file
      glob: report.html
      required: true
    summary:
      kind: json
      required: true
      description: Small structured summary written to out/outputs.json.
    passed:
      kind: boolean
```

Supported field kinds include `string`, `integer`, `number`, `boolean`, `enum`, `json`, `file`, `file_array`, `directory`, and `directory_array`.

Path-like inputs are resolved as references inside the active session execution environment. The operator runtime does not copy inputs into a separate staging area; the current local workspace or selected remote workspace is already the execution boundary.

## Preflight choices

Use `preflight.questions[]` when an operator needs a bounded user decision before execution. This keeps operator-specific choices in the plugin manifest rather than in the Omiga kernel.

```yaml
preflight:
  questions:
    - id: method
      param: method
      question: Which statistical method should this operator use?
      header: Method
      askWhen:
        missing: true
        empty: true
        values: [auto]
      options:
        - label: Conservative
          value: conservative
          description: Prefer stricter calls with fewer false positives.
        - label: Exploratory
          value: exploratory
          description: Prefer broader calls for early discovery work.
```

- `param` must reference an `interface.params` field.
- `askWhen.missing`, `askWhen.empty`, and `askWhen.values[]` decide when the chat path asks the question.
- `options[].label` is what the user sees; `options[].value` is written back into `params.{param}`.
- Keep defaults in `interface.params` when non-interactive callers should still run without a chat preflight.

## Smoke tests

Use `smokeTests[]` to declare deterministic validation payloads. The UI uses these payloads for generic smoke-run buttons; no project-specific code is needed. If multiple smoke tests are declared, the UI offers a selector and verifies the resulting run after execution.

```yaml
smokeTests:
  - id: default
    name: Write text report smoke
    description: Generates a deterministic two-line report artifact.
    inputs: {}
    params:
      message: hello operator smoke
      repeat: 2
    resources: {}
```

Smoke test payloads are statically validated against the same `inputs`, `params`, `resources`, resource ranges, and bindings used by real operator calls.

When a smoke test is launched from the UI, run provenance/status records:

```json
{
  "runContext": {
    "kind": "smoke",
    "smokeTestId": "default",
    "smokeTestName": "Write text report smoke"
  }
}
```

Regular Agent calls omit this context. Operator cards use it to separate normal calls from smoke validation runs.

## Runtime and resources

Runtime support is declared as composable axes. Operators can run without a container, or through the active session's direct Docker/Singularity backend when the manifest declares that support.

```yaml
runtime:
  placement:
    supported: [local, ssh]
  container:
    supported: [none, docker, singularity]
    images:
      docker: ghcr.io/example/operator:1.0.0
      singularity: docker://ghcr.io/example/operator:1.0.0
  retry:
    maxAttempts: 2
resources:
  cpu:
    default: 1
    min: 1
    max: 8
    exposed: true
  walltime:
    default: 60s
    exposed: true
bindings:
  - param: threads
    resource: cpu
    mode: equal
```

Agents may override only resources marked `exposed: true`.

Retry rules:

- `runtime.retry.maxAttempts` controls infrastructure retries and is clamped to 1-5 attempts; default is 2.
- Retries apply only to retryable infrastructure failures such as execution backend unavailability, transient command dispatch failure, or remote provenance write failure.
- Tool non-zero exit, invalid inputs, runtime mismatch, and output validation failures are not retried; they are returned to the Agent for correction.
- Run status/provenance include `attempt`, `maxAttempts`, and `previousErrors` so the Agent/UI can distinguish a clean first-attempt success from a recovered retry.

Container selection rules:

- If `none` is declared, direct local/SSH execution prefers no-container unless the manifest explicitly sets `runtime.container.default`, `preferred`, `type`, or `backend` to `docker`/`singularity`.
- If only `docker` or `singularity` is declared, the selected session backend must match, or the manifest must explicitly select that backend.
- Supported image fields are `runtime.container.image`, `runtime.container.images.{docker,singularity}`, `dockerImage`/`docker_image`, and `singularityImage`/`singularity_image`.
- Local/SSH direct container execution bind-mounts the isolated run directory read-write and path-like inputs read-only. Local execution also mounts the project root and plugin root read-only. SSH artifacts, logs, and provenance stay on the remote workspace.
- Sandbox/remote backends remain responsible for their own container isolation; the operator runtime is validated and recorded rather than nested in another container command.

The bundled `operator-smoke@omiga-curated` plugin includes `container_text_report@0.1.0` as a live container validation fixture. It declares Docker and Singularity images and exposes a generic smoke payload that runs through the active container backend.

The bundled curated operator plugins keep one atomic operator per plugin (or a tightly scoped validation pair for smoke tests):

- `operator-pca-r@omiga-curated` exposes `omics_pca_matrix@0.1.0` — base-R PCA for expression/count matrices, optional sample metadata grouping, PCA scatter, and scree defaults.
- `operator-differential-expression-r@omiga-curated` exposes `omics_differential_expression_basic@0.1.0` — bulk RNA-seq differential expression over one or more group comparisons, auto-prioritizing DESeq2, edgeR, and limma/voom for counts, limma for quantitative matrices, and Wilcoxon/chi-square/t-test/Welch methods as explicit or fallback tests, with volcano/quadrant/beeswarm default displays.
- `operator-enrichment-r@omiga-curated` exposes `omics_functional_enrichment_basic@0.1.0` — real ORA via `clusterProfiler::enricher` plus ranked GSEA via `fgsea` over GMT or two-column TSV gene sets, with bar/dot/curve default displays.
- `operator-seqtk@omiga-curated` exposes `seqtk_sample_reads@0.1.0` — `seqtk sample` wrapper for FASTQ/FASTA subsampling on the active local/SSH environment.

Manual Docker validation can be run with:

```sh
cargo test --manifest-path src-tauri/Cargo.toml executes_bundled_container_smoke_operator_with_docker_runtime --lib -- --ignored --nocapture
```

## Cache policy

Operator result reuse is explicit opt-in. Add either a top-level cache policy or a runtime cache policy:

```yaml
cache:
  enabled: true
```

or:

```yaml
runtime:
  cache:
    enabled: true
```

Cache keys are scoped to the active execution surface and include operator identity/version/source, the selected local/SSH/sandbox surface, canonical inputs, input fingerprints, effective params/resources, the manifest argv template, and enforcement metadata.

Rules:

- Cache lookup only scans the active session run registry: `.omiga/runs` under the current local workspace, selected SSH workspace, or sandbox workspace.
- Cache never uses a global user-home output store and never copies outputs into a new run directory.
- A cache hit creates a new run record under the active workspace and records `cache.hit=true`, `sourceRunId`, and `sourceRunDir`, while output artifact refs point to the original workspace artifact.
- Cached output refs are verified in place before reuse.
- Smoke runs bypass cache even when the manifest enables cache, so validation always executes the operator command.

## Execution

Execution is structured argv, not an inline shell template. Plugin wrapper files can be referenced relative to the plugin root and are staged into remote run workspaces when executing over SSH/sandbox.

```yaml
execution:
  argv:
    - /bin/sh
    - ./bin/write_text_report.sh
    - ${outdir}
    - ${params.message}
    - ${params.repeat}
```

Available substitutions include `${inputs.name}`, `${params.name}`, `${resources.name}`, `${workdir}`, `${outdir}`, and `${plugin_dir}`. Array inputs used as a whole token expand into multiple argv tokens.

Operators must write durable result artifacts under `${outdir}`. Output globs are relative to `${outdir}`; absolute paths and `..` components are rejected so collected results cannot escape the active session run workspace.

Operators may also write a small structured metadata manifest to `${outdir}/outputs.json`. When present, it must be a JSON object and stay under the same run outdir; it is persisted in provenance as `structuredOutputs` while large files continue to be referenced through declared `outputs.*.glob` artifacts.

Structured output fields are declared in `interface.outputs` without `glob` and with a non-path kind such as `string`, `integer`, `number`, `boolean`, `enum`, or `json`. Required structured output fields must be present in `${outdir}/outputs.json`, and declared values are validated with the same type/bounds/enum rules as inputs and params. Extra metadata keys are allowed for now, but agents should rely only on declared fields.

## Run storage

- Local runs live under the current session workspace: `.omiga/runs/{run_id}`.
- SSH runs live under the selected remote session workspace: `.omiga/runs/{run_id}`. They must not fall back to `~/.omiga/runs`.
- Sandbox runs live under the sandbox workspace: `/workspace/.omiga/runs/{run_id}`.
- User registry remains local: `~/.omiga/operators/registry.json`.
- Remote artifacts, logs, and provenance stay remote; results keep references and are read/verified in place.
- Path-like input fingerprints are persisted in provenance. Local file inputs use `sha256` plus size/mtime; remote file inputs best-effort `sha256sum`/`shasum -a 256` on the selected execution surface and fall back to stat/reference metadata if checksum tooling is unavailable.
- Operator outputs are collected only from `.omiga/runs/{run_id}/out` in the active session workspace or selected remote workspace.
- Structured outputs are read only from `.omiga/runs/{run_id}/out/outputs.json` in that same workspace and are capped at 1 MiB.
- Cache hit records are also written under the active workspace `.omiga/runs/{run_id}` and only reference prior artifacts inside that same execution surface.
- Run cleanup is workspace-scoped. The UI previews global or per-operator candidates, preserves the latest matching runs, and requires confirmation before deleting old/cache run directories under the active `.omiga/runs` root.

## Failure diagnostics

Failed runs persist structured diagnostics in `status.json` and, when available, the Agent-facing tool result:

```json
{
  "status": "failed",
  "attempt": 1,
  "maxAttempts": 2,
  "error": {
    "kind": "tool_exit_nonzero",
    "retryable": false,
    "message": "Operator process exited with code 2.",
    "suggestedAction": "Inspect stdout/stderr, then adjust inputs or params and retry.",
    "stdoutTail": "...",
    "stderrTail": "..."
  }
}
```

The UI surfaces the latest failed status on operator cards/details and exposes a copyable diagnosis payload containing operator identity, run identity, run context, log tails, and suggested action.

## Manually running the bundled smoke test

The built-in validation fixture is:

- Plugin: `operator-smoke@omiga-curated`
- Operator id/alias: `write_text_report`
- Agent tool name after exposure: `operator__write_text_report`
- Smoke test id: `default`
- Manifest: `src-tauri/bundled_plugins/plugins/operator-smoke/operators/write-text-report/operator.yaml`

Recommended manual path:

1. Open **Settings → Plugins**.
2. Search for **Smoke Test** and click **Add to Omiga** if it is not installed.
3. Ensure the plugin is enabled.
4. In the **Operators** section, expose **Write Text Report**.
5. Click **Run Write text report smoke**.
6. Confirm the run detail opens, verification succeeds, and the output artifact `operator-report.txt` is listed.

The UI executes the manifest payload:

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

and records:

```json
{
  "runKind": "smoke",
  "smokeTestId": "default",
  "smokeTestName": "Write text report smoke"
}
```

After exposure, an Agent can also call the same operator payload directly:

```json
{
  "tool": "operator__write_text_report",
  "arguments": {
    "inputs": {},
    "params": {
      "message": "hello operator smoke",
      "repeat": 2
    },
    "resources": {}
  }
}
```

Direct Agent calls validate execution and artifacts, but they are regular operator runs unless the app-level `run_operator` command is invoked with `runKind: "smoke"` and smoke test metadata.
