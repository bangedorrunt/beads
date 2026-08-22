#!/usr/bin/env bash
# Fixture: duplicate_metadata_rows
# FM: fm-caches_indexes-* (recoverable anomalies — duplicate metadata key)
#
# Inserts two extra rows under an existing `jsonl_content_hash` metadata key.
# Triggers `db.recoverable_anomalies` = error with the "metadata contains
# duplicate rows" finding. `--repair` rebuilds the DB from JSONL, which
# replaces metadata with a single canonical entry per key.

set -euo pipefail
target_dir="${1:?usage: corrupt.sh <target_dir>}"
tool_bin="${TOOL_BIN:-br}"

mkdir -p "$target_dir"
cd "$target_dir"
"$tool_bin" init >/dev/null 2>&1
"$tool_bin" create --title "dup-meta seed" --type task --priority 2 --json >/dev/null
"$tool_bin" sync --flush-only >/dev/null 2>&1 || true

# Insert two duplicate rows under an existing key. Uses python3 because the
# sqlite3 CLI isn't guaranteed in the harness env.
python3 <<'PY'
import sqlite3
conn = sqlite3.connect(".beads/beads.db")
conn.execute("INSERT INTO metadata(key, value) VALUES('jsonl_content_hash', 'dup-row-1')")
conn.execute("INSERT INTO metadata(key, value) VALUES('jsonl_content_hash', 'dup-row-2')")
conn.commit()
conn.close()
PY

if [ -e .fixture_baseline ]; then
  echo "fixture baseline already exists; expected a fresh workspace" >&2
  exit 1
fi
mkdir -p .fixture_baseline
tar --exclude=.fixture_baseline -cf .fixture_baseline/state.tar .
