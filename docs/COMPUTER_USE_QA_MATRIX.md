# Computer Use QA matrix

This matrix keeps Computer Use verification repeatable across the user-facing
Python backend, optional internal Rust sidecar checks, mock mode, real macOS
safe-failure mode, and UI/core policy surfaces.

## Quick smoke commands

Default Python mock smoke:

```sh
scripts/computer-use-smoke.py --suite python-mock
```

The internal Rust sidecar remains behind a developer feature flag and is not
exposed in user settings. If validating it, build or provide a Rust sidecar
binary first:

```sh
cargo build --manifest-path src-tauri/Cargo.toml --bin computer-use-sidecar
scripts/computer-use-smoke.py --suite rust-mock --rust-bin /path/to/computer-use-sidecar
```

Safe real-mode probes. These may require macOS Accessibility for observe, but
they intentionally use invalid targets or out-of-bounds coordinates so they do
not click or type:

```sh
scripts/computer-use-smoke.py --suite python-real-safe
# Internal Rust parity only:
scripts/computer-use-smoke.py --suite rust-real-safe --rust-bin /path/to/computer-use-sidecar
```

By default, real-safe probes pass when the backend fails closed before observe
because permissions are unavailable. On a QA/packaging machine where
Accessibility and Screen Recording have already been granted, require a real
observation with:

```sh
scripts/computer-use-smoke.py --suite python-real-safe --require-real-observe
scripts/computer-use-release-check.py --include-real-safe --require-real-observe
```

Run all scriptable safe probes, including internal Rust parity checks:

```sh
scripts/computer-use-smoke.py --suite all-safe --rust-bin /path/to/computer-use-sidecar
```

Controlled positive macOS E2E. This opens a temporary AppleScript dialog,
types a unique token, clicks the observed OK button, and verifies the returned
dialog text. It is side-effectful only inside the temporary dialog and requires
Accessibility permission:

```sh
scripts/computer-use-smoke.py --suite python-real-dialog-e2e
# Internal Rust parity only:
scripts/computer-use-smoke.py --suite rust-real-dialog-e2e --rust-bin /path/to/computer-use-sidecar
```

Controlled key-press E2E. This opens the same temporary dialog, presses Enter
through the backend `key_press` tool, and verifies that the default OK button is
submitted by that key press:

```sh
scripts/computer-use-smoke.py --suite python-real-key-e2e
# Internal Rust parity only:
scripts/computer-use-smoke.py --suite rust-real-key-e2e --rust-bin /path/to/computer-use-sidecar
```

Controlled drag E2E. This opens a temporary TextEdit document, sets the window
bounds, drags the title area through the backend `drag` tool, and verifies that
the window moved:

```sh
scripts/computer-use-smoke.py --suite python-real-drag-e2e
# Internal Rust parity only:
scripts/computer-use-smoke.py --suite rust-real-drag-e2e --rust-bin /path/to/computer-use-sidecar
```

Controlled scroll E2E. This opens a temporary long TextEdit document, scrolls
down through the backend `scroll` tool, and verifies that the vertical scroll
indicator moved:

```sh
scripts/computer-use-smoke.py --suite python-real-scroll-e2e
# Internal Rust parity only:
scripts/computer-use-smoke.py --suite rust-real-scroll-e2e --rust-bin /path/to/computer-use-sidecar
```

Controlled shortcut E2E. This opens a temporary TextEdit document, runs the
fixed `select_all` shortcut through the backend `shortcut` tool, types a unique
replacement token, and verifies that the document changed:

```sh
scripts/computer-use-smoke.py --suite python-real-shortcut-e2e
# Internal Rust parity only:
scripts/computer-use-smoke.py --suite rust-real-shortcut-e2e --rust-bin /path/to/computer-use-sidecar
```

Controlled visual OCR E2E. This opens a temporary dialog containing known
visible text, calls `observe` with `extractVisualText=true`, and verifies that
native macOS Vision OCR returns bounded text boxes containing the target words:

```sh
scripts/computer-use-smoke.py --suite python-real-visual-text
# Internal Rust parity only:
scripts/computer-use-smoke.py --suite rust-real-visual-text --rust-bin /path/to/computer-use-sidecar
```

If the Rust binary is not in the default Cargo target path:

```sh
scripts/computer-use-smoke.py --suite rust-mock --rust-bin /path/to/computer-use-sidecar
```

Aggregate the default low-risk release gate:

```sh
scripts/computer-use-release-check.py
```

The aggregator outputs JSON by default and can produce a Markdown artifact:

```sh
scripts/computer-use-release-check.py \
  --format markdown \
  --output /tmp/computer-use-release-check.md
```

When validating an installed internal Rust feature artifact, compare the bundled
binary with the source binary and run the installed sidecar in mock mode:

```sh
scripts/computer-use-release-check.py \
  --include-rust-sidecar \
  --rust-bin /path/to/computer-use-sidecar \
  --verify-installed-sidecar
```

For distribution hardening, generate and immediately verify a checksum manifest
for Computer Use packaging artifacts:

```sh
scripts/computer-use-release-check.py \
  --write-artifact-manifest /tmp/omiga-computer-use-artifacts.json \
  --verify-artifact-manifest /tmp/omiga-computer-use-artifacts.json \
  --skip-smoke \
  --fail-on-generated-artifacts
```

On a macOS packaging machine, check signing/notarization prerequisites before
running a bundle/sign/upload job:

```sh
scripts/computer-use-release-check.py \
  --include-signing-preflight \
  --skip-smoke \
  --fail-on-generated-artifacts
```

If a local codesign identity or notarytool keychain profile should be required,
add `--codesign-identity "Developer ID Application: ..."` and, for an explicit
credential check, `--notarytool-profile PROFILE --verify-notary-profile`.

## Scripted coverage

| Suite | Backend mode | Side effects | Expected coverage |
| --- | --- | --- | --- |
| `python-mock` | `OMIGA_COMPUTER_USE_BACKEND=mock` | None | Same deterministic semantics against Python sidecar |
| `python-real-safe` | `OMIGA_COMPUTER_USE_BACKEND=real` | Observe only; action calls are validation failures | Same safe-failure semantics against Python real backend; add `--require-real-observe` when permissions must be proven |
| `python-real-dialog-e2e` | `OMIGA_COMPUTER_USE_BACKEND=real` | Opens/closes a temporary dialog, types a unique token, clicks OK | Positive user-facing macOS click_element + type_text proof |
| `python-real-key-e2e` | `OMIGA_COMPUTER_USE_BACKEND=real` | Opens/submits a temporary dialog, presses Enter | Positive user-facing macOS key_press proof |
| `python-real-drag-e2e` | `OMIGA_COMPUTER_USE_BACKEND=real` | Opens/closes a temporary TextEdit document and moves its window by dragging the title area | Positive user-facing macOS drag proof |
| `python-real-scroll-e2e` | `OMIGA_COMPUTER_USE_BACKEND=real` | Opens/closes a temporary TextEdit document, scrolls down | Positive user-facing macOS scroll-wheel proof |
| `python-real-shortcut-e2e` | `OMIGA_COMPUTER_USE_BACKEND=real` | Opens/closes a temporary TextEdit document, replaces its content through select_all + type_text | Positive user-facing macOS fixed-shortcut proof |
| `python-real-visual-text` | `OMIGA_COMPUTER_USE_BACKEND=real` | Opens/closes a temporary dialog, captures a screenshot for OCR | Positive user-facing macOS Vision OCR observe proof |
| `rust-mock` | `OMIGA_COMPUTER_USE_BACKEND=mock` | None | Internal feature-flagged sidecar parity check, not user-facing |
| `rust-real-safe` | `OMIGA_COMPUTER_USE_BACKEND=real` | Observe only; action calls are validation failures | Internal feature-flagged sidecar safe-failure parity check; add `--require-real-observe` when permissions must be proven |
| `rust-real-dialog-e2e` | `OMIGA_COMPUTER_USE_BACKEND=real` | Opens/closes a temporary dialog, types a unique token, clicks OK | Internal Rust parity proof for positive click_element + type_text |
| `rust-real-key-e2e` | `OMIGA_COMPUTER_USE_BACKEND=real` | Opens/submits a temporary dialog, presses Enter | Internal Rust parity proof for positive key_press |
| `rust-real-drag-e2e` | `OMIGA_COMPUTER_USE_BACKEND=real` | Opens/closes a temporary TextEdit document and moves its window by dragging the title area | Internal Rust parity proof for left-button drags |
| `rust-real-scroll-e2e` | `OMIGA_COMPUTER_USE_BACKEND=real` | Opens/closes a temporary TextEdit document, scrolls down | Internal Rust parity proof for positive scroll-wheel events |
| `rust-real-shortcut-e2e` | `OMIGA_COMPUTER_USE_BACKEND=real` | Opens/closes a temporary TextEdit document, replaces its content through select_all + type_text | Internal Rust parity proof for fixed shortcuts |
| `rust-real-visual-text` | `OMIGA_COMPUTER_USE_BACKEND=real` | Opens/closes a temporary dialog, captures a screenshot for OCR | Internal Rust parity proof for macOS Vision OCR observe |

## Manual app QA

| Area | Steps | Expected result |
| --- | --- | --- |
| Composer gate | Leave Computer Use off, send a normal message | `computer_*` tools are not exposed |
| Task mode | Select Computer Use → Task, send one message | `computer_*` available for that task; mode returns to off |
| Session mode | Select Computer Use → Session | `computer_*` remains available until turned off or stopped |
| Stop | Click Computer Use stop action | Core run is stopped; backend stop is attempted best-effort |
| Settings backend diagnostics | Open Settings → Computer Use | Backend diagnostics load without permission probes, screenshots, or backend process startup |
| Permission probe | Click “检测权限” | Accessibility/Screen Recording status refreshes only after click |
| Allowed apps | Remove current app from allowlist and observe | Backend/core return `app_not_allowed`; no screenshot/action is performed |
| Result-first evidence UI | Refresh result evidence in Settings | UI shows result records, OK/Needs attention/Blocked/Stopped signals, size/retention/evidence path, not screenshots or step-by-step operation content |
| Screenshot retention | Enable screenshots, observe, then refresh evidence | Screenshots land under `/tmp/omiga-computer-use/<runId>/`; UI keeps only evidence path/counts while retention cleanup reports pruned temp dirs when old |
| Optional OCR observe | Call observe with `extractVisualText=true` on a visible text target | Result includes `visualTextRequested`, bounded `visualText` boxes, counts/limits, and structured `visualTextError` if Screen Recording/Vision is unavailable |
| Packaging config | Run `scripts/computer-use-release-check.py` | Tauri resources include `bundled_plugins`; marketplace, plugin manifest, MCP config, wrappers, and executable bits are valid |

## Security regression checks

| Risk | Check | Expected result |
| --- | --- | --- |
| Raw MCP bypass | Try model-visible `mcp__computer__click` | Tool schema is hidden; execution is rejected |
| Unsupported click buttons | Inspect `computer_click` schema | Only `left` is advertised; right/middle are not model-visible until implemented |
| Observed element click | Observe, then call `computer_click_element` with a returned element id | The backend resolves the element center from the same observation cache and clicks after target validation |
| Semantic AX metadata | Observe a target with visible controls | Elements include bounded semantic hints (`kind`, `depth`, `parentId`, `roleDescription`, `enabled`/`focused` when available, and `interactable`) without increasing the depth/count caps |
| Optional visual OCR | Observe a text target with `extractVisualText=true` | Backend captures only after allowlist passes, returns at most the visual-text cap, does not merge OCR into actions, and fails soft via `visualTextError` |
| Fixed key press | Observe, then call `computer_key` with `enter`, `escape`, arrow, paging, or delete key | Backend revalidates target and emits only fixed AppleScript key codes; arbitrary shortcuts/scripts are rejected |
| Left-button drag | Observe a target, then call `computer_drag` with start/end coordinates inside the target | Backend revalidates target, rejects unsupported buttons/out-of-window endpoints, and emits a fixed left-button mouse down/drag/up sequence |
| Fixed scroll wheel | Observe a scrollable target, then call `computer_scroll` with `down` | Backend revalidates target and posts a fixed CoreGraphics scroll-wheel event at the target center |
| Fixed shortcut allowlist | Observe a target, then call `computer_shortcut` with `select_all` and with unsupported `command_q` | `select_all` runs after target revalidation; unsupported shortcuts return `unsupported_shortcut` and no arbitrary combo/script is executed |
| Target switch race | Observe app A, switch to app B, then click/type | Backend `validate_target` returns target mismatch; no action |
| Coordinate escape | Click outside target bounds | Core or backend returns `point_outside_target_window`; no click |
| Secret clipboard leak | Type long `password = ...` with clipboard fallback allowed | Direct typing unsupported; clipboard fallback stays false; response does not echo text |
| Stop enforcement | Stop run, then action with same `runId` | `run_stopped` |
| Internal Rust flag missing binary | Set `OMIGA_COMPUTER_USE_EXPERIMENTAL_RUST=1 OMIGA_COMPUTER_USE_SIDECAR=rust` without installed sidecar | Wrapper exits with install guidance; Settings remains Python-facing |
| Artifact manifest drift | Generate a manifest, then change a packaged Computer Use artifact and verify it | `artifact-manifest-verify` reports size/hash/executable mismatch |
| Signing preflight | Run `--include-signing-preflight` on macOS | `codesign`, `xcrun notarytool`, `xcrun stapler`, Tauri macOS plist references, entitlements, and Info.plist are present/parseable |

## Release checklist

1. Default low-risk gate:
   - `scripts/computer-use-release-check.py`
2. Final packaging hygiene gate:
   - `scripts/computer-use-release-check.py --fail-on-generated-artifacts`
   - This fails if bundled plugin resources contain generated `__pycache__` or `.pyc` files.
3. Optional full app/build gate:
   - `scripts/computer-use-release-check.py --include-build --include-cargo-test`
4. On a macOS machine with permission prompts reviewed:
   - `scripts/computer-use-smoke.py --suite python-real-safe`
   - or `scripts/computer-use-release-check.py --include-real-safe`
   - add `--require-real-observe` when this gate must fail unless real observe succeeds
5. On a permission-ready macOS QA machine, run the controlled positive E2E:
   - `scripts/computer-use-smoke.py --suite python-real-dialog-e2e`
   - or `scripts/computer-use-release-check.py --include-real-dialog-e2e`
6. On a permission-ready macOS QA machine, run the fixed-key positive E2E:
   - `scripts/computer-use-smoke.py --suite python-real-key-e2e`
   - or `scripts/computer-use-release-check.py --include-real-key-e2e`
7. On a permission-ready macOS QA machine, run the drag positive E2E:
   - `scripts/computer-use-smoke.py --suite python-real-drag-e2e`
   - or `scripts/computer-use-release-check.py --include-real-drag-e2e`
8. On a permission-ready macOS QA machine, run the scroll positive E2E:
   - `scripts/computer-use-smoke.py --suite python-real-scroll-e2e`
   - or `scripts/computer-use-release-check.py --include-real-scroll-e2e`
9. On a permission-ready macOS QA machine, run the shortcut positive E2E:
   - `scripts/computer-use-smoke.py --suite python-real-shortcut-e2e`
   - or `scripts/computer-use-release-check.py --include-real-shortcut-e2e`
10. On a permission-ready macOS QA machine, run the optional visual OCR E2E:
   - `scripts/computer-use-smoke.py --suite python-real-visual-text`
   - or `scripts/computer-use-release-check.py --include-real-visual-text`
11. If testing internal Rust feature packaging:
   - `scripts/install-computer-use-sidecar.sh --profile release`
   - `scripts/computer-use-release-check.py --include-rust-sidecar --rust-bin /path/to/computer-use-sidecar --verify-installed-sidecar`
   - `OMIGA_COMPUTER_USE_EXPERIMENTAL_RUST=1 OMIGA_COMPUTER_USE_SIDECAR=rust OMIGA_COMPUTER_USE_BACKEND=mock scripts/computer-use-smoke.py --suite rust-mock`
12. Before distribution packaging:
   - `scripts/computer-use-release-check.py --write-artifact-manifest /tmp/omiga-computer-use-artifacts.json --verify-artifact-manifest /tmp/omiga-computer-use-artifacts.json --include-signing-preflight --skip-smoke --fail-on-generated-artifacts`
