#!/usr/bin/env bash
# Run ignored real-provider validation tests using Omiga's normal config loader.
#
# Usage:
#   ./scripts/real-llm-validation.sh smoke
#   ./scripts/real-llm-validation.sh schedule
#   ./scripts/real-llm-validation.sh team
#   ./scripts/real-llm-validation.sh autopilot
#   ./scripts/real-llm-validation.sh all
#
# The tests call omiga_lib::llm::load_config(), so provider/model come from
# project config, ~/.config/omiga/*, or legacy ~/.omiga/*.

set -euo pipefail

cd "$(dirname "$0")/.."

suite="${1:-smoke}"

case "$suite" in
  smoke|schedule|team|autopilot|all) ;;
  *)
    cat >&2 <<MSG
Unknown suite: $suite
Usage: $0 [smoke|schedule|team|autopilot|all]
MSG
    exit 2
    ;;
esac

find_config() {
  local names=(omiga.yaml omiga.yml omiga.json omiga.toml)
  local name
  for name in "${names[@]}"; do
    if [[ -f "$name" ]]; then
      printf '%s\n' "$PWD/$name"
      return 0
    fi
  done

  local config_home="${XDG_CONFIG_HOME:-$HOME/.config}"
  for name in "${names[@]}"; do
    if [[ -f "$config_home/omiga/$name" ]]; then
      printf '%s\n' "$config_home/omiga/$name"
      return 0
    fi
  done

  for name in "${names[@]}"; do
    if [[ -f "$HOME/.omiga/$name" ]]; then
      printf '%s\n' "$HOME/.omiga/$name"
      return 0
    fi
  done

  return 1
}

if config_path="$(find_config)"; then
  echo "[real-llm] Using config: $config_path"
else
  cat >&2 <<'MSG'
[real-llm] No Omiga config file found.
Create one from the template, then retry:

  cp config.example.yaml omiga.yaml
  # edit omiga.yaml, or use ${ENV_VAR} placeholders plus exported secrets

See docs/REAL_LLM_VALIDATION.md for details.
MSG
  exit 1
fi

run_test() {
  local test_target="$1"
  echo "[real-llm] cargo test --test $test_target -- --ignored --nocapture"
  cargo test --manifest-path src-tauri/Cargo.toml --test "$test_target" -- --ignored --nocapture
}

case "$suite" in
  smoke)
    run_test real_runtime_smoke
    ;;
  schedule)
    run_test real_schedule_harness
    ;;
  team)
    run_test real_team_harness
    ;;
  autopilot)
    run_test real_autopilot_harness
    ;;
  all)
    run_test real_runtime_smoke
    run_test real_schedule_harness
    run_test real_team_harness
    run_test real_autopilot_harness
    ;;
esac
