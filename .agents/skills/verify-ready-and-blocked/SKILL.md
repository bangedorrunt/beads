---
name: verify-ready-and-blocked
description: Formally verify beads (br) ready-queue wave gating, timed-deferral wakeup, and blocked-issues projection via TLA+ and /statechart. Use when modifying src/storage/sqlite.rs (build_ready_issue_candidates_query, get_blocked_issues_with_projection, may_have_blocked_command_results) or src/cli/commands/blocked.rs.
---

# Verify Beads Ready-Queue Wave Gating & Blocked Projection (`br`)

<!-- synced_commit: ec87eca4df12b3664ed60d6ed515df06ce1e2b49 -->

This skill formally models and checks `br`'s SQLite projections for `br ready` and `br blocked` in [`src/storage/sqlite.rs`](../../../src/storage/sqlite.rs) and [`src/cli/commands/blocked.rs`](../../../src/cli/commands/blocked.rs):

1. **Ready candidate selection (`W1InReady` / `W2InReady`)**: [`build_ready_issue_candidates_query`](../../../src/storage/sqlite.rs#L10369-L10449) selects issues with `i.status = 'open' OR (i.status = 'deferred' AND i.defer_until IS NOT NULL)` where `defer_until <= now`, `blocked_issues_cache` has no entry, and no lower-wave active issue exists.
2. **Wave-gate subquery (`LowerWaveGateActive`)**: [`build_ready_issue_candidates_query`](../../../src/storage/sqlite.rs#L10435-L10448) gates higher-wave issues behind lower-wave issues.
3. **Blocked projection & fast-path (`MayHaveBlockedCommandResults` / `W1InBlockedJson`)**: [`get_blocked_issues_with_projection`](../../../src/storage/sqlite.rs#L11807-L11836) and [`may_have_blocked_command_results`](../../../src/storage/sqlite.rs#L11864-L11909) invoked by [`execute`](../../../src/cli/commands/blocked.rs#L114-L120).

## Running the Formal Model

Always run and inspect the TLA+ model using the `/statechart` skill:

```bash
OUT_DIR="$(mktemp -d)"
~/.agents/skills/statechart/scripts/statechart \
  .agents/skills/verify-ready-and-blocked/models/BeadsReadyAndBlocked.tla \
  --config .agents/skills/verify-ready-and-blocked/models/BeadsReadyAndBlocked.cfg \
  --view .agents/skills/verify-ready-and-blocked/models/BeadsReadyAndBlocked.view.ts \
  --out "$OUT_DIR"
cat "$OUT_DIR/report.md"
cat "$OUT_DIR/chart.mmd"
```

To verify the fixed model (`FixReadyWaveAndBlockedJson = TRUE`), pass `--config .agents/skills/verify-ready-and-blocked/models/BeadsReadyAndBlocked.fixed.cfg`.

## Invariants

| Invariant | Meaning | Status on `FixReadyWaveAndBlockedJson = FALSE` | Status on `FixReadyWaveAndBlockedJson = TRUE` |
|-----------|---------|------------------------------------------------|----------------------------------------------|
| `WaveOrderingPreservedInReady` | Whenever a Wave 1 issue is returned by `br ready`, no Wave 2 issue may simultaneously be returned by `br ready`. | **VIOLATED in 2 steps**: `DeferW1Timed -> ExpireDeferClock`. Outer `WHERE` wakes `status = 'deferred' AND defer_until IS NOT NULL AND datetime(defer_until) <= datetime('now')` (`sqlite.rs:10381`), but the wave-gate `NOT EXISTS` subquery (`sqlite.rs:10441`) checks `lower_wave.status IN ('open', 'in_progress')`, ignoring the woken `status = 'deferred'` Wave 1 issue and releasing Wave 2 alongside Wave 1. | **VERIFIED & PASSING**. Lower wave gate includes expired timed deferrals (`defer_until <= now`). Regression witness: `tests/storage_ready.rs::ready_holds_wave_2_while_wave_1_timed_deferral_expired`. |
| `ExplicitBlockedVisibleInBlockedJson` | Whenever a non-closed issue has `status = 'blocked'`, `br blocked --json` includes it. | **VIOLATED in 1 step**: `SetExplicitBlockedStatus`. `src/cli/commands/blocked.rs:114-120` short-circuits `--json` and `--format toon` when `!storage.may_have_blocked_command_results()?`, which checks `EXISTS(SELECT 1 FROM blocked_issues_cache LIMIT 1)` (`sqlite.rs:11878`) without checking `EXISTS(SELECT 1 FROM issues WHERE status = 'blocked' LIMIT 1)`. | **VERIFIED & PASSING**. Probe includes `EXISTS(SELECT 1 FROM issues WHERE status = 'blocked' LIMIT 1)`. Regression witness: `tests/storage_ready.rs::may_have_blocked_command_results_detects_explicit_blocked_issue`. |

## Coverage Map

| Model Element | Rust Symbol | File & Lines |
|---------------|-------------|--------------|
| `W1InReady`, `W2InReady` | `SqliteStorage::build_ready_issue_candidates_query` | `src/storage/sqlite.rs:10369-10449` |
| `LowerWaveGateActive` | `NOT EXISTS (SELECT 1 FROM issues lower_wave ...)` | `src/storage/sqlite.rs:10435-10448` |
| `MayHaveBlockedCommandResults` | `SqliteStorage::may_have_blocked_command_results` | `src/storage/sqlite.rs:11864-11909` |
| `W1InBlockedJson` | `execute` (`br blocked --json`) + `get_blocked_issues_with_projection` | `src/cli/commands/blocked.rs:114-127`, `src/storage/sqlite.rs:11807-11836` |
| `DeferW1Timed`, `DeferW1Manual`, `UndeferW1` | `execute_defer` / `execute_undefer` | `src/cli/commands/defer.rs:94-210` |

## What the Model Leaves Out

- `external:` dependency polling (`has_external_dependencies` branch in `may_have_blocked_command_results`).
- Priority/created_at sorting within a wave and `--limit` pagination.
- Epics, tombstones, and parent-child transitive blocking in `blocked_issues_cache`.

## Maintenance

When `src/storage/sqlite.rs` or `src/cli/commands/blocked.rs` changes, use `/maintain-formal-verification-skill` to update `models/BeadsReadyAndBlocked.tla` and re-run `/statechart` to verify both `.cfg` and `.fixed.cfg`.
