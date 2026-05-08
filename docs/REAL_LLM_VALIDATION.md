# Real LLM Runtime Validation

Last reviewed: 2026-04-25

Omiga can run the next validation step against a real LLM provider loaded from the normal configuration path. This is useful for local/manual acceptance before marking `/schedule`, `/team`, or `/autopilot` as runtime-validated.

CI should still keep a deterministic mock/offline path because real providers require secrets, network access, billing, and provider availability.

For that deterministic path, see `docs/MOCK_LLM_RUNTIME_VALIDATION.md` and
`scripts/mock-llm-validation.sh`.

## Configuration lookup order

The Rust runtime uses `omiga_lib::llm::load_config()`, which loads provider/model settings from the standard config file first and then supplements missing secret values from environment variables.

Config file locations checked by the runtime include:

1. Project root: `omiga.yaml`, `omiga.yml`, `omiga.json`, `omiga.toml`
2. Parent project root when running from `src-tauri`
3. User config directory: `~/.config/omiga/omiga.yaml`, `~/.config/omiga/omiga.yml`, `~/.config/omiga/omiga.json`, `~/.config/omiga/omiga.toml`
4. Legacy Omiga home: `~/.omiga/omiga.yaml`, `~/.omiga/omiga.yml`, `~/.omiga/omiga.json`, `~/.omiga/omiga.toml`

Use `config.example.yaml` as the starting template:

```bash
cp config.example.yaml omiga.yaml
```

Then set the default provider and a provider entry, for example:

```yaml
version: "1.0"
default: "deepseek"

providers:
  deepseek:
    type: deepseek
    api_key: ${DEEPSEEK_API_KEY}
    model: deepseek-chat
    enabled: true

settings:
  max_tokens: 4096
  temperature: 0.7
  timeout: 600
  enable_tools: true
```



The loader also accepts simple dotenv-style `KEY=VALUE` files for local setups such as `~/.omiga/omiga.yaml`:

```bash
DEEPSEEK_API_KEY=sk-...
DEEPSEEK_MODEL=deepseek-v4-flash
```

You may either put the real key directly in a private user config file or keep `${DEEPSEEK_API_KEY}` in the file and export the secret in your shell. Do not commit real keys.

## Run validation

Use the helper script:

```bash
./scripts/real-llm-validation.sh smoke
./scripts/real-llm-validation.sh schedule
./scripts/real-llm-validation.sh team
./scripts/real-llm-validation.sh autopilot
./scripts/real-llm-validation.sh all
```

Recommended sequence:

1. `smoke` — one small provider chat call; proves config/model/network are usable.
2. `schedule` — validates real planner plan construction for `/schedule`-style work.
3. `team` — validates team plan construction and terminal verification task.
4. `autopilot` — validates phased plan construction and reviewer-family augmentation.
5. `all` — run the full real-provider harness when you are ready to spend tokens.

## Existing test targets

The helper script wraps these ignored-by-default tests:

- `src-tauri/tests/real_runtime_smoke.rs`
- `src-tauri/tests/real_schedule_harness.rs`
- `src-tauri/tests/real_team_harness.rs`
- `src-tauri/tests/real_autopilot_harness.rs`

They are ignored by default so normal `cargo test` remains offline and deterministic.

## Pass criteria

- Smoke: provider response contains `ok`.
- Schedule: scheduler returns a multi-step plan.
- Team: scheduler returns a multi-step team plan that includes the terminal team verification task.
- Autopilot: scheduler returns a multi-step phased plan and includes reviewer-family augmentation.

## Failure triage

| Failure | Likely cause | Fix |
| --- | --- | --- |
| `No config file found` | No `omiga.yaml`/user config exists | Copy `config.example.yaml` to `omiga.yaml` or create `~/.config/omiga/omiga.yaml` / `~/.omiga/omiga.yaml` |
| Missing API key | Config references `${VAR}` but env var is unset | Export the referenced env var or put the key in an untracked private config |
| Provider/model HTTP error | Model name or base URL is wrong | Check provider entry in config and run `smoke` again |
| Timeout/network error | Provider unavailable or network blocked | Increase config timeout or use another provider/base URL |
| Plan has only one task | Model did not follow planning prompt well enough | Try a stronger model or reduce request ambiguity |
