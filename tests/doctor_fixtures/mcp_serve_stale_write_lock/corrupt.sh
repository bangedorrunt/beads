#!/usr/bin/env bash
# Fixture: mcp_serve_stale_write_lock
# FM: fm-agent_coordination-mcp-serve-stale-write-lock.
#
# Simulates a killed `br serve` owner by planting an old regular lock inode and
# an orphan holder-pid sidecar. No process owns the advisory lock.

set -euo pipefail

target_dir="${1:?usage: corrupt.sh <target_dir>}"
tool_bin="${TOOL_BIN:-br}"

mkdir -p "$target_dir"

# GNU-only `touch -d` is unavailable on BSD/macOS fixture hosts.
set_old_mtime() {
  python3 - "$@" <<'PY'
import calendar, os, sys
when = calendar.timegm((2024, 1, 1, 0, 0, 0))
for path in sys.argv[1:]:
    os.utime(path, (when, when))
PY
}

cd "$target_dir"

"$tool_bin" init --quiet 2>&1
"$tool_bin" create --title "mcp stale lock seed" --type task --priority 2 --json >/dev/null

: > .beads/.write.lock
printf '99999999\n' > .beads/.write.lock.holder.pid
set_old_mtime .beads/.write.lock .beads/.write.lock.holder.pid
{ stat -c '%d:%i' .beads/.write.lock 2>/dev/null \
    || stat -f '%d:%i' .beads/.write.lock; } > .fixture_lock_identity

echo "fixture corrupt.sh: planted persistent .write.lock and orphan holder pid sidecar" >&2
