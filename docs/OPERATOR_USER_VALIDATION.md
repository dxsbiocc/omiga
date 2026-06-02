# Operator System — User Validation Checklist

Hands-on dogfooding plan for the operator/chain/SLURM work landed in PRs #7–#25.
Designed so one person can walk through it in ~30 minutes on macOS.

> All automated checks (Rust, vitest, Playwright) already pass. This document
> covers what only a human at a real keyboard can confirm.

---

## 0. Setup (5 min)

```bash
# 1. Install one real bioinformatics tool so smoke runs have something to call.
brew install seqtk        # ~1 MB, only dependency for the simplest fixture

# 2. From the repo root, start the dev shell.
bun install               # if you haven't already
bun run tauri dev         # opens the Tauri window with hot reload
```

If `tauri dev` errors with a missing Rust target, run `rustup target add aarch64-apple-darwin` (Apple Silicon) or `x86_64-apple-darwin` (Intel) once.

A test FASTQ to use throughout:

```bash
mkdir -p /tmp/omiga-uv && cat > /tmp/omiga-uv/sample.fastq <<'EOF'
@r1
ACGTACGTACGT
+
IIIIIIIIIIII
@r2
TTGGCCAA
+
IIIIIIII
@r3
AAACCCGGGTTT
+
IIIIIIIIIIII
EOF
```

---

## 1. Core async execution (PR #9, #14, #16) — 5 min

Confirms the centerpiece: long tasks no longer freeze the UI.

| Step | Expected |
|------|----------|
| Open **Settings → Plugins → Operators** | Catalog loads, every card shows status + env badge |
| Find `seqtk_sample_reads`, click **Background** | Card immediately shows "Running…" chip; UI stays responsive (try switching tabs) |
| Look at top-right floating tasks button | Badge increments by 1, drawer lists the task with elapsed timer |
| Wait for completion (a few seconds) | Drawer empties; success chip on card; **desktop notification** fires |
| Close the app and re-open while a longer run is mid-flight | The drawer rehydrates the active task (PR #16) |

🚩 **Report a bug if**: UI freezes during run, notification never fires, drawer count gets stuck after completion, the rehydration after relaunch is wrong.

---

## 2. Run history + timeline (PR #8, #19, #22) — 5 min

| Step | Expected |
|------|----------|
| In Settings, scroll to the **Runs timeline** section | Recent runs listed newest-first with status / kind / alias chips |
| Toggle status filter → **Failed** | Only failed runs remain |
| Toggle date range → **Today** | List filters to today |
| Switch view → **Grouped chains** | (Will show chain runs grouped if you did §5 below) |
| Click a row | Run detail dialog opens; **"Open in Finder"** button reveals the run dir |

🚩 **Report if**: pagination breaks, filters silently fail, "Open in Finder" path is wrong.

---

## 3. Cache + Force re-run (PR #8) — 2 min

| Step | Expected |
|------|----------|
| Background-run `seqtk_sample_reads` once successfully | New run dir written |
| Run again with the SAME args | Backend returns cached result; run appears with **"cached" chip** |
| Toggle **Force re-run** switch on the card, run again | Cache bypassed; fresh run dir, status "success" not "cached" |

🚩 **Report if**: Force re-run still serves cache, OR the cache hit reuses stale output.

---

## 4. Favorites (PR #23) — 1 min

| Step | Expected |
|------|----------|
| Click ☆ on `seqtk_sample_reads` | Card moves to the top, star fills |
| Toggle **Favorites only** chip | Catalog shows only pinned operators |
| Reload app | Pin persists (stored in `~/.omiga/operator-favorites.json`) |

🚩 **Report if**: star UI doesn't update, persistence breaks across restarts.

---

## 5. Chain editor + templates + DAG (PR #12, #15, #20, #22, #24) — 8 min

This is the riskiest cluster — never validated against real input until now.

### 5a. Build a linear chain

| Step | Expected |
|------|----------|
| Header → **Open chain editor** | Dialog opens, empty steps |
| Add step 1: `seqtk_sample_reads`, `reads = /tmp/omiga-uv/sample.fastq`, label `s1` | Required-field warning clears |
| Add step 2: `seqtk_sample_reads`, click **Use output from step s1** in the `reads` field | Field auto-fills `{{s1.outputDir}}/...` |
| Click **Run chain** | Both steps execute in order; in timeline they appear as a grouped chain card |

### 5b. Save as template

| Step | Expected |
|------|----------|
| In editor, **Save as template** → name "sample-then-resample" | Template saved (`~/.omiga/user-chains/*.yaml`) |
| Close + reopen editor → **Load template** → pick that one | Steps re-populate identically |

### 5c. DAG (fan-out)

| Step | Expected |
|------|----------|
| Build: step `root` → two children `a` and `b`, both `dependsOn: [root]` | Editor allows multi-select "Depends on"; cycle detection rejects `a → b → a` |
| Run | `a` and `b` start in parallel (check timestamps in run detail) |

🚩 **Report if**: cycle detection lets bad input through, `{{...}}` substitution misses, parallel branches don't actually run concurrently, templates fail to round-trip.

---

## 6. SLURM (PR #11, #17) — skip unless you have HPC access

If you don't have a SLURM cluster reachable over SSH, **skip this section**.

| Step | Expected |
|------|----------|
| Switch execution surface → SSH (your HPC) | Status badge reflects connectivity |
| Run any `placement: [ssh]` operator that scheduler=slurm | Status chip cycles `PD · job N` → `R · job N` → final state |
| If the job fails (e.g. OOM) | Run detail shows **SLURM diagnostics** card with "OOM detected, suggested `--mem=XXG`" |

🚩 **Report if**: queue status never updates, sacct diagnostics fail to surface on real failures.

---

## 7. Cancellation (PR #9) — 2 min

| Step | Expected |
|------|----------|
| Pick any operator likely to take >5 s (or start a chain with 3+ steps) | … |
| In the global tasks drawer, click **Cancel** on the active task | Card chip clears; run history records a "cancelled" entry |

🚩 **Report if**: cancel button has no effect, run continues after cancel, status shows succeeded when it was cancelled.

---

## 8. ProviderSwitcher (PR #13) — 30 seconds

| Step | Expected |
|------|----------|
| In chat composer header, open the provider picker | List scrolls if long, doesn't get clipped |
| If you have two providers with the same model name (e.g. work + personal) | Each row shows model **+** config name secondary text — disambiguated |
| In ChatComposer with a long custom model id | The trigger respects ChatComposer's `maxWidth` (200–260 px) and doesn't push the toolbar |

🚩 **Report if**: scroll is broken, duplicate rows look identical, trigger expands too wide.

---

## How to file bugs found

Each bug, even tiny, captured as a one-paragraph entry in
`docs/OPERATOR_USER_VALIDATION_FINDINGS.md`:

```
### #N — short title
- **Section / Step**: e.g. "§5a step 3"
- **Expected**: …
- **Observed**: …
- **Severity**: blocker / major / minor / polish
- **Reproduce**: 1. 2. 3.
- **Screenshot/log path** (optional)
```

After the walkthrough, that file becomes the next backlog — and the **only**
authoritative driver for further codex dispatch. Any feature work not tied to a
finding from this document should be deferred.

---

## What this does NOT cover (acceptable for v1)

- Multi-user / multi-machine state sync
- Cross-platform: Linux/Windows builds were never tried
- Docker container runtime (`executes_bundled_container_smoke_operator_with_docker_runtime` is ignored in CI; needs a running Docker daemon to validate)
- Network resilience (offline mode, SSH drops mid-run)
- Performance at scale (>100 runs in timeline, >50 operators in catalog)
- i18n (current strings mix EN/CN)

Triage these later only if real-world usage demands.
