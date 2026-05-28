# Browser Operator Python sidecar (MVP)

`browser_operator.py` is a stdio JSON Lines RPC sidecar for browser automation.

The Omiga facade and sidecar source are bundled with the app. The heavy
`browser-use` runtime and Playwright browser binaries are installed on demand
into a managed user directory so the desktop bundle does not grow by hundreds of
MB.

## Install backend

Omiga does **not** install this backend during app startup or package install.
The composer prompts the user to install it only when they first enable Browser
Operator mode. Use the commands below for manual/dev installs.

Recommended managed install:

```bash
python3 src-tauri/browser-operator/install_backend.py --json
```

This creates:

- virtualenv: `~/.omiga/browser-operator/.venv`
- Playwright browser cache: `~/.omiga/browser-operator/ms-playwright`

The Rust launcher auto-detects that venv. You can skip Chromium/browser binary
download when you only plan to connect to an existing Chrome/CDP endpoint:

```bash
python3 src-tauri/browser-operator/install_backend.py --skip-browser-install --json
```

Runtime override knobs:

- `OMIGA_BROWSER_OPERATOR_HOME=/custom/path`
- `OMIGA_BROWSER_OPERATOR_PYTHON=/path/to/python`
- `OMIGA_BROWSER_OPERATOR_BOOTSTRAP_PYTHON=/path/to/python3`

Tauri commands used by the on-demand composer prompt:

- `browser_operator_backend_status`
- `browser_operator_install_backend`

## Run

```bash
python3 src-tauri/browser-operator/browser_operator.py
```

Each stdin line must be JSON:

```json
{"id":1,"method":"health","params":{}}
```

Each stdout line is JSON:

```json
{"id":1,"ok":true,"result":{"status":"ok"}}
```

## Supported methods

- `health`
- `open` — `{ "url": "https://example.com" }`
- `snapshot` — optional `{ "max_elements": 200, "max_text_chars": 20000 }`
- `click` — `{ "selector": "button[type=submit]" }` or `{ "index": 0 }`
- `fill` — `{ "selector": "input[name=email]", "value": "alice@example.com" }`
- `screenshot` — optional `{ "format": "png" }`
- `close`

## Environment variables

- `OMIGA_BROWSER_OPERATOR_HEADLESS=true|false`
- `OMIGA_BROWSER_OPERATOR_CDP_URL=http://localhost:9222`
- `PLAYWRIGHT_BROWSERS_PATH=/path/to/ms-playwright` (auto-set for managed venv)

## Behavior notes

- The script first tries `from browser_use import Browser` and then `BrowserSession`.
- If `browser_use` is unavailable, non-`health` methods return:
  - `ok: false`
  - `error.code: "browser_use_unavailable"`
- `fill` responses never echo the filled plaintext value.
- `screenshot` persists output into a temp session directory and returns `result.path`.

## Self-test

```bash
python3 src-tauri/browser-operator/browser_operator.py --self-test
```

This validates request parsing and `health` without opening a browser.

## Rust integration sketch

Spawn the process with piped stdin/stdout and exchange one JSON object per line.

Pseudo-flow:

1. `Command::new("python3")`
2. arg `src-tauri/browser-operator/browser_operator.py`
3. `stdin(Stdio::piped())`, `stdout(Stdio::piped())`
4. write lines like `{"id":1,"method":"health","params":{}}\n`
5. read one stdout line per response and decode as JSON

Example request sequence:

```json
{"id":1,"method":"health","params":{}}
{"id":2,"method":"open","params":{"url":"https://example.com"}}
{"id":3,"method":"snapshot","params":{}}
{"id":4,"method":"click","params":{"selector":"a[href]"}}
{"id":5,"method":"screenshot","params":{}}
{"id":6,"method":"close","params":{}}
```
