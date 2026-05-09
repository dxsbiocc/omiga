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

## Allowed apps and screenshots

**Settings → Computer Use → Allowed apps** is enforced by the core and the
macOS sidecar. Each entry may be an app name such as `Omiga` or a bundle id
such as `com.omiga.desktop`. If the current target is not in the allowlist,
Computer Use returns `app_not_allowed` and does not perform the action.

`Save observation screenshots` is off by default. When it is off, the backend
still reads target metadata but does not write a screenshot file for
`computer_observe`.

## macOS permissions

Phase 8 supports macOS only. The backend may need:

- **Accessibility** — required for clicking, typing, and reading window
  metadata through fixed System Events calls.
- **Screen Recording** — required for screenshots returned by
  `computer_observe`.

If macOS blocks access, the backend returns a structured error and marks the
target unsafe instead of executing an action.

## Safety boundaries

- Models see only Omiga's stable `computer_*` tools.
- Raw backend tools such as `mcp__computer__click` are hidden and rejected.
- Actions require a recent `computer_observe`.
- The observed/validated target must match Settings → Computer Use allowed
  apps.
- Click/type actions must include the latest `observationId` and
  `targetWindowId`.
- Omiga revalidates the frontmost target before actions.
- Stop marks the run stopped in core and blocks later actions.
- `computer_type` output and UI cards hide full typed text.
- The backend uses fixed internal macOS commands; it does not run
  model-provided scripts.

## Local run records

Audit records are project-local:

```text
<project>/.omiga/computer-use/runs/{sessionId}/{runId}/
  run.json
  actions.jsonl
```

Open **Settings → Computer Use** to see run/action counts and clear local run
records. Secret-like fields are redacted before audit entries are written.

## Current MVP limitations

- macOS only.
- Shallow accessibility metadata: active-window element only.
- Conservative occlusion handling: a frontmost target mismatch blocks actions.
- Left-click only.
- `computer_type` uses controlled clipboard paste and restores the prior
  clipboard afterward.
- OCR, deep AX trees, drag gestures, and Windows/Linux backends are deferred.
