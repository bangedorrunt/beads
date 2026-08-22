#!/usr/bin/env bash
# Fixture: base_jsonl_missing_post_flush
# FM: fm-state_files-base-jsonl-missing-or-stale (missing-post-flush subset)
#
# Older skeletons removed .beads/beads.base.jsonl after a sync flush. This
# fixture avoids deletion: a fresh `br init` workspace already has no anchor,
# so we synthesize the post-flush evidence by setting metadata.last_export_time
# directly.

set -euo pipefail
target_dir="${1:?usage: corrupt.sh <target_dir>}"
tool_bin="${TOOL_BIN:-br}"

mkdir -p "$target_dir"
cd "$target_dir"
"$tool_bin" init >/dev/null 2>&1

# Avoid unrelated inner-gitignore repair noise; this fixture is testing the
# detect-only missing-post-flush branch.
for pattern in ".write.lock" "*.tmp"; do
  if ! grep -Fxq "$pattern" .beads/.gitignore 2>/dev/null; then
    printf '\n%s\n' "$pattern" >> .beads/.gitignore
  fi
done

# Uses python3 because the sqlite3 CLI isn't guaranteed in the harness env.
python3 <<'PY'
import sqlite3
conn = sqlite3.connect(".beads/beads.db")
conn.execute(
    "UPDATE metadata SET value='2026-05-01T00:00:00Z' WHERE key='last_export_time'"
)
conn.commit()
conn.close()
PY

if [ -e .beads/beads.base.jsonl ]; then
  echo "corrupt.sh: fresh workspace unexpectedly has .beads/beads.base.jsonl" >&2
  exit 1
fi

python3 <<'PY' > .fixture_last_export_time
import sqlite3
conn = sqlite3.connect(".beads/beads.db")
row = conn.execute(
    "SELECT value FROM metadata WHERE key='last_export_time' "
    "ORDER BY rowid DESC LIMIT 1"
).fetchone()
print(row[0] if row else "")
conn.close()
PY

if [ "$(cat .fixture_last_export_time)" != "2026-05-01T00:00:00Z" ]; then
  echo "corrupt.sh: failed to plant metadata.last_export_time" >&2
  exit 1
fi

if [ -e .fixture_baseline ]; then
  echo "fixture baseline already exists; expected a fresh workspace" >&2
  exit 1
fi
mkdir -p .fixture_baseline
tar --exclude=.fixture_baseline -cf .fixture_baseline/state.tar .

