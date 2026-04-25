#!/usr/bin/env bash
# Generate local Rust coverage for the Tauri backend.
#
# Requires cargo-llvm-cov:
#   cargo install cargo-llvm-cov
#
# Usage:
#   ./scripts/coverage.sh
#   COV_FORMAT=lcov ./scripts/coverage.sh
#   COV_OPEN=0 ./scripts/coverage.sh research_cli

set -euo pipefail

cd "$(dirname "$0")/.."

if ! command -v cargo-llvm-cov >/dev/null 2>&1; then
  echo "ERROR: cargo-llvm-cov is not installed." >&2
  echo "Install with: cargo install cargo-llvm-cov" >&2
  exit 1
fi

COV_FORMAT="${COV_FORMAT:-html}"
COV_OUT="${COV_OUT:-coverage}"
COV_OPEN="${COV_OPEN:-0}"

cmd=(cargo llvm-cov --manifest-path src-tauri/Cargo.toml)

case "$COV_FORMAT" in
  html)
    cmd+=(--html --output-dir "$COV_OUT")
    ;;
  lcov)
    mkdir -p "$COV_OUT"
    cmd+=(--lcov --output-path "$COV_OUT/lcov.info")
    ;;
  text)
    cmd+=(--text)
    ;;
  json)
    mkdir -p "$COV_OUT"
    cmd+=(--json --output-path "$COV_OUT/coverage.json")
    ;;
  *)
    echo "ERROR: unsupported COV_FORMAT '$COV_FORMAT' (html|lcov|text|json)." >&2
    exit 1
    ;;
esac

if [[ $# -gt 0 ]]; then
  cmd+=(-- "$@")
fi

echo "Running: ${cmd[*]}"
"${cmd[@]}"

if [[ "$COV_FORMAT" == "html" && "$COV_OPEN" == "1" ]]; then
  index="$COV_OUT/html/index.html"
  if [[ -f "$index" ]]; then
    if command -v open >/dev/null 2>&1; then
      open "$index"
    elif command -v xdg-open >/dev/null 2>&1; then
      xdg-open "$index"
    else
      echo "Coverage report: $index"
    fi
  fi
fi
