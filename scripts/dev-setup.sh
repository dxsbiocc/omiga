#!/usr/bin/env bash
# Prepare a local Omiga checkout for development and run baseline verification.
#
# Usage:
#   ./scripts/dev-setup.sh
#
# Optional skips:
#   SKIP_NPM=1 ./scripts/dev-setup.sh
#   SKIP_RUST=1 ./scripts/dev-setup.sh
#   SKIP_VERIFY=1 ./scripts/dev-setup.sh

set -euo pipefail

cd "$(dirname "$0")/.."

echo "=== Omiga developer setup ==="

if [[ "${SKIP_NPM:-0}" != "1" ]]; then
  if ! command -v npm >/dev/null 2>&1; then
    echo "ERROR: npm is required for frontend setup." >&2
    exit 1
  fi

  if [[ -f package-lock.json ]]; then
    echo "[frontend] npm ci"
    npm ci
  else
    echo "[frontend] npm install"
    npm install
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
  echo "[verify] frontend tests"
  npm test

  echo "[verify] frontend build"
  npm run build

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
