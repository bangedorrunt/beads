<!-- governed-by: ADR-0004 -->
---
status: accepted
date: 2026-09-04
decision-makers: Captain (bangedorrunt)
consulted: flywheel × toron × br architecture review; gastownhall/beads upstream source, documentation, issues, pull requests, and discussions reviewed 2026-09-04
informed: flywheel maintainers; toron maintainers
---

# ADR-0004: Adopt revisioned witnessed mutations for the flywheel × toron × br stack

**Status:** Accepted (2026-09-04, Captain).
**Extends:** [ADR-0001](0001-make-beads-the-fail-closed-work-ledger.md), the fail-closed work-ledger decision.
**Constrained by:** [ADR-0002](0002-replace-fsqlite-asupersync-with-rusqlite-and-strip-platform-surface.md), the synchronous bundled-rusqlite storage decision.
**Related:** [ADR-0003](0003-absorb-beads-viewer-into-br.md), the proposed single-binary robot/TUI decision.

> This ADR records the stability, robustness, performance, and coordination lessons learned from `gastownhall/beads`, and converts the adoptable lessons into an implementation contract for this fork. It deliberately does not adopt Dolt, Gas Town, a beads daemon, a new Nostr kind, or a second lease authority.

## 1. Decision summary

Adopt a **revisioned, witnessed mutation protocol** at the existing SQLite/JSONL boundary:

1. Every durable issue mutation has a monotonic issue revision.
2. Claim, unclaim, reclaim, status transition, assignee change, gate authorization, and close can require an expected revision and fail with a structured stale-write conflict.
3. Gate validation, close metadata, issue status, event, dirty marking, and the close witness are committed atomically in one SQLite transaction.
4. Issues, gates, and their hashes are published as one export generation with a manifest; readers distinguish complete, stale, and incomplete publication.
5. Incremental JSONL export may patch only a proven small dirty set, but it must use the same canonical serializer and filters as full export and fall back conservatively.
6. Workspace identity and schema safety checks refuse dangerous “empty replacement” and forward-schema states before writable work begins.
7. `br doctor --bundle` and `br capabilities --json` expose reproducible evidence and machine-readable protocol versions.
8. Toron remains the owner of reservations, heartbeats, mail, and liveness. Flywheel remains the owner of dispatch and VERIFY execution. `br` remains local, synchronous, non-networked, and never runs git.

The desired causal chain is:

```text
                 durable proof / publication
                              │
                              ▼
┌─────────────┐   reserve   ┌──────────┐   CAS claim   ┌───────────┐
│   toron     │─────────────▶│ flywheel│──────────────▶│    br     │
│ mail/lease  │              │ dispatch│               │ SQLite    │
│ liveness    │◀─────────────│ VERIFY  │◀──────────────│ gates     │
└─────────────┘  heartbeat   └──────────┘  result       │ JSONL     │
                                                        └─────┬─────┘
                                                              │
                                          generation manifest │
                                                              ▼
                                                        git-visible
                                                        interchange
```

## 2. Problem statement

The stack has three cooperating authorities, but stale observations and partial publication can make their views disagree:

| Authority | Owns | Must not own |
|---|---|---|
| `br` | durable issue state, dependencies, events, legal gates, close witness, JSONL publication | Nostr, reservations, git commands, hidden daemons |
| toron | mail, pins, file reservations, lease/liveness, jobs | durable issue status or legal-close policy |
| flywheel | scheduling, waves, pane dispatch, VERIFY execution, verdict selection | a shadow ready predicate, markdown as the long-term schema, repair of illegal closes |

Observed and researched failure classes include:

- two agents claim from the same stale `open` observation;
- a reclaimed worker later closes an issue it no longer owns;
- gate and close are recorded in separate operations and crash between steps;
- a closed issue carries a verdict field but the proof is absent from the clone-visible export;
- an incremental exporter diverges from the full exporter and leaks private records, loses type discriminators, ignores owner filters, or races on a shared temporary path;
- a missing remote/server database is silently recreated as empty;
- an older binary writes against a newer schema and produces a misleading SQL error;
- ephemeral records are reported as successfully durable or silently disappear;
- a full database backend adds version, server, remote, bootstrap, and recovery state that duplicates responsibilities already owned by toron and git/JSONL;
- long lists and graph analysis load more state than the requested projection needs;
- users cannot produce a self-contained incident artifact for a failed overnight run.

## 3. Prior-art findings

### 3.1 Adopt directly as protocol principles

The following upstream lessons are compatible with this fork:

- **Refuse silent empty recreation.** Upstream PR [#5791](https://github.com/gastownhall/beads/pull/5791) fixed existing project metadata being treated as permission to create an empty replacement database. Its review also exposed that force/reinit aliases can bypass a guard unless the guard sits at the irreversible operation chokepoint.
- **Fail fast on forward schema drift.** Upstream PR [#4531](https://github.com/gastownhall/beads/pull/4531) added a clear schema-newer failure instead of allowing a stale binary to fail later on an unknown column. The guard must cover writable and read-only opens.
- **Use leases with TTL and heartbeat.** Upstream’s lease design uses expiry, heartbeat, holder identity, and a granting-replica identity. A dead worker stops renewing; reclaim is bounded by expiry and authority.
- **Use an explicit conflict token for ownership races.** Upstream’s Dolt implementation rewrites a shared `row_lock` cell so status/ownership writers cannot merge silently. This fork uses a simpler deterministic integer CAS revision because SQLite already serializes writers.
- **Separate ephemeral liveness from durable issue state.** Upstream federation documents that leases are replica-local while status/assignee visibility is durable and replicated. This maps naturally to toron-owned reservations and `br`-owned issue state.
- **Make alternate performance paths preserve the full contract.** Upstream PR [#5806](https://github.com/gastownhall/beads/pull/5806) measured a compelling incremental-export speedup, but its review found production-path and contract failures: state hash versus commit identity, memory leakage, owner-filter drift, missing `_type`, and temporary-file races. The optimization is worth adopting only with stronger differential proofs.
- **Use bounded, explicit capability and health output.** Versioned envelopes, structured degradation, and explicit recovery state are safer than optimistic text.
- **Use compact projections for agents.** Upstream issue [#690](https://github.com/gastownhall/beads/issues/690) supports TOON for token-efficient projections. This fork already has TOON, so it remains a projection rather than the sync format.

### 3.2 Reject or adapt, with reasons

#### Dolt as the primary backend — rejected

Upstream’s [Dolt architecture](https://raw.githubusercontent.com/gastownhall/beads/main/docs/architecture/dolt.md) provides cell-level merge, database history, branches, server multi-writer mode, remotes, and federation. Those capabilities are attractive, but adopting them here would add:

- embedded/server mode state;
- server startup, ports, sockets, and liveness;
- Dolt version pinning and engine-specific recovery;
- working-set, commit, reset, remote, bootstrap, and migration state;
- a second federation mechanism beside toron;
- a larger operational surface that is harder to explain and recover.

The upstream [sync model](https://raw.githubusercontent.com/gastownhall/beads/main/docs/core-concepts/sync-concepts.md) explicitly makes JSONL non-canonical, while this fork intentionally keeps JSONL as the git-visible interchange. ADR-0002 already selected bundled rusqlite. Therefore this fork borrows the protocols, not the backend.

#### A second lease system inside `br` — rejected

`br` must not duplicate toron reservations. It may persist claim metadata and enforce expected revisions, but it must not grant, renew, inspect, or reclaim toron leases. Missing mail evidence is never proof of abandonment.

#### Automatic daemon or hidden queue — rejected

A bounded retry loop and explicit conflict result are preferable to a background process. A queue may be reconsidered only after measured contention proves that bounded retries are insufficient.

#### TOON as durable interchange — rejected

TOON is useful for model-facing output but JSONL remains the mergeable, inspectable, git-visible interchange contract.

## 4. Invariants

The following must hold after implementation:

### Authority and mutation invariants

1. A durable status/ownership mutation either commits completely or leaves no durable partial mutation.
2. A caller supplying an expected revision can only mutate the exact revision it observed.
3. A stale caller cannot overwrite a newer assignee, status, close, gate, or claim.
4. A legal close has a matching legal PASS gate, commit SHA, close verdict, event, and revision witness.
5. A failed or ambiguous publication is reported as pending/incomplete, never as fully published.
6. Toron is the only reservation/heartbeat/reclaim authority; `br` never invents absence-of-mail evidence.

### Storage and publication invariants

7. A writable open refuses a database whose schema is newer than the binary.
8. An existing workspace is never silently recreated as an empty database, including force/reinit aliases.
9. Incremental export is semantically equivalent to full export under the same filters.
10. A publication generation is accepted only when all required files match its manifest hashes.
11. Any uncertain manifest, dirty set, serializer, or filter state takes the conservative full-export path or fails closed.
12. Temporary files are process-scoped, same-directory, no-clobber, and atomically published.

### Operational invariants

13. Every structured mutation conflict includes issue ID, expected revision, actual revision when available, and retryability.
14. Every protocol envelope exposes its schema/contract version.
15. Health states remain explicit: unknown is not healthy; recoverable and unsafe states block the operations documented in `HEALTH_CONTRACT.md`.
16. Incident bundles contain enough evidence to reproduce classification without credentials or private toron content.

## 5. Canonical protocol

### 5.1 Issue revision

Add a non-null integer `revision`/`issue_revision` to the durable issue row. It starts at 1 for a created issue and increments exactly once for each committed durable issue mutation. Timestamps remain human metadata; revision is the concurrency token.

Commands that mutate ownership or status accept an optional `--expected-revision N`. When present, the storage update includes the revision predicate. Zero affected rows produce a structured stale-revision conflict. When absent, existing local operator behavior may remain available where policy allows, but flywheel-issued claim/close commands must supply it.

The JSON witness includes:

```json
{
  "issue_id": "bd-example",
  "previous_revision": 12,
  "revision": 13,
  "mutation": "claim",
  "conflict": null
}
```

### 5.2 Atomic witnessed close

The preferred close path is one transaction:

```text
validate target transition
  → load issue + current revision
  → validate commit SHA shape and legal gate
  → validate expected revision
  → append transition-scoped gate result/history
  → update closed status, close_verdict, commit_sha, closed_at
  → append event
  → mark issue dirty
  → commit
```

The public command may retain a compatibility two-command gate-report flow while the atomic helper is introduced, but the helper is the canonical path for flywheel. A successful database commit followed by a failed JSONL publication must return a distinct “durable DB / publication pending” result.

### 5.3 Publication generations

The publication unit is:

```text
issues.jsonl
 gates.jsonl
 manifest.json
```

The manifest records at minimum:

```json
{
  "format": "br.publication.v1",
  "generation": 42,
  "schema_version": 18,
  "issues_sha256": "...",
  "gates_sha256": "...",
  "issues_line_count": 120,
  "gates_line_count": 18,
  "source_revision": 987,
  "filters": {
    "include_ephemeral": false,
    "include_memories": false,
    "excluded_owners": []
  }
}
```

Readers classify the publication as `complete`, `stale`, `incomplete`, or `unknown`. A close witness that requires clone-visible proof is not complete until its gate is represented in the accepted generation.

### 5.4 Incremental export

Incremental export is allowed only when:

- a prior valid manifest exists;
- the dirty set is known and below a configured threshold;
- the existing target is a regular file with a valid route;
- canonical serialization and all filters are available;
- no schema/filter/format generation changed.

Otherwise use full export. The incremental path must:

- include the same issue discriminator and record shape;
- preserve owner, memory, ephemeral, tombstone, and template filters;
- handle deletions;
- preserve untouched lines byte-for-byte where safe;
- write via the existing pinned atomic publication primitives;
- verify normalized semantic equivalence against a full-export oracle in tests.

## 6. Workspace and schema safety

The storage/open boundary must classify project identity before creation:

| Evidence | Action |
|---|---|
| no project metadata and no DB | create a new workspace |
| metadata + JSONL, DB absent | explicit rebuild/import path; do not pretend it is empty |
| metadata, DB absent, JSONL absent | refuse and direct operator to restore/backup |
| DB path exists but is not SQLite | refuse and report/quarantine |
| schema version newer than binary | refuse all writable operations with structured upgrade error |
| fresh DB before schema bootstrap | allow initialization, then verify schema/integrity |

The safety check must execute below aliases such as `--force`, `--reinit`, or recreate-style flags. Legitimate recreation requires an explicit operator-only confirmation or token and must be visibly distinct from normal initialization.

## 7. Responsibilities across repositories

### This repository (`br`)

Implement:

- issue revision storage and structured stale conflicts;
- CAS-aware claim/status/close paths;
- atomic witnessed close;
- publication manifest and gate/issue generation verification;
- conservative incremental export using existing dirty/witness infrastructure;
- missing-workspace and forward-schema guards;
- doctor bundle and capabilities output;
- tests, schemas, docs, and code↔ADR links.

### Flywheel follow-up (documented, not edited here)

- call `br ready --json` as the sole dispatchable set;
- acquire toron reservation before CAS claim;
- carry returned revision through execution;
- heartbeat toron within TTL;
- report VERIFY result to the correct gate;
- use atomic witnessed close with expected revision and commit SHA;
- remove markdown VERIFY/PRINCIPLES parsing after all consumers migrate.

### Toron follow-up (documented, not edited here)

- preserve bead ID as mail thread ID;
- keep reservation holder/path data authoritative;
- enforce TTL, heartbeat cadence, and granting-store identity;
- make reclaim require local lease authority or an explicit operator override;
- never treat missing `br` data as permission to steal work.

Recommended cross-system flow:

```text
br ready --json
  → toron reserve(thread=bead-id, reason=bead-id)
  → br claim --expected-revision N --pin PIN
  → flywheel dispatch + toron heartbeat
  → br gate report / br close-witnessed
  → br sync --flush-only
  → git commit by operator/agent, never by br
```

## 8. Implementation plan

### 8.1 Affected paths

- `src/model/mod.rs` — revision field and serialized witness types.
- `src/storage/schema.rs` — additive revision column, manifest metadata, migration and forward-drift checks.
- `src/storage/sqlite.rs` — CAS predicates, atomic transaction helpers, mutation results.
- `src/storage/db.rs` — structured conflict/error mapping where needed.
- `src/cli/mod.rs` and `src/cli/commands/{create,update,close,gate,init,sync,doctor}.rs` — flags and commands.
- `src/close_policy.rs` — close witness integration.
- `src/sync/{mod,path,witness}.rs` — manifest, generation verification, incremental/full equivalence.
- `src/health.rs` and `docs/reliability/HEALTH_CONTRACT.md` — new anomaly/publication states.
- `src/schema.rs` or the existing capability/schema command surface — protocol manifest.
- `tests/e2e_concurrency.rs`, `tests/e2e_sync_failure_injection.rs`, `tests/e2e_sync_reconcile.rs`, and focused new tests — executable proof.
- `docs/COORDINATION_EVIDENCE.md`, `docs/SYNC_SAFETY.md`, and this ADR — operational contracts.

### 8.2 Implementation order

1. Add revision schema/model serialization and migration with forward-drift tests.
2. Add storage CAS primitives and structured stale conflicts.
3. Wire claim/status/close commands; implement atomic witnessed close.
4. Add publication manifest and generation verification for issues plus gates.
5. Wire conservative incremental export and differential tests.
6. Add workspace identity guard, doctor bundle, and capabilities manifest.
7. Update operational docs and leave explicit flywheel/toron integration contracts.
8. Run the full Rust verification fence and inspect the final diff.

### 8.3 Patterns to follow

- Existing `with_write_transaction`/SQLite transaction boundaries.
- Existing `GateResultRecord` transition scoping and `status_revision` history.
- Existing `AnomalyClass`/`WorkspaceHealth` taxonomy.
- Existing pinned JSONL route and atomic publication helpers.
- Existing witness/chunk hashing and `dirty_issues`/`export_hashes` tables.
- `--json` for all agent-facing output; TOON only as a projection.
- `thiserror`/structured error codes; no string-only conflict signaling.
- Inline unit tests plus e2e/failure-injection tests.

### 8.4 Patterns to avoid

- Dolt, a new async runtime, a daemon, or a network client in `br`.
- A second lease/reservation authority.
- Full-export and incremental-export serializers that can drift.
- Fixed shared `.tmp` paths.
- Treating missing mail or missing database as healthy/empty.
- Retrying unknown transaction outcomes as if they were definitely uncommitted.
- Using timestamps as concurrency tokens.
- Auto-running git or silently committing publication files.
- Adding speculative abstractions before a concrete proof exists.

## 9. Verification checklist

### Revision and close

- [ ] Fresh issue starts with revision 1 and JSON round-trips it.
- [ ] Every durable issue mutation increments revision once.
- [ ] Two concurrent claims produce exactly one success.
- [ ] A stale claim cannot overwrite a newer assignee.
- [ ] A stale close cannot close a reclaimed or reassigned issue.
- [ ] Atomic witnessed close leaves gate, close fields, event, dirty marker, and revision consistent under injected failures.
- [ ] Structured stale conflicts contain issue ID, expected/actual revision, and retryability.

### Publication and sync

- [ ] Issues and gates publish with a manifest whose hashes verify.
- [ ] Partial/mismatched publication is classified as incomplete, never healthy.
- [ ] Incremental export is tested through the production auto-flush path.
- [ ] Incremental and full export are semantically equivalent across creates, updates, deletes, tombstones, owner filters, memories, ephemerals, labels, dependencies, comments, and gates.
- [ ] Missing manifest, large dirty set, changed filter, or uncertain state uses safe full export.
- [ ] Crash/failure injection leaves the previous valid generation readable.

### Safety and operations

- [ ] Existing project with missing DB is not silently recreated, including force/reinit aliases.
- [ ] Newer schema is rejected on writable and read-only open.
- [ ] `br doctor --bundle` excludes secrets/private toron content and includes required evidence.
- [ ] `br capabilities --json` exposes protocol and schema versions.
- [ ] `br ready --json` remains the only local dispatchability predicate.
- [ ] `br` contains no toron/flywheel dependency, network lease authority, or git command.
- [ ] TOON output remains a projection and JSONL remains durable interchange.

### Required commands

```bash
cargo fmt --check
cargo check --all-targets
cargo clippy --all-targets -- -D warnings
cargo test
cargo test --all-features
git diff --check
```

Relevant focused proofs:

```bash
cargo test --test e2e_concurrency -- --nocapture
cargo test --test e2e_sync_failure_injection -- --nocapture
cargo test --test e2e_sync_reconcile -- --nocapture
cargo test --test workspace_failure_replay -- --nocapture
```

## 10. Consequences

### Positive

- stale agents fail explicitly instead of silently overwriting newer work;
- close proof becomes one durable transaction and one inspectable witness;
- clones can verify issue and gate publication together;
- incremental export can scale without weakening full-export semantics;
- storage failures become actionable schema/identity errors;
- incident diagnosis becomes reproducible;
- toron, flywheel, and `br` each retain one clear authority;
- upstream’s useful concurrency lessons are adopted without Dolt’s operational state machine.

### Negative

- schema migration and protocol-version coordination are required;
- flywheel and toron consumers must migrate to expected-revision and witnessed-close flows;
- export manifests add files and recovery states;
- incremental export is a substantial proof burden and must remain conservative;
- some existing operator commands may remain less strict when no expected revision is supplied until consumer migration completes.

### Revisit triggers

Reconsider this ADR only if:

- SQLite write serialization cannot meet measured swarm latency despite bounded retry and batching;
- JSONL publication becomes a measured scalability limit after the witness-based exporter is proven;
- toron’s coordination authority changes materially;
- a future backend can provide equivalent safety with less operational complexity and without violating ADR-0001/0002.

## 11. Evidence and references

Upstream research:

- [Dolt architecture](https://raw.githubusercontent.com/gastownhall/beads/main/docs/architecture/dolt.md)
- [Sync concepts](https://raw.githubusercontent.com/gastownhall/beads/main/docs/core-concepts/sync-concepts.md)
- [Federation and lease rules](https://raw.githubusercontent.com/gastownhall/beads/main/docs/multi-agent/federation.md)
- [Lease implementation](https://raw.githubusercontent.com/gastownhall/beads/main/internal/storage/issueops/lease.go)
- [Missing database guard PR #5791](https://github.com/gastownhall/beads/pull/5791)
- [Forward schema-drift guard PR #4531](https://github.com/gastownhall/beads/pull/4531)
- [Incremental export PR #5806 and review](https://github.com/gastownhall/beads/pull/5806)
- [Ephemeral-record disappearance issue #2111](https://github.com/gastownhall/beads/issues/2111)
- [TOON issue #690](https://github.com/gastownhall/beads/issues/690)
- [Dolt usability discussion #2332](https://github.com/gastownhall/beads/discussions/2332)

Local evidence:

- [ADR-0001](0001-make-beads-the-fail-closed-work-ledger.md)
- [ADR-0002](0002-replace-fsqlite-asupersync-with-rusqlite-and-strip-platform-surface.md)
- [ADR-0003](0003-absorb-beads-viewer-into-br.md)
- [`HEALTH_CONTRACT.md`](../reliability/HEALTH_CONTRACT.md)
- [`COORDINATION_EVIDENCE.md`](../COORDINATION_EVIDENCE.md)
- `src/storage/db.rs`, `src/storage/schema.rs`, `src/storage/sqlite.rs`, `src/sync/`, `src/witness.rs`
- `tests/e2e_concurrency.rs`, `tests/e2e_sync_failure_injection.rs`, `tests/e2e_sync_reconcile.rs`

## 12. Code ↔ ADR linking

Key implementation entry points must carry `// governed-by: ADR-0004` or a nearby module-level reference. The ADR must be consulted before changing revision semantics, close witnesses, publication manifests, or workspace/schema safety checks.
