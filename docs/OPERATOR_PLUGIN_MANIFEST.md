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
```

Supported field kinds include `string`, `integer`, `number`, `boolean`, `enum`, `json`, `file`, `file_array`, `directory`, and `directory_array`.

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

Container selection rules:

- If `none` is declared, direct local/SSH execution prefers no-container unless the manifest explicitly sets `runtime.container.default`, `preferred`, `type`, or `backend` to `docker`/`singularity`.
- If only `docker` or `singularity` is declared, the selected session backend must match, or the manifest must explicitly select that backend.
- Supported image fields are `runtime.container.image`, `runtime.container.images.{docker,singularity}`, `dockerImage`/`docker_image`, and `singularityImage`/`singularity_image`.
- Local/SSH direct container execution bind-mounts the isolated run directory read-write and path-like inputs read-only. Local execution also mounts the project root and plugin root read-only. SSH artifacts, logs, and provenance stay on the remote workspace.
- Sandbox/remote backends remain responsible for their own container isolation; the operator runtime is validated and recorded rather than nested in another container command.

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

## Run storage

- Local runs live under the current session workspace: `.omiga/runs/{run_id}`.
- SSH/sandbox runs live under the selected remote workspace: `.omiga/runs/{run_id}`.
- User registry remains local: `~/.omiga/operators/registry.json`.
- Remote artifacts, logs, and provenance stay remote; results keep references and are read/verified in place.

## Failure diagnostics

Failed runs persist structured diagnostics in `status.json` and, when available, the Agent-facing tool result:

```json
{
  "status": "failed",
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
2. Search for **Operator Smoke Test** and click **Add to Omiga** if it is not installed.
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
