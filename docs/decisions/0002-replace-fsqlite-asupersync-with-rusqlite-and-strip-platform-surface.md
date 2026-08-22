<!-- governed-by: ADR-0001 -->
---
status: accepted
date: 2026-08-22
decision-makers: Captain (bangedorrunt)
consulted: Cargo.toml dependency audit 2026-08-22; src/franken_sync.rs bridge analysis; ADR-0001 §5.8/§10-D storage clause
informed: flywheel maintainers; toron maintainers (consumers of the `br` binary only; no interface change)
---

# ADR-0002: Replace fsqlite/asupersync with rusqlite and strip platform surface

**Status:** Accepted (2026-08-22, captain).
**Amends:** [ADR-0001](0001-make-beads-the-fail-closed-work-ledger.md) — §5.8 ("Wave 1 keeps fsqlite"), §8.2 ("No new crates for wave 1"), §6 Non-goals ("Replacing fsqlite"), and §10 Option D's rejection clause. The engine swap is re-routed from "wave 4 at the earliest, maybe never" to a **prerequisite wave** executed before ADR-0001 wave 1, so schema-18 work lands on maintainable ground.
**Does not amend:** ADR-0001's close/ready/gate semantics, schema versioning plan, JSONL-in-git, binary name `br`, "br never runs git".

## 1. TL;DR

The fork currently depends on **16 niche crates** to store rows: the `fsqlite` family (15 crates: fsqlite, -types, -error, -core, -func, -vdbe, -vfs, -pager, -parser, -planner, -wal, -btree, -ast, -mvcc, -observability) plus `asupersync` (exact-pinned =0.4.8). fsqlite 0.3 is an async pure-Rust SQLite reimplementation whose `!Send` futures force a hand-maintained `block_on` bridge (`src/franken_sync.rs`, 502 lines) inside an otherwise fully synchronous CLI. The captain has no bandwidth to watch these crates.

**Decision:** swap the engine to `rusqlite` (bundled, synchronous, the de-facto mainstream Rust SQLite binding), delete the bridge and asupersync entirely, and strip the platform/feature surface this fork never needed:

1. `fsqlite` family + `asupersync` → `rusqlite` with `bundled`. `src/franken_sync.rs` is deleted; its compat module already speaks rusqlite-style API shapes, so the port is mechanical in intent even where it is large in lines.
2. Delete the `mcp` feature: `src/mcp/`, `fastmcp-rust`, and the asupersync dev-dependency (`test-internals`). This pulls forward the MCP deletion ADR-0001 §5.9 already scheduled for wave 5 and re-affirms §6 Non-goals ("Beads MCP").
3. Drop Windows as a target: remove `[target.'cfg(windows)'.dependencies]` (`cap-primitives`) and Windows-pathing accommodations. macOS + Linux are the supported hosts (captain's statement, 2026-08-22). Parent issues #419/#413 stay closed-not-ours.
4. Remove declared-but-unused deps found in the audit: `mimalloc` (zero references in `src/`), and replace `dunce` with `std::fs::canonicalize` on unix-only builds.
5. No async runtime enters this crate. `br` stays synchronous end-to-end. tokio was considered and rejected: it would add exactly the maintenance surface this ADR removes.

## 2. Context

Measured 2026-08-22:

| Fact | Evidence |
| :--- | :--- |
| Storage layer is fully synchronous | `franken_sync.rs` module doc: "br's storage layer is fully synchronous"; every future created, polled, dropped within one bridge call |
| asupersync exists only for fsqlite + mcp | Direct uses: `franken_sync.rs` (bridge driver) and `src/mcp/mod.rs` (`build_serve_cx`); dev-dep enables `test-internals` |
| fastmcp-rust exact-pins asupersync | Cargo.toml L80–83 comment |
| fsqlite features buy nothing here | No FTS5/MATCH usage anywhere in `src/storage/`; search is LIKE-based; `fts5`/`icu`/`rtree` features enabled but unused |
| Windows cfg surface in code: zero | No `cfg(windows)` / `target_os = "windows"` matches under `src/`; Windows exists only in the dependency table |
| `cap-primitives`, `mimalloc`: zero direct uses | grep over `src/` returns nothing |
| Blast radius of the swap | `src/storage/sqlite.rs` = 34,939 lines, 74 fsqlite touchpoints across 25 files |

Driver: maintenance burden. Every fsqlite-family bump is a 15-crate coordinated upgrade against a niche project; asupersync is pinned to satisfy two consumers; neither has the maintenance budget rusqlite enjoys (SQLite-recommended binding, enormous downstream).

## 3. Semantic mapping (the part that needs care, not vibes)

fsqlite-specific behaviors the port must reproduce or consciously drop:

| fsqlite behavior (franken_sync.rs) | rusqlite equivalent |
| :--- | :--- |
| `BusyRecovery` bounded retry window | SQLite busy handler covers `SQLITE_BUSY_RECOVERY`; configure `busy_timeout` and drop the caller-side retry |
| `BusySnapshot` first-committer-wins retry in autocommit | WAL mode gives snapshot isolation; keep the transaction-retry contract at callers that had it |
| Stale-schema refresh via forced `prepare()` | Not needed: real SQLite connections see committed DDL immediately |
| Thread-local runtime + reentrancy guard | Deleted outright — sync calls need no driver |
| MVCC concurrent-writer semantics | Plain SQLite writer locking. Swarm write bursts are already serialized by `.write.lock` + inode lock (the sanctioned `unsafe`); acceptable per ADR-0001 §3 |
| `Row` / `SqliteValue` / `FrankenError` re-exports | Map to `rusqlite::Row` / `types::Value` / `rusqlite::Error`; public surface of `br` does not export them (verify with a compile gate) |

WAL mode, `PRAGMA user_version` (schema stamping), prepared statements, explicit transactions, and `integrity_check` are all first-class in rusqlite — nothing about ADR-0001 wave 1 (schema 18, fail-closed close, gates.jsonl) changes shape.

## 4. Waves

| Wave | Outcome | Gate proof |
| :--- | :--- | :--- |
| **W0** | This ADR accepted; bead graph filed; tracker committed | beads exist with deps/waves |
| **W1** | `mcp` feature deleted (`src/mcp/`, fastmcp-rust, asupersync dev-dep) | `cargo build` default features green; no `asupersync` outside franken_sync |
| **W2** | Engine swap: rusqlite in, fsqlite family out, `franken_sync.rs` deleted | full test suite green; `cargo tree` shows no `fsqlite*`/`asupersync`; `br doctor` clean on fixture workspace |
| **W3** | Platform/dead-dep strip: cap-primitives, mimalloc, dunce→std; dep audit (cargo-machete); docs updated | `cargo tree` shows none of the removed crates; AGENTS.md dependency table matches Cargo.toml |
| **W4** | Verify gate: full suite + clippy + fmt + e2e + conformance; binary installed to `~/.local/bin/br`; ADR index updated | all gates exit 0 from clean HEAD |

Waves run one at a time. W2 is the linchpin: single-writer on `src/storage/**`.

## 5. Non-goals

* No tokio or any async runtime.
* No JSONL format change, no schema-version change, no CLI surface change beyond deleting `serve`.
* No change to rich_rust / toon_rust (sibling projects, maintained by us, out of scope today).
* No upstream sync (AGENTS.md RULE 2 stands).

## 6. Consequences

* Good: 16 niche pins → 1 mainstream crate (+ libsqlite3-sys bundled build).
* Good: `franken_sync.rs` (502 lines of bridge machinery + tests) deleted; reentrancy/BusyRecovery folklore dies with it.
* Good: release build sheds icu/fts5/rtree compile cost and the serde_json `arbitrary_precision` unification wart documented in Cargo.toml L45–53.
* Bad: we inherit plain-SQLite concurrency semantics; if swarm bursts ever corrupt under rusqlite locking the way parent #426 did, the fix surface is standard SQLite tuning, not engine forks.
* Bad: one large mechanical diff over 35k lines lands before ADR-0001 wave 1 — rebase risk for anything else in flight; mitigated by wave serialization.
* Neutral: `bv` unaffected (separate repo).

## 7. Verification

- [ ] `cargo tree --edges normal | rg "fsqlite|asupersync|fastmcp|cap-primitives|mimalloc|dunce"` → no matches
- [ ] `test ! -e src/franken_sync.rs && test ! -e src/mcp`
- [ ] `cargo clippy --all-targets -- -D warnings` green
- [ ] `cargo test` green (unit + integration + conformance + e2e)
- [ ] `br doctor` clean on a fixture `.beads/` workspace
- [ ] Freshly built binary installed to `~/.local/bin/br`; `br version` reports the new build
- [ ] AGENTS.md dependency table updated; ADR index lists 0002 accepted
