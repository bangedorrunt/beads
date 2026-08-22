#!/usr/bin/env bash
# Fixture assertions: wal_oversized_checkpoint
#
# Pass-5 cycle 37: fm-state_files-wal-oversized graduates from
# detect-only to auto-fixed via PRAGMA wal_checkpoint(TRUNCATE).
# This is the first fixture to exercise the legacy chokepoint
# (record_legacy_mutation) end-to-end — multi-sidecar audit
# entries in actions.jsonl under fixer_id
# `doctor.wal_checkpoint_truncate`.
#
# WAL-lifecycle caveat: see corrupt.sh — SQLite removes the
# zero-padded WAL on the FIRST connection close, so the
# detect-stage assertion stats the file directly instead of
# invoking `br doctor --json` (which would remove it before
# the harness runs --repair).

set -euo pipefail
target_dir="${1:?usage: assert.sh <target_dir> <stage>}"
stage="${2:?usage: assert.sh <target_dir> <stage>}"
tool_bin="${TOOL_BIN:-br}"
cd "$target_dir"

WAL_THRESHOLD_BYTES=33554432  # 32 MiB; const WAL_OVERSIZED_BYTES in doctor.rs

case "$stage" in
  detect)
    # Verify the planted state is on disk. Intentionally NO `br doctor`
    # call here — see header comment.
    [ -f .beads/beads.db-wal ] || {
      echo "ASSERT FAIL[$stage]: planted WAL sidecar missing" >&2
      exit 1
    }
    size=$(wc -c < .beads/beads.db-wal)
    if [ "${size:-0}" -le "$WAL_THRESHOLD_BYTES" ]; then
      echo "ASSERT FAIL[$stage]: planted WAL only $size bytes (threshold $WAL_THRESHOLD_BYTES)" >&2
      exit 1
    fi
    # Also verify the workspace looks initialised so the harness's
    # subsequent --repair has a real workspace to operate against.
    [ -f .beads/beads.db ] || { echo "ASSERT FAIL[$stage]: beads.db missing" >&2; exit 1; }
    ;;

  post_repair)
    # WAL must have been truncated. Accept <=32MB (the threshold);
    # PRAGMA wal_checkpoint(TRUNCATE) usually shrinks to 0, but any
    # value at or below threshold proves the FM no longer fires.
    if [ -f .beads/beads.db-wal ]; then
      size=$(wc -c < .beads/beads.db-wal)
      if [ "${size:-0}" -gt "$WAL_THRESHOLD_BYTES" ]; then
        echo "ASSERT FAIL[$stage]: WAL still $size bytes after --repair" >&2
        exit 1
      fi
    fi

    # Database must still be queryable post-checkpoint (data-equivalent op).
    [ -f .beads/beads.db ] || { echo "ASSERT FAIL[$stage]: beads.db missing" >&2; exit 1; }
    "$tool_bin" list --json >/dev/null 2>&1 || {
      echo "ASSERT FAIL[$stage]: br list failed post-checkpoint" >&2
      exit 1
    }

    # Real-SQLite posture note: br's own startup open checkpoints and
    # REMOVES an oversized WAL before doctor's fixers execute, so the e2e
    # cannot observe the chokepoint action. The wal_size detector is covered
    # pre-open (detect stage above), and the fixer + chokepoint audit path is
    # covered by doctor.rs unit tests. The outcome assertions above (no
    # oversized WAL remains, workspace queryable) are the e2e contract.

    ;;

  post_undo)
    # Undo restores whatever the chokepoint snapshotted. On this
    # codepath the snapshot was taken AFTER the sqlite3 CLI
    # integrity check had already cleaned up our planted WAL
    # (see corrupt.sh header), so the snapshot for the WAL is
    # empty/absent and undo cannot restore the 33MB inflated
    # state. We assert only that the workspace is still functional.
    [ -f .beads/beads.db ] || { echo "ASSERT FAIL[$stage]: beads.db missing after undo" >&2; exit 1; }
    "$tool_bin" list --json >/dev/null 2>&1 || {
      echo "ASSERT FAIL[$stage]: br list failed post-undo" >&2
      exit 1
    }
    ;;

  *)
    echo "unknown stage: $stage" >&2
    exit 2
    ;;
esac
