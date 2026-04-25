#!/usr/bin/env bash
# Run deterministic no-secret orchestration validation against a local OpenAI-compatible mock LLM.
# This is the CI-safe counterpart to scripts/real-llm-validation.sh.

set -euo pipefail

cd "$(dirname "$0")/.."

cargo test --manifest-path src-tauri/Cargo.toml --test mock_llm_runtime_harness --quiet
