# Local retrieval plugin protocol

Omiga retrieval plugins are **local application tools**. A plugin runs as a child process and communicates with Omiga over newline-delimited JSON (JSONL) on stdin/stdout. This keeps `search`, `query`, and `fetch` extensible without turning plugin execution into an externally exposed protocol or adding another MCP layer.

A runnable fixture is available at:

```text
src-tauri/fixtures/retrieval-plugins/basic/
```

Use it as the canonical minimal example when authoring or testing a retrieval plugin.

## Boundaries and guarantees

- The plugin process is spawned from the local plugin directory declared in `.omiga-plugin/plugin.json`.
- Omiga sends exactly one request at a time to a child process. `runtime.concurrency` must be `1` for protocol version 1.
- Successful idle child processes may be pooled and reused until `runtime.idleTtlMs` expires.
- Failed, cancelled, timed-out, malformed, disabled, removed, or expired child processes are discarded and are not reused.
- User cancellation is not treated as plugin instability; protocol, timeout, and execution failures can place the route into backoff/quarantine.
- This protocol is internal to the desktop app. Do not rely on it as a public network API.

## Manifest shape

Declare retrieval support under the top-level `retrieval` key in `.omiga-plugin/plugin.json`:

```json
{
  "name": "retrieval-protocol-example",
  "version": "0.1.0",
  "retrieval": {
    "protocolVersion": 1,
    "runtime": {
      "command": "./bin/basic_retrieval_plugin.py",
      "args": [],
      "cwd": ".",
      "idleTtlMs": 30000,
      "requestTimeoutMs": 5000,
      "cancelGraceMs": 500,
      "concurrency": 1
    },
    "sources": [
      {
        "id": "example_dataset",
        "category": "dataset",
        "label": "Example Dataset",
        "description": "Local fixture source that demonstrates search, query, and fetch responses.",
        "aliases": ["example data"],
        "subcategories": ["sample metadata"],
        "capabilities": ["search", "query", "fetch"],
        "requiredCredentialRefs": [],
        "optionalCredentialRefs": ["pubmed_email"],
        "riskLevel": "low",
        "riskNotes": ["Fixture only: does not perform network access or mutate local files."],
        "defaultEnabled": false,
        "replacesBuiltin": false,
        "parameters": []
      }
    ]
  }
}
```

### Runtime fields

| Field | Required | Notes |
| --- | --- | --- |
| `command` | yes | Must start with `./`, resolve inside the plugin root, and point to a file. |
| `args` | no | Static command-line arguments passed to the child process. |
| `env` | no | Static environment additions. Do not put secrets here. Use credential refs. |
| `cwd` | no | `.` or a `./...` path inside the plugin root. Defaults to plugin root. |
| `idleTtlMs` | no | How long a successful idle process can remain pooled. |
| `requestTimeoutMs` | no | Per-execute timeout. Defaults are controlled by Omiga lifecycle policy. |
| `cancelGraceMs` | no | Grace period for shutdown after cancellation. |
| `concurrency` | yes | Must be `1` in protocol version 1. |

### Source fields

| Field | Required | Notes |
| --- | --- | --- |
| `id` | yes | Route source id. Normalized to lowercase snake_case. |
| `category` | yes | Route category, for example `dataset`, `literature`, `knowledge`, or `web`. Dataset aliases such as `data` normalize to `dataset`. |
| `capabilities` | yes | Any non-empty subset of `search`, `query`, `fetch`. |
| `label` / `description` | no | Displayed in Settings and tool descriptions. |
| `aliases` / `subcategories` | no | Extra route hints for tool matching. |
| `requiredCredentialRefs` | no | Allowlisted credential keys that must exist before execution. |
| `optionalCredentialRefs` | no | Allowlisted credential keys projected only if configured. |
| `riskLevel` / `riskNotes` | no | UI and review metadata. |
| `defaultEnabled` | yes | Must be `false` in this version. Users enable plugins explicitly. |
| `replacesBuiltin` | no | Reserved for future controlled replacement behavior. |
| `parameters` | no | JSON metadata for source-specific parameters. |

Allowed credential refs currently include:

```text
tavily_api_key, exa_api_key, firecrawl_api_key, firecrawl_url,
parallel_api_key, semantic_scholar_api_key, pubmed_api_key,
pubmed_email, pubmed_tool_name
```

Omiga projects only the requested refs into the `credentials` object for a request.

## JSONL messages

Every stdin/stdout line is one JSON object. Responses must echo the request `id`.

### Initialize

Omiga sends this immediately after spawning the child process:

```json
{
  "id": "initialize",
  "type": "initialize",
  "protocolVersion": 1,
  "pluginId": "retrieval-protocol-example"
}
```

The plugin must respond with every manifest-declared retrieval source and capability:

```json
{
  "id": "initialize",
  "type": "initialized",
  "protocolVersion": 1,
  "sources": [
    {
      "category": "dataset",
      "id": "example_dataset",
      "capabilities": ["search", "query", "fetch"]
    }
  ]
}
```

Omiga rejects initialization if the protocol version, source ids, categories, or declared capabilities do not match the manifest.

### Execute

`search`, `query`, and `fetch` all use the same execute envelope:

```json
{
  "id": "request-uuid",
  "type": "execute",
  "request": {
    "operation": "search",
    "category": "dataset",
    "source": "example_dataset",
    "query": "BRCA1",
    "maxResults": 5,
    "params": {"organism": "human"},
    "credentials": {"pubmed_email": "dev@example.test"}
  }
}
```

Request fields are shared across operations:

| Field | Search | Query | Fetch | Notes |
| --- | --- | --- | --- | --- |
| `operation` | yes | yes | yes | `search`, `query`, or `fetch`. |
| `category` / `source` | yes | yes | yes | Route selected from the manifest. |
| `query` | usually | usually | optional | Free-text search/query input. |
| `id` | optional | optional | usually | Fetch target identifier. |
| `url` | optional | optional | optional | Fetch URL when the source supports URL fetch. |
| `result` | optional | optional | optional | Prior result object for follow-up fetches. |
| `params` | optional | optional | optional | Source-specific JSON object. |
| `maxResults` | optional | optional | optional | Result count hint. |
| `prompt` | optional | optional | optional | Additional user instruction/context. |
| `credentials` | yes | yes | yes | Only requested allowlisted refs are included. |

### Result

For `search` and `query`, return `items`. For `fetch`, return `detail` and optionally `items`.

```json
{
  "id": "request-uuid",
  "type": "result",
  "response": {
    "ok": true,
    "operation": "search",
    "category": "dataset",
    "source": "example_dataset",
    "effectiveSource": "example_dataset",
    "items": [
      {
        "id": "example-1",
        "accession": "EXAMPLE:1",
        "title": "Example result",
        "url": "https://example.test/datasets/example-1",
        "snippet": "Short preview text.",
        "content": "Longer content, if available.",
        "metadata": {"organism": "human"},
        "raw": {"providerSpecific": true}
      }
    ],
    "total": 1,
    "notes": ["fixture response"],
    "raw": {"protocolFixture": true}
  }
}
```

Omiga validates that the response operation matches the request operation. Unsupported operations or malformed JSON are protocol errors.

### Error

Return an error envelope for expected provider-side failures:

```json
{
  "id": "request-uuid",
  "type": "error",
  "error": {
    "code": "rate_limited",
    "message": "Provider rate limit reached; retry later."
  }
}
```

Use stable, lowercase `code` values. Put user-safe text in `message`; do not include secrets.

### Shutdown

Omiga may ask a child process to shut down before killing it:

```json
{"id":"shutdown","type":"shutdown"}
```

Respond and exit promptly:

```json
{"id":"shutdown","type":"shutdown"}
```

## Authoring checklist

1. Keep plugin execution local and deterministic during development.
2. Make the runtime command executable and keep it under the plugin root.
3. Start with `concurrency: 1`, `defaultEnabled: false`, and a small `requestTimeoutMs`.
4. Implement `initialize`, `execute`, and `shutdown`; ignore blank lines.
5. Echo every request id exactly once.
6. Validate `operation`, `category`, and `source` before doing provider work.
7. Never log or return credential values; only inspect keys you requested.
8. Prefer structured `error` responses for provider errors and reserve crashes for truly unrecoverable bugs.
9. Test repeated `search`/`query`/`fetch` calls and overlapping calls; the host will only reuse idle processes.
10. Check Settings → Plugins for route health, available plugins, and process-pool diagnostics while developing.

## Local validation command

The desktop backend exposes a local validation command for developer tools and future UI hooks:

```ts
await invoke("validate_omiga_retrieval_plugin", {
  pluginRoot: "/absolute/path/to/plugin",
  smoke: true,
});
```

The report includes manifest checks, a retrieval source summary, the protocol doc path, and optional smoke results. When `smoke` is `true`, Omiga starts the plugin without installing it and runs credential-free `search`, `query`, and `fetch` smoke requests for sources that declare those capabilities. Sources with required credentials are skipped for smoke execution instead of projecting secrets.

Validation is local and read-only from Omiga's perspective, but it **does execute the plugin runtime command** when `smoke` is enabled. Only smoke-test plugin roots you trust.
