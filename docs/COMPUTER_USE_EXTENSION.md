# Computer Use extension

Computer Use lets Omiga operate local desktop apps through a guarded
`computer_*` facade. The capability is optional: installing or enabling the
plugin does **not** allow local UI control until the user explicitly enables
Computer Use for a task or session in the chat composer.

## Install and enable

1. Open **Settings → Plugins**.
2. Install and enable the bundled **Computer Use** plugin.
3. In the chat composer, choose **Computer Use → Computer Task** for one
   message or **Computer Use → Computer Session** for repeated local actions.
4. Use **Computer Use → Stop current Computer Use run** to stop the active run.

The settings and audit controls live in **Settings → Computer Use**.

## Backend implementation status

Computer Use is packaged as a bundled optional plugin under:

```text
src-tauri/bundled_plugins/plugins/computer-use/
  plugin.json
  .mcp.json
  bin/computer-use
  bin/computer-use-macos.py
```

The user-facing macOS runtime uses the Python real backend. A Rust sidecar is
kept as an internal experimental feature flag for development, but it is not
exposed in user settings and is not selected by default. Development builds can
inspect or install the internal Rust MCP sidecar with:

```sh
scripts/install-computer-use-sidecar.sh --profile release
```

Use `scripts/install-computer-use-sidecar.sh --status` to inspect source and
destination paths without building or copying; status output includes source
and destination SHA-256 values when those files exist. The script installs
`bin/computer-use-sidecar` inside the bundled plugin, but runtime remains
Python unless an internal developer flag enables Rust. This keeps the core
`computer_*` facade protocol unchanged while the Rust port matures behind a
feature flag.

Settings → Computer Use shows a read-only backend diagnostic summary for the
active Python backend and wrapper. This diagnostic does not probe macOS
permissions, capture screenshots, or start a backend process.

For repeatable validation, see `docs/COMPUTER_USE_QA_MATRIX.md`. The release
check aggregator runs the default low-risk script, formatting, plugin packaging
config, install-status, diff, and mock-smoke checks:

```sh
scripts/computer-use-release-check.py
```

For final packaging, add `--fail-on-generated-artifacts` after cleaning
`__pycache__`/`.pyc` files from bundled plugin resources.

For distribution hardening, the same release check can write and verify a
checksum manifest for Computer Use packaging artifacts:

```sh
scripts/computer-use-release-check.py \
  --write-artifact-manifest /tmp/omiga-computer-use-artifacts.json \
  --verify-artifact-manifest /tmp/omiga-computer-use-artifacts.json \
  --skip-smoke \
  --fail-on-generated-artifacts
```

On macOS packaging machines, run the signing/notarization preflight before a
bundle/sign/upload job:

```sh
scripts/computer-use-release-check.py \
  --include-signing-preflight \
  --skip-smoke \
  --fail-on-generated-artifacts
```

This verifies local `codesign`, `xcrun notarytool`, `xcrun stapler`, Tauri
macOS plist references, and parseable entitlements/Info.plist. Optional
`--codesign-identity` and `--notarytool-profile --verify-notary-profile` flags
can require local signing credentials when a packaging machine is configured.

For targeted MCP smoke checks, run:

```sh
scripts/computer-use-smoke.py --suite mock
```

On a macOS QA machine with Accessibility and Screen Recording already granted,
use `--require-real-observe` to make the real-safe probe fail unless the backend
can actually observe the current target:

```sh
scripts/computer-use-release-check.py --include-real-safe --require-real-observe
```

To prove the positive click/type path against a harmless temporary dialog, run:

```sh
scripts/computer-use-release-check.py --include-real-dialog-e2e
```

To prove the fixed non-text key path against the same harmless temporary
dialog, run:

```sh
scripts/computer-use-release-check.py --include-real-key-e2e
```

To prove the fixed left-button drag path against a temporary TextEdit window,
run:

```sh
scripts/computer-use-release-check.py --include-real-drag-e2e
```

To prove the fixed scroll-wheel path against a temporary long TextEdit
document, run:

```sh
scripts/computer-use-release-check.py --include-real-scroll-e2e
```

To prove the fixed shortcut path against a temporary TextEdit document, run:

```sh
scripts/computer-use-release-check.py --include-real-shortcut-e2e
```

To prove optional native visual OCR observe against a temporary dialog, run:

```sh
scripts/computer-use-release-check.py --include-real-visual-text
```

## Allowed apps and screenshots

**Settings → Computer Use → Allowed apps** is enforced by the core and the
macOS backend. Each entry may be an app name such as `Omiga` or a bundle id
such as `com.omiga.desktop`. If the current target is not in the allowlist,
Computer Use returns `app_not_allowed` and does not perform the action.

`Save observation screenshots` is off by default. When it is off, the backend
still reads target metadata but does not write a screenshot file for
`computer_observe`. The only exception is an explicit OCR observe request:
`extractVisualText=true` captures a temporary screenshot because macOS Vision
needs an image source. OCR remains opt-in and bounded.

## macOS permissions

Phase 8 supports macOS only. The backend may need:

- **Accessibility** — required for clicking, typing, and reading window
  metadata through fixed System Events calls.
- **Screen Recording** — required for screenshots returned by
  `computer_observe` and for optional `extractVisualText=true` OCR observe.

If macOS blocks access, the backend returns a structured error and marks the
target unsafe instead of executing an action.

The scripted real-safe checks intentionally accept that fail-closed state unless
`--require-real-observe` is supplied. Use that stricter flag for final local
permission validation, not for headless/default release checks.

## Safety boundaries

- Models see only Omiga's stable `computer_*` tools.
- Raw backend tools such as `mcp__computer__click` are hidden and rejected.
- Actions require a recent `computer_observe`.
- The observed/validated target must match Settings → Computer Use allowed
  apps.
- Click/type/key/scroll/shortcut actions must include the latest `observationId` and
  `targetWindowId`.
- Omiga revalidates the frontmost target before actions.
- Stop marks the run stopped in core and blocks later actions.
- `computer_type` output and UI cards hide full typed text.
- The backend uses fixed internal macOS commands; it does not run
  model-provided scripts.
- Optional visual text extraction is off by default. When explicitly requested
  with `extractVisualText=true`, the backend captures one screenshot, runs
  native macOS Vision OCR through a fixed helper, returns bounded text boxes,
  and surfaces OCR failures as structured observe fields without executing an
  action.

## Local run records

Audit records are project-local:

```text
<project>/.omiga/computer-use/runs/{sessionId}/{runId}/
  run.json
  actions.jsonl
```

Open **Settings → Computer Use** to see result-record counts and clear local run
records. The UI stays result-first: it does not render saved screenshots,
step-by-step operation flows, or typed content. For audit/debugging, it keeps
compact status signals such as OK, Needs attention, Blocked, and Stopped, plus
the local evidence path so advanced users can inspect files manually when
needed. Secret-like fields are redacted before audit entries are written.

## Current MVP limitations

- macOS only.
- Bounded accessibility metadata: the macOS backend returns real elements from
  the frontmost window up to a fixed depth/count and includes normalized
  semantic hints such as `kind`, `roleDescription`, `parentId`, `depth`,
  `enabled`, `focused`, and `interactable`. Optional OCR returns separate
  bounded `visualText` boxes when `extractVisualText=true`; it is not merged
  into a full semantic UI tree.
- Conservative occlusion handling: a frontmost target mismatch blocks actions.
- Left-click only; `computer_click` exposes only the supported left button, and
  click/click_element revalidates the frontmost target immediately before
  clicking.
- Fixed non-text keys only; `computer_key` supports Enter/Return, Tab, Escape,
  Backspace/Delete, arrows, Page Up/Down, Home/End, and Space through fixed
  key-code snippets after target revalidation.
- Left-button drags only; `computer_drag` requires both start and end
  coordinates inside the validated target window and is intended for simple
  visible drags such as moving a window or selecting visible content.
- Fixed scroll-wheel only; `computer_scroll` supports up/down/left/right at the
  validated target center through CoreGraphics scroll events.
- Fixed shortcuts only; `computer_shortcut` supports `select_all`, `undo`, and
  `redo` through whitelisted snippets after target revalidation.
- `computer_type` prefers direct macOS keystrokes. Controlled clipboard paste is
  only a fallback for ordinary text, and probable secret/token/password inputs
  disable clipboard fallback to avoid clipboard-history exposure.
- Deep AX trees, richer drag-and-drop semantics, visual model image input, and
  Windows/Linux backends are deferred.
