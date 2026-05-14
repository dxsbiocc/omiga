# First-Time User Guide

Welcome to Omiga — a local-first desktop AI coding agent workbench. This guide gets you from a fresh install to your first working session.

## System requirements

- **macOS 11+**, Windows 10+, or Linux with WebKit/GTK packages for Tauri
- **Bun 1.x** — used for all JavaScript operations (do not use `npm install`)
- **Rust 1.75+** and the Tauri 2 platform prerequisites for your OS (only needed if building from source)
- At least one LLM provider API key, or a local OpenAI-compatible endpoint

You do not need to install Rust to run a pre-built release. You do need it to build from source with `bun run tauri build`.

## Configuration: omiga.yaml

Omiga reads `omiga.yaml` from your project root. The minimum working configuration requires one enabled LLM provider.

Start from the template:

```bash
cp config.example.yaml omiga.yaml
```

Minimum required fields:

```yaml
version: "1.0"
default: "deepseek"         # name of the provider entry below

providers:
  deepseek:
    type: deepseek
    api_key: ${DEEPSEEK_API_KEY}   # read from environment
    model: deepseek-chat
    enabled: true
```

Then export your key before launching:

```bash
export DEEPSEEK_API_KEY="sk-..."
bun run tauri dev
```

For OpenAI:

```yaml
providers:
  openai:
    type: openai
    api_key: ${OPENAI_API_KEY}
    model: gpt-4o
    enabled: true
```

For a local endpoint (Ollama, LM Studio, etc.):

```yaml
providers:
  local:
    type: custom
    api_key: dummy
    base_url: http://localhost:11434/v1/chat/completions
    model: llama3
    enabled: true
```

Configuration is looked up in this order: project root `omiga.yaml` → parent directory → `~/.config/omiga/omiga.yaml` → `~/.omiga/omiga.yaml`.

Never commit API keys. Use environment variables or keep `omiga.yaml` in `.gitignore`.

## First session

1. Launch Omiga (`bun run tauri dev` for development, or open the packaged app).
2. Click **New Session** in the left sidebar.
3. Select a workspace directory — this is the project root Omiga will use for file operations.
4. Type a message in the composer at the bottom and press Enter.
5. Watch the response stream in. Tool calls (file reads, searches) appear inline with their inputs and outputs.

The left panel shows your sessions list. The center panel is the chat transcript. The right panel is the file workspace — it shows the file tree for the selected workspace, open files in the Monaco editor, and any rendered outputs (PDFs, HTML, images).

## Key concepts

**Sessions** are persistent conversation threads tied to a workspace directory. Messages, tool results, memory snapshots, and orchestration events are stored in SQLite at `~/.omiga/omiga.db` (or a project-local path if configured). Sessions survive app restarts.

**Agents** are specialized roles — researcher, analyst, reporter — that the orchestrator can delegate to. Each agent has a card (a Markdown file with YAML front matter) that declares its tools, permissions, and context policy. See `docs/agent-card-spec.md`.

**Tools** are the actions agents can take: `file_read`, `file_write`, `bash`, `web_search`, `web_fetch`, `mcp__*` (MCP server tools), and `operator__*` (operator plugin tools). Each tool call goes through the permission system before execution.

**Skills** are reusable workflows or prompts stored as Markdown files. User-level skills live in `~/.omiga/skills/`; project-level skills live in `.omiga/skills/`. Skills are matched by name and injected into agent context on demand.

**Memory** is multi-layered: working memory (current session), long-term memory (persistent insights), project wiki (structured project knowledge), and source registry (cached web/document summaries). Manage memory in **Settings → Memory**.

## Common first questions

**Where are sessions stored?**
Session data lives in SQLite. The default database path is `~/.omiga/omiga.db`. You can inspect it with any SQLite browser. Never edit it directly while the app is running.

**How do I switch providers mid-session?**
Open **Settings → Providers**, enable another provider, then use the provider selector in the session header to switch. The switch takes effect on the next message. Previous messages are not re-processed.

**Where do operator run artifacts go?**
After a successful operator run, results are exported to `operator-results/<operator_alias>/<run_id>/` inside your session workspace. The run record itself lives at `.omiga/runs/<run_id>/`.

**How do I add a custom model?**
Add a `custom` type provider in `omiga.yaml` with `base_url` pointing to an OpenAI-compatible endpoint. Set `model` to the model ID your server expects.

## Troubleshooting

**Provider not connecting / "failed to fetch" error.**
- Confirm the environment variable is exported in the same shell that launched the app.
- Check that `enabled: true` is set on the provider entry in `omiga.yaml`.
- For custom endpoints, verify the URL is reachable: `curl http://localhost:11434/v1/models`.

**No response from LLM / request hangs.**
- Check `settings.timeout` in `omiga.yaml`. The default is 600 seconds. For slow local models, this is usually enough; for fast cloud APIs, a 30-60 second timeout is more informative.
- Open the Tauri devtools (right-click → Inspect) and check the console for network errors.

**Tool calls are blocked / permission dialogs appear on every call.**
Omiga defaults to asking for confirmation on non-read operations. To reduce dialogs during development, open **Settings → Permissions** and apply the **Development** preset, which auto-approves file operations within the project root.

## Next steps

**Connect an MCP server.** Go to **Settings → MCP** and add a server configuration. MCP tools appear as `mcp__<server>__<tool>` and are available to agents immediately.

**Use operators.** Open **Settings → Plugins**, search the curated catalog, and add an operator plugin. Expose an operator alias and agents can call it as `operator__<alias>`.

**Explore memory.** After a few sessions, review **Settings → Memory** to see what Omiga has stored. You can edit, delete, or promote memory entries.

**Run validation.** To verify your setup end-to-end:

```bash
bun run test          # frontend tests
cargo test --manifest-path src-tauri/Cargo.toml   # Rust tests
./scripts/mock-llm-validation.sh all              # orchestration without a real key
```
