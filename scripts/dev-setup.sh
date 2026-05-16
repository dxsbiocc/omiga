#!/usr/bin/env bash
# Prepare a local Omiga checkout for development and run baseline verification.
#
# Usage:
#   ./scripts/dev-setup.sh
#
# Optional skips:
#   SKIP_JS=1 ./scripts/dev-setup.sh
#   SKIP_NPM=1 ./scripts/dev-setup.sh   # legacy alias for SKIP_JS
#   SKIP_RUST=1 ./scripts/dev-setup.sh
#   SKIP_VERIFY=1 ./scripts/dev-setup.sh

set -euo pipefail

cd "$(dirname "$0")/.."

echo "=== Omiga developer setup ==="

require_bun() {
  if ! command -v bun >/dev/null 2>&1; then
    echo "ERROR: Bun is required for JavaScript setup." >&2
    echo "Install Bun 1.x, then rerun this script. This repository intentionally does not use npm install." >&2
    exit 1
  fi
}

if [[ "${SKIP_JS:-${SKIP_NPM:-0}}" != "1" ]]; then
  require_bun

  if [[ -f bun.lock ]]; then
    echo "[frontend] bun install --frozen-lockfile"
    bun install --frozen-lockfile
  else
    echo "[frontend] bun install"
    bun install
  fi
fi

if [[ "${SKIP_RUST:-0}" != "1" ]]; then
  if ! command -v cargo >/dev/null 2>&1; then
    echo "ERROR: cargo is required for Rust/Tauri setup." >&2
    exit 1
  fi

  if command -v rustup >/dev/null 2>&1; then
    echo "[rust] ensuring rustfmt and clippy components"
    rustup component add rustfmt clippy >/dev/null
  fi
fi

if [[ "${SKIP_VERIFY:-0}" != "1" ]]; then
  require_bun

  echo "[verify] frontend tests"
  bun run test

  echo "[verify] frontend build"
  bun run build

  echo "[verify] rust fmt"
  cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check

  echo "[verify] rust clippy (advisory until existing warning debt is retired)"
  if ! cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets; then
    echo "[verify] rust clippy reported existing warning/error debt; continuing because strict clippy is not yet a gate." >&2
  fi

  echo "[verify] rust tests"
  cargo test --manifest-path src-tauri/Cargo.toml
fi

echo "=== setup complete ==="
