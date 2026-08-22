#!/usr/bin/env bash
# Fixture: schemas_missing_required_column
# FM: fm-schemas-missing-required-column (P1)
#
# Drops the required `comments.text` column (one of the columns the
# `schema.columns` check actively watches). `--repair` reapplies the
# canonical schema via the JSONL→DB rebuild path, which re-creates the
# comments table with the full column set.
#
# Note: the rebuild path is destructive at the table level (full rebuild),
# which is acceptable here because the lost column would have been NULL on
# every existing row anyway. Test harness uses raw DROP COLUMN; the fixer
# never DROPs.

set -euo pipefail
target_dir="${1:?usage: corrupt.sh <target_dir>}"
tool_bin="${TOOL_BIN:-br}"

mkdir -p "$target_dir"
cd "$target_dir"
"$tool_bin" init >/dev/null 2>&1

"$tool_bin" create --title "missing-column seed" --type task --priority 2 --json >/dev/null
"$tool_bin" sync --flush-only >/dev/null 2>&1 || true

# Drop indexes that reference the column first (SQLite's ALTER TABLE DROP
# COLUMN refuses to drop columns referenced by indexes). Then drop the
# column. Test harness only. Uses python3 because the sqlite3 CLI isn't
# guaranteed in the harness env.
python3 <<'PY'
import sqlite3
conn = sqlite3.connect(".beads/beads.db")
conn.execute("DROP INDEX IF EXISTS idx_comments_issue_id")
conn.execute("DROP INDEX IF EXISTS idx_comments_created_at")
conn.execute("ALTER TABLE comments DROP COLUMN text")
conn.commit()
conn.close()
PY

if [ -e .fixture_baseline ]; then
  echo "fixture baseline already exists; expected a fresh workspace" >&2
  exit 1
fi
mkdir -p .fixture_baseline
tar --exclude=.fixture_baseline -cf .fixture_baseline/state.tar .
