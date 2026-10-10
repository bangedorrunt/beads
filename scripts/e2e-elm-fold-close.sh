#!/usr/bin/env bash
# E2E for the elm-effect close core. Reproducible and self-checking: it
# builds a fresh workspace, drives `br close` through the fold, and writes
# a JSON artifact recording what the ledger actually did.
#
# Run:   sh scripts/e2e-elm-fold-close.sh [path-to-br]
# Probe: each step asserts its own outcome; a wrong outcome exits non-zero.
set -uo pipefail

BR=${1:-target/debug/br}
HERE=$(cd "$(dirname "$0")/.." && pwd)
BR=$(cd "$HERE" && realpath "$BR" 2>/dev/null || echo "$HERE/$BR")
WORK=$(mktemp -d "${TMPDIR:-/tmp}/br-elm-e2e-XXXXXX")
ARTIFACT=${ARTIFACT:-"$HERE/target/elm-fold-close-e2e.json"}
export AGENT_NAME=${AGENT_NAME:-IvoryRidge}

fail() { printf 'e2e FAILED at: %s\n' "$1" >&2; exit 1; }

cd "$WORK" || fail "workdir"
git init -q . || fail "git init"
"$BR" init --prefix bd --actor IvoryRidge >/dev/null 2>&1 || fail "br init"

# A P2 bead with a typed, loop-runnable VERIFY fence: that is the cheap
# band, where `command-verified` is the ONLY legal verdict kind.
"$BR" create "e2e elm fold bead" \
  -d "proves br close routes its fail-closed decision through the fold" \
  --verify 'cargo test -p beads' --priority 2 \
  --principle 'boundary-discipline — the fold decides; the interpreter reads' \
  --json >/dev/null 2>&1 || fail "br create"
ID=$("$BR" list --status open --json 2>/dev/null | python3 -c 'import json,sys;print(json.load(sys.stdin)["issues"][0]["id"])')

# Step 1 — a close with no PASS row is refused, and the refusal names the
# legal gate. This is the invariant the whole bead exists for.
CLOSED_WITHOUT_VERDICT=$("$BR" close "$ID" --reason "no verdict recorded" --commit-sha 0000000 --json 2>/dev/null)
REFUSED=$(printf '%s' "$CLOSED_WITHOUT_VERDICT" | python3 -c 'import json,sys
try: d=json.load(sys.stdin)
except Exception: print("no"); raise SystemExit
print("POLICY_VIOLATION" if d.get("error",{}).get("code")=="POLICY_VIOLATION" else "no")')
[ "$REFUSED" = "POLICY_VIOLATION" ] || fail "close without a verdict row was not refused"
STILL_OPEN=$("$BR" show "$ID" --json 2>/dev/null | python3 -c 'import json,sys;print(json.load(sys.stdin)["status"])')
[ "$STILL_OPEN" = "open" ] || fail "a refused close still moved the status"
NO_VERDICT_NAMES_LEGAL=$(printf '%s' "$CLOSED_WITHOUT_VERDICT" | \
  python3 -c 'import sys;sys.exit(0 if "command-verified" in sys.stdin.read() else 1)' && echo yes || echo no)

# Step 2 — a FAIL row also refuses, and names itself rather than the
# generic "no legal PASS row" message.
"$BR" gate report "$ID" --gate command-verified --provider IvoryRidge --status fail \
  --to closed --note 'sha=0000000' --json >/dev/null 2>&1 || fail "br gate report fail"
FAIL_CLOSE=$("$BR" close "$ID" --reason "fail row recorded" --commit-sha 0000000 --json 2>/dev/null)
FAIL_REFUSED=$(printf '%s' "$FAIL_CLOSE" | python3 -c 'import json,sys
d=json.load(sys.stdin); e=d.get("error",{})
print("POLICY_VIOLATION" if e.get("code")=="POLICY_VIOLATION" else "no")')
[ "$FAIL_REFUSED" = "POLICY_VIOLATION" ] || fail "a FAIL verdict row did not refuse the close"
FAIL_NAMED_ITSELF=$(printf '%s' "$FAIL_CLOSE" | \
  python3 -c 'import sys;sys.exit(0 if "gate_command-verified" in sys.stdin.read() else 1)' && echo yes || echo no)
[ "$FAIL_NAMED_ITSELF" = "yes" ] || fail "a FAIL verdict row was not refused under its own name"

# Step 3 — a legal PASS row authorizes the write and the close lands.
"$BR" gate report "$ID" --gate command-verified --provider IvoryRidge --status pass \
  --to closed --note 'sha=0000000' --json >/dev/null 2>&1 || fail "br gate report pass"
CLOSE_OUT=$("$BR" close "$ID" --reason "the fold authorized the write" --commit-sha 0000000 --json 2>/dev/null)
STATUS=$(printf '%s' "$CLOSE_OUT" | python3 -c 'import json,sys
d=json.load(sys.stdin)
print(d[0]["status"] if isinstance(d,list) and d else d.get("error",{}).get("code","unknown"))')
[ "$STATUS" = "closed" ] || fail "a legal PASS verdict did not close the bead (got: $STATUS)"

# Step 4 — replaying the issue event log does not grow the verdict table.
# The gate sidecar is a stream of its own; the fold never writes one from
# the other, so importing issues with no sidecar leaves verdicts untouched.
#
# The assertion is only meaningful if the sidecar really was flushed first,
# so check that it carries rows BEFORE removing it. Without that guard the
# step passes trivially against a workspace that never had one.
[ -f .beads/gates.jsonl ] || fail "the gate sidecar was never flushed"
BEFORE=$(wc -l < .beads/gates.jsonl | tr -d ' ')
[ "$BEFORE" -gt 0 ] || fail "the gate sidecar carried no verdict rows before the import"
rm -f .beads/gates.jsonl
"$BR" sync --import-only >/dev/null 2>&1

GATE_ROWS=0
[ -f .beads/gates.jsonl ] && GATE_ROWS=$(wc -l < .beads/gates.jsonl | tr -d ' ')
[ "$GATE_ROWS" = "0" ] || fail "an issue import grew the verdict table (${GATE_ROWS} rows)"

python3 - "$ARTIFACT" "$ID" "$STATUS" "$NO_VERDICT_NAMES_LEGAL" "$FAIL_NAMED_ITSELF" "$BEFORE" "$GATE_ROWS" "$WORK" <<'PY' || fail "artifact"
import json, sys
artifact, bead, status, no_verdict_names, fail_named, before, gate_rows, work = sys.argv[1:9]
json.dump({
    "bead": bead, "closed_status": status,
    "refused_without_verdict": True,
    "no_verdict_refusal_names_legal_gate": no_verdict_names == "yes",
    "fail_row_refusal_named_itself": fail_named == "yes",
    "gate_rows_before_import": int(before),
    "gate_rows_after_issues_only_import": int(gate_rows),
    "workspace": work,
}, open(artifact, "w"), indent=2)
print(f"wrote {artifact}")
PY

printf 'e2e OK: %s closed through the fold; verdict table stayed empty on issues-only import\n' "$ID"
rm -rf "$WORK"
