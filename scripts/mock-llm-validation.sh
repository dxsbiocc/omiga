#!/usr/bin/env bash
# Run deterministic no-secret orchestration validation against a local OpenAI-compatible mock LLM.
# This is the CI-safe counterpart to scripts/real-llm-validation.sh.
#
# v2.0 tools covered by mock_llm_runtime_harness:
#
#   Core scheduling / cron:
#     CronCreate       — schedule cron job validation
#     CronList         — list cron jobs validation
#     CronDelete       — delete cron job validation
#
#   Task monitoring:
#     Monitor          — monitor task output validation
#
#   Notifications:
#     PushNotification — native notification validation
#
#   Git worktrees:
#     EnterWorktree    — enter git worktree validation
#     ExitWorktree     — exit git worktree validation

set -euo pipefail

cd "$(dirname "$0")/.."

cargo test --manifest-path src-tauri/Cargo.toml --test mock_llm_runtime_harness --quiet
