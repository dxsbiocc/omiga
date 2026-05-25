# Plugin Developer Guide

This guide covers how to build an Omiga operator plugin — a declarative execution unit that agents can invoke as a tool.

## What plugins can do

Operator plugins wrap deterministic workflows and expose them to agents as structured tools. Common use cases:

- **Analysis workflows** — wrap a script that runs PCA, differential expression, or statistical tests, and return structured outputs agents can reason about.
- **MCP server wrapping** — expose an existing MCP server's capabilities with Omiga-native preflight dialogs and smoke tests.
- **Data retrieval pipelines** — shell out to CLI tools (seqtk, bwa, samtools, etc.) over the active local or SSH workspace.
- **Report generation** — write deterministic text or HTML artifacts to `${outdir}` and declare them as outputs.

Plugins cannot run arbitrary JavaScript in the UI or escalate beyond the user's current execution surface. The operator runtime validates inputs, manages retry, caches deterministic runs, and keeps all artifacts under `.omiga/runs/`.

## Plugin manifest format

Each operator is declared in a YAML file under `operators/<name>/operator.yaml`. The schema requires:

```yaml
apiVersion: omiga.ai/operator/v1alpha1
kind: Operator
metadata:
  id: my_analysis           # stable, kebab/snake identifier
  version: 0.1.0
  name: My Analysis
  description: Runs a short analysis and writes a report.

interface:
  inputs:
    data_file:
      kind: file             # file | file_array | directory | directory_array
      required: true
      description: Input CSV file.
  params:
    method:
      kind: enum
      options: [fast, thorough]
      default: fast
  outputs:
    report:
      kind: file
      glob: report.html      # relative to ${outdir}
      required: true
    passed:
      kind: boolean          # structured field → written to ${outdir}/outputs.json

preflight:
  questions:
    - id: choose_method
      param: method
      question: Which analysis method should be used?
      header: Method
      askWhen:
        missing: true
        values: [auto]
      options:
        - label: Fast
          value: fast
          description: Quick approximate analysis.
        - label: Thorough
          value: thorough
          description: Slower, more precise analysis.

runtime:
  placement:
    supported: [local, ssh]
  container:
    supported: [none, docker]
    images:
      docker: ghcr.io/yourorg/my-analysis:0.1.0
  retry:
    maxAttempts: 2
resources:
  cpu:
    default: 2
    min: 1
    max: 8
    exposed: true

cache:
  enabled: true

execution:
  argv:
    - /bin/sh
    - ./bin/run_analysis.sh
    - ${inputs.data_file}
    - ${params.method}
    - ${outdir}

smokeTests:
  - id: default
    name: Analysis smoke
    description: Runs with a bundled fixture file.
    inputs:
      data_file: test/fixture.csv
    params:
      method: fast
    resources: {}
```

Supported `kind` values for `interface` fields: `string`, `integer`, `number`, `boolean`, `enum`, `json`, `file`, `file_array`, `directory`, `directory_array`.

## Step-by-step: create your first plugin

### 1. Set up the plugin directory layout

```
my-plugin/
├── operators/
│   └── my-analysis/
│       └── operator.yaml    # manifest
├── bin/
│   └── run_analysis.sh      # execution entry point
├── test/
│   └── fixture.csv          # smoke test fixture
└── plugin.json              # plugin metadata
```

`plugin.json` declares the plugin's identity:

```json
{
  "id": "my-analysis@yourorg",
  "name": "My Analysis",
  "version": "0.1.0",
  "description": "Runs my analysis workflow.",
  "operators": ["my_analysis"]
}
```

### 2. Write the execution script

The script receives arguments in the order declared in `execution.argv`. Write durable outputs under `$3` (which maps to `${outdir}`):

```bash
#!/bin/sh
set -e
DATA_FILE="$1"
METHOD="$2"
OUTDIR="$3"

mkdir -p "$OUTDIR"

# do real work here
echo "Analysis complete" > "$OUTDIR/report.html"

# write structured outputs
cat > "$OUTDIR/outputs.json" <<EOF
{"passed": true}
EOF
```

### 3. Validate the manifest locally

```bash
cargo test --manifest-path src-tauri/Cargo.toml operator_manifest -- --nocapture
```

### 4. Run the smoke fixture manually

Reference the legacy test fixture as a working example:

```
src-tauri/bundled_plugins/plugins/operator-smoke/operators/write-text-report/operator.yaml
```

To run the smoke test via the UI:

1. Open **Settings → Plugins**.
2. Find your plugin and click **Add to Omiga**.
3. In the **Operators** section, expose the operator alias.
4. Click **Run [operator name] smoke**.
5. Verify the run detail shows `status: passed` and lists your declared output artifacts.

## Installing and testing locally

Curated marketplace plugins live in the independent `omiga-plugins` repository next to the Omiga app checkout:

```bash
cd ../omiga-plugins
python3 scripts/validate_marketplace.py
```

Omiga reads that repository's `marketplace.json` as the curated marketplace source. Do not add project-local marketplace copies under `.omiga/plugins/` or `src-tauri/omiga-plugins/`.

For MCP server wrapping, no additional `~/.cursor/mcp.json` changes are required. Omiga manages MCP server processes independently of Cursor's configuration.

To verify operator tool registration, enable the plugin and look for the dynamically exposed `operator__my_analysis` tool in the agent tool list.

## Plugin permissions model

Operators run with the permissions of the active user session. The operator runtime enforces:

- All output paths must stay under `${outdir}` (no `..` components).
- Path-like inputs are resolved inside the active session workspace; they cannot reference arbitrary filesystem paths outside the project root.
- Resources marked `exposed: true` (e.g., `cpu`) can be overridden by agents within declared `min`/`max` bounds.
- Agents cannot override resources not marked `exposed`.
- Smoke runs bypass the cache so validation always executes.

High-risk operators (those running arbitrary shell commands) are subject to the same permission gates as the `bash` tool. Surface the expected risk profile in your plugin documentation.

## Publishing to the marketplace

To distribute a curated plugin:

1. Add the plugin directory and `marketplace.json` entry in `../omiga-plugins`.
2. Run `python3 scripts/validate_marketplace.py`.
3. Commit and push the `omiga-plugins` repository so Omiga can refresh from the remote marketplace.

## Common pitfalls

**Output not found after run.** Output `glob` patterns are relative to `${outdir}`. If your script writes to a subdirectory, adjust the glob: `glob: results/report.html`.

**Smoke test runs real data.** The smoke test payload is executed against the active session workspace. Use a small self-contained fixture committed inside the plugin directory, not a path from the user's project.

**Container image not pulled.** When `container.supported` includes `docker`, the active session must have a Docker backend configured. The manifest is validated; the image is not auto-pulled on install.

**Cache reuse across param changes.** The cache key includes `effective params`. Changing a `param` value produces a new run. Changing a param that is not declared in `interface.params` is not tracked — always declare every param.

**Non-zero exit not retried.** Only infrastructure failures are retried (`maxAttempts`). A non-zero script exit is returned to the agent immediately. Implement idempotent retry logic inside the script if needed.
