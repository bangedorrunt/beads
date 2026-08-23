#!/usr/bin/env bash
# governed-by: ADR-0003
# Regenerate the bv golden fixtures under tests/fixtures/bv_parity/ from real
# `bv` output on the 12-issue fixture workspace (ADR-0003 §4).
#
# Requirements: `bv` on PATH (built from Dicklesworthstone/beads_viewer at the
# commit recorded in tests/fixtures/bv_parity/BV_COMMIT.txt) and jq.
# The script stamps the generation instant (RFC3339) into GOLDEN_NOW.txt; the
# parity tests export it as BR_ANALYSIS_NOW so staleness/urgency scoring is
# reproducible against these goldens forever. CI never needs Go or bv.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
fixtures="$root/tests/fixtures/bv_parity"
bv_bin="${BV_BIN:-$(command -v bv)}"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

mkdir -p "$tmp/.beads"
# bv reads issues + inline dependencies straight from the JSONL (research doc
# §0: no SQLite, no events/comments), so the fixture ships unmodified.
cp "$fixtures/fixture_issues.jsonl" "$tmp/.beads/issues.jsonl"

cd "$tmp"
for cmd in triage next plan insights; do
  "$bv_bin" --robot-"$cmd" --format json >"$tmp/robot-$cmd.json"
done

now="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
commit="$("$bv_bin" --version 2>/dev/null | sed -n 's/.*-\([0-9a-f]\{7,\}\)$/\1/p')"

for cmd in triage next plan insights; do
  if ! jq -e . "$tmp/robot-$cmd.json" >/dev/null 2>&1; then
    echo "FATAL: bv --robot-$cmd emitted non-JSON output" >&2
    exit 1
  fi
done

mv "$tmp/robot-triage.json" "$tmp/robot-next.json" "$tmp/robot-plan.json" \
   "$tmp/robot-insights.json" "$fixtures/"
printf '%s\n' "$now" >"$fixtures/GOLDEN_NOW.txt"
printf '%s\n' "${commit:-unknown}" >"$fixtures/BV_COMMIT.txt"

echo "regenerated goldens at $now (bv commit ${commit:-unknown})"
jq -r '.triage.quick_ref.top_picks[0].id // empty' "$fixtures/robot-triage.json" \
  | sed 's/^/triage top pick: /'
jq -r '.id // empty' "$fixtures/robot-next.json" | sed 's/^/next pick: /'
