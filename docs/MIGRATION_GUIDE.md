# Migration Guide: v0.x to v1.0

This guide covers what changed between the v0.x series (last stable: 0.2.0) and v1.0, and how to upgrade safely.

## What changed in v1.0

### Operator plugin system

The largest new surface is the operator plugin system. Operators are declarative execution units (YAML manifests under `operators/<name>/operator.yaml`) that agents invoke as `operator__<alias>` tools. This includes:

- Operator manifest schema (`apiVersion: omiga.ai/operator/v1alpha1`)
- Preflight question dialogs for operator-specific user decisions
- Run provenance stored under `.omiga/runs/<run_id>/` in the active workspace
- Explicit result caching with `cache.enabled: true` in the manifest
- Docker and Singularity container execution paths
- Bundled curated operator plugins (PCA, differential expression, functional enrichment, seqtk sampling)
- Smoke test infrastructure with per-operator validation payloads

If you have no plugins configured, this change is purely additive.

### IDE Bridge

An IDE Bridge connector surface was added to synchronize context between Omiga and external editors. No configuration changes are required unless you opt into IDE Bridge connections through **Settings → Connectors**.

### E2E test suite

A Playwright-based end-to-end test suite was added under `e2e/`. To run it:

```bash
bun run tauri dev &   # app must be running
npx playwright test
```

This does not affect the runtime or your existing sessions.

### Skill fork

Skills now support a fork workflow. User-level skills at `~/.omiga/skills/` can be forked from project-level skills at `.omiga/skills/`. This is a UI feature; the on-disk format is unchanged.

## Database schema: automatic migration

The SQLite database at `~/.omiga/omiga.db` is migrated automatically on first launch. Omiga uses `CREATE TABLE IF NOT EXISTS` and `ALTER TABLE ... ADD COLUMN` migrations that are safe to run on an existing database.

Columns added in recent migrations (all nullable with defaults — no data loss):

| Table | New column | Purpose |
|---|---|---|
| `messages` | `token_usage_json` | Token usage for UI display |
| `messages` | `reasoning_content` | Thinking replay text (Moonshot/Kimi) |
| `messages` | `follow_up_suggestions_json` | LLM-generated follow-up suggestions |
| `messages` | `turn_summary` | Assistant turn recap text |
| `sessions` | `active_provider_entry_name` | Per-session provider override |

You do not need to run any migration commands. Back up `~/.omiga/omiga.db` before upgrading if you have important session history:

```bash
cp ~/.omiga/omiga.db ~/.omiga/omiga.db.backup-$(date +%Y%m%d)
```

## Config file changes

### omiga.yaml

The config schema is backward-compatible. `version: "1.0"` was already the declared schema version in v0.x. No fields have been removed.

New optional `settings` fields (all have defaults if omitted):

```yaml
settings:
  web_search_methods: [tavily, exa, firecrawl, parallel, google, bing, ddg]
  web_use_proxy: true
```

If you have an existing `omiga.yaml` without these fields, the defaults apply. No action required.

### .omiga/permissions.json

The permissions format is unchanged. Existing deny rules continue to work.

### Plugin cache

A one-time legacy plugin cache migration runs automatically on startup (`b354a6d`). If you have an old plugin cache at a non-standard path, it is migrated to the canonical location. No manual action is needed.

## Breaking changes

No API surfaces or behavior changes in v1.0 are breaking for users who have not built integrations against internal Tauri command interfaces. The public-facing changes are:

- The `operator__*` tool namespace is new. If you have custom agent cards with wildcard `tools.forbidden` rules (e.g., `forbidden: ["operator_*"]`), review them to confirm intent.
- Modal and Daytona execution backends explicitly return `not yet implemented` errors. They were non-functional in v0.x as well. If you configured them, expect explicit error messages rather than silent failure.

## Step-by-step upgrade procedure

1. **Back up your database.**
   ```bash
   cp ~/.omiga/omiga.db ~/.omiga/omiga.db.backup-$(date +%Y%m%d)
   ```

2. **Pull the latest code** (or install the new release package).
   ```bash
   git pull
   ```

3. **Install updated dependencies.**
   ```bash
   bun install
   ```

4. **Keep your existing `omiga.yaml`.** No changes required unless you want the new `web_search_methods` ordering.

5. **Launch the app.** The database migration runs automatically on first startup.
   ```bash
   bun run tauri dev
   # or open the packaged release app
   ```

6. **Verify your sessions load correctly.** Open an existing session and confirm messages display as expected.

7. **Run the validation suite** if you want to confirm the full stack.
   ```bash
   bun run test
   cargo test --manifest-path src-tauri/Cargo.toml
   ./scripts/mock-llm-validation.sh all
   ```

If anything looks wrong after launch, restore your backup:

```bash
cp ~/.omiga/omiga.db.backup-$(date +%Y%m%d) ~/.omiga/omiga.db
```

and file an issue with the output of `bun run tauri dev` and the SQLite migration log.
