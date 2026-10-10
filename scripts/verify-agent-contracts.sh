#!/usr/bin/env bash
# Verify agent-facing schema, JSON/TOON, baseline, and MCP contracts.

set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

# This verifier must be read-only. Do not let local snapshot-update env vars
# turn drift detection into fixture regeneration.
unset INSTA_UPDATE
unset UPDATE_AGENT_BASELINE

cargo_runner=()
if [[ "${BR_AGENT_CONTRACT_USE_RCH:-0}" == "1" ]]; then
    cargo_runner=(rch exec --)
fi

run() {
    printf '\n==> %s\n' "$*"
    "$@"
}

run_cargo() {
    run "${cargo_runner[@]}" cargo "$@"
}

run_cargo test --test snapshots schema_document_golden_ -- --nocapture
run_cargo test --test snapshots schema_command_shapes_match_live_json_outputs -- --nocapture
run_cargo test --test snapshots agent_json_and_toon_outputs_match_semantically -- --nocapture
run_cargo test --test e2e_schema agent_baseline_snapshots_match_current_binary -- --nocapture

# The MCP contract row was removed with the feature itself (ADR-0002 W1): the
# `mcp` feature no longer exists, so `cargo test --features mcp` fails with
# "the package 'beads' does not contain this feature" and `set -e` took the
# whole verifier down at exit 101 — which meant the remaining rows above were
# the only thing anyone ever saw, and a red row read as a red verifier.
# Agents integrate through `br --json ...`; nothing here tests a socket.
