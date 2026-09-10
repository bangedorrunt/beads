# AGENTS.md — beads (bangedorrunt's flywheel × toron work-ledger)

> **Governing decision:** [ADR-0001](docs/decisions/0001-make-beads-the-fail-closed-work-ledger.md).
> This is **bangedorrunt's fork of `beads_rust`** — a dedicated work-ledger
> built to maximize the flywheel × toron experience. It no longer shares the
> upstream author's vision: the fork exists for the fail-closed ledger,
> wave-gated dispatch, and the `bd-###` coordination contract, and is the
> authority on its own semantics. The upstream lineage (`Dicklesworthstone/beads_rust`,
> fast-forward `9c45f79a`, 2026-08-21) is historical — a source of fixes to
> cherry-pick on request, never a direction to follow. Close is fail-closed.
> Binary remains `br`. Do not add MCP tools. Do not run git from `br` — the only exception is the explicit, user-invoked `br vcs-status` diagnostic, which never runs automatically and never mutates state.
>
> **Coordination contract (this fork, flywheel × toron):** bead IDs are `bd-###`
> (the configured `issue_prefix`). The bead ID is the shared key across the
> stack: toron mail `--thread` = bead ID, reservation `--reason` = bead ID,
> commit message cites the bead ID. Close is **one report, two channels**:
> commit BEFORE close with the bead ID in the message → `br gate report` +
> `br close --commit-sha <sha>` (the durable, ledger-gated copy) → mail the
> captain `[{id}] done` on `thread_id=<id>` (the captain's copy; the loop's
> dispatch-guard reads the `] done` subject as a dedupe hold, and the loop
> sends its own verified `[{id}] done` since 2026-08-18). `--as`/`--to` are
> pins (adjective+noun), never the host username; mail to the captain is
> always `--to captain`. Full mail/reserve args live in the toron skill —
> do not restate them here.
> **Upstream:** do not fetch/merge/cherry-pick from `Dicklesworthstone/beads_rust` unless the captain says **fork sync**. Then review and take only commits this fork still needs. Never merge `upstream/main` wholesale. See **RULE 2**.
>
> Guidelines below are THIS FORK's rules (inherited from upstream, now
> bangedorrunt's own). They serve the flywheel × toron contract, not upstream.
> Where anything below conflicts with ADR-0001, RULE 2, or this banner's
> coordination contract, **this fork wins** — upstream opinion is irrelevant.

## FORK SYNC IS OPT-IN, CHERRY-PICK ONLY

This repo is a **hard fork**. Default: ignore `Dicklesworthstone/beads_rust`. Do not "stay current."

**Do nothing** with the parent until the captain's message contains **fork sync** (same intent: "sync the fork", "sync upstream"). A drive-by `git fetch upstream` plus merge is a policy violation.

When **fork sync** is requested, do this and nothing else:

1. `git fetch upstream` (remote `upstream` = `https://github.com/Dicklesworthstone/beads_rust.git`).
2. List commits `HEAD..upstream/main` (or since the last recorded baseline SHA in this banner / ADR-0001).
3. Classify **every** commit as `TAKE` or `SKIP` with one line why. No silent drops.
4. **TAKE** only a commit that fixes a defect this fork still has: storage/WAL/lock honesty, `integrity_check` vs migrate-schema, EPIPE/`SIGABRT` on closed pipes, JSONL flush/hash that can exit 0 on a skipped write, doctor lying about health, schema correctness for tables we still use.
5. **SKIP** (always): MCP/FastMCP, GitHub/Claude/Codex plugin install, `br agents --add` / AGENTS.md writer, `bd` migration, capacity exemptions, changelog-as-product, CLI growth, worktree-as-a-feature, generic tracker UX, anything on ADR-0001 Forbidden. Mixed commits (needed fix + skipped feature) are **SKIP**; note the SHA so a later split can be considered.
6. If `TAKE` is empty: stop. Report that. Do not merge.
7. If `TAKE` is non-empty: cherry-pick those SHAs onto `main`, one commit at a time, in parent order. Resolve conflicts toward **our** close/ready/gate/schema-18 semantics. Never `git merge upstream/main`. Never rebase this fork onto upstream.
8. After the cherry-picks: run the smallest relevant proof (`mbx test` on the touched modules, or `br doctor` in a scratch dir). Report TAKE/SKIP lists, new HEAD, and leftover parent bugs we still do not want.

Baseline: founding fast-forward `9c45f79a` (2026-08-21); fork-sync TAKEs absorbed through upstream `34ca862b` (2026-09-01 era, comment-ID reject). Update the banner SHA only when a fork-sync TAKE actually lands.

---
## CI/Release Workflow Supply-Chain Policy

For any `.github/workflows/` edit, use
[`docs/CI_SUPPLY_CHAIN.md`](docs/CI_SUPPLY_CHAIN.md) as the canonical policy.
It defines the immutable external GitHub Action pin inventory, upstream update
audit, workflow-fragment harnesses, branch-trigger expectations, and proof
commands for workflow changes.

Important boundaries:

- `br` never performs workflow git operations, releases, pull requests, network
  dispatches, or upstream lookups automatically.
  verifier scripts are operator shortcuts and may call Cargo internally.
- Whole-crate `mbx check --all-targets` and
  `mbx clippy --all-targets -- -D warnings` are required when Rust code
  changes 
- Run `git diff --check`, `actionlint` when available, the relevant workflow
  harnesses

---

## Toolchain: Rust & Cargo

We only use **Cargo** in this project, NEVER any other package manager.
- **Runner:** mbx is the cargo cache: prefix every cargo invocation with `mbx` (bare `cargo` misparses under the mbx shim).

- **Edition:** Rust 2024 (nightly required — see `rust-toolchain.toml`)
- **Dependency versions:** Explicit versions for stability
- **Configuration:** Cargo.toml only (single crate, not a workspace)
- **Unsafe code:** Forbidden (`#![forbid(unsafe_code)]` via crate lints)

Key dependencies, the release profile, and feature flags live in `Cargo.toml` — read them there, never from a copy in this file.


---

## Compiler Checks (CRITICAL)

**After any substantive code changes, you MUST verify no errors were introduced:**

```bash
# Check for compiler errors and warnings
mbx check --all-targets

# Check for clippy lints (pedantic + nursery are enabled)
mbx clippy --all-targets -- -D warnings

# Verify formatting
mbx fmt --check
```

If you see errors, **carefully understand and resolve each issue**. Read sufficient context to fix them the RIGHT way.

---

## Testing

### Testing Policy

Every module includes inline `#[cfg(test)]` unit tests (happy path, edge cases, error conditions). Integration, conformance, property, and regression tests live in `tests/`; shared fixtures in `tests/fixtures/` and `tests/common/`; benches in `benches/`.

### Unit Tests

```bash
# Run all tests
mbx test

# Run tests for a specific module
mbx test storage

# Run tests with all features enabled
mbx test --all-features
```

---

## Third-Party Library Usage

Unsure how a third-party library works: fetch current docs via Context7 MCP first, then `chie qmd search --agent`; web search only if both return nothing.

---

## beads_rust (br) — This Project

**This is the project you're working on.** beads_rust is an agent-first, dependency-aware issue tracker CLI (`br`) that stores issues in SQLite with JSONL export for git-based sync. It is a Rust port of the classic Go beads issue tracker (`bd`), designed to be non-invasive (no automatic git operations, no daemons, no hooks).

### What It Does

Provides lightweight issue tracking with dependency graphs, priority-based triage, content-addressed deduplication, and multiple output modes (rich terminal, plain text, JSON, TOON). Designed specifically for AI coding agents to select "ready work," manage task dependencies, and coordinate via structured robot output.

### Where to look (code pointers — the tree itself is `ls`)

| Task | Location |
|------|----------|
| CLI dispatch, output-mode detection | `src/main.rs`, `src/cli/mod.rs` |
| Subcommands | `src/cli/commands/*.rs` |
| Types (`Issue`, `Dependency`, `Comment`, `Event`, `Label`), hashing | `src/model/mod.rs` |
| SQLite engine, schema, audit log, queries | `src/storage/{sqlite,schema,events}.rs`, `src/storage/queries/` |
| JSONL import/export, path discovery, history | `src/sync/{mod,path,history}.rs` |
| Layered config + project routing | `src/config/{mod,routing}.rs` |
| Errors + exit codes | `src/error/{mod,structured,context}.rs` |
| Output modes, themes, widgets | `src/format/*`, `src/output/*` |
| Validation | `src/validation/mod.rs` |
| IDs, hashes, time, spinners, md import | `src/util/{id,hash,time,progress,markdown_import}.rs` |
| TUI | `src/tui/{mod,app,ui,keys,theme}.rs` (`keys.rs:34` `REGISTRY` is the binding doc) |
| Tests | inline `#[cfg(test)]` + `tests/` (e2e, conformance, storage, proptest, repro, jsonl, md import) |

### Fork semantics (what makes this not-upstream)

- **Non-invasive** — `br` never runs git; all git ops are explicit user actions (only exception: user-invoked `br vcs-status` diagnostic, read-only).
- **SQLite + JSONL hybrid** — SQLite is primary; JSONL export is the git-sync format.
- **Content-addressed dedup** (SHA-256) and **hash-based short IDs** (`bd-###` shape).
- **Append-only audit log** — every mutation recorded.
- **Fail-closed close** — gate row + citing sha, per the banner and the br skill.

## VERIFY Fence Honesty (legal-close interaction)

A VERIFY fence that is a single loop-runnable command (`cargo test ...`, `timeout ...`) legally closes ONLY via `command-verified` (row 1). If you actually verified via unit tests, the fence must state the composed commands you really ran — the ledger checks the fence shape, and a bare runnable line makes `unit-test-verified` an illegal close. (Lesson: bd-2mdo, 2026-08-23 — first fence was refused by ledger check until it matched reality.)

## The Close Ceremony (fail-closed + captain copy)

Authoritative steps live in the br skill ("Claim → work → close → report"); this file keeps no second copy. Order is load-bearing: first-run bootstrap (toron identity + reservation, `AGENT_NAME=<pin>` on every commit) → commit BEFORE close with the bead id in the message → `br gate report` + `br close --commit-sha` (durable copy) → mail the captain `[{id}] done` on the bead thread (captain's copy) → `br sync --flush-only`.

---

## Sync Safety Maintenance

When modifying sync-related code (`src/sync/`, `src/cli/commands/sync.rs`), you MUST follow the maintenance checklist:

**See: [`docs/SYNC_MAINTENANCE_CHECKLIST.md`](docs/SYNC_MAINTENANCE_CHECKLIST.md)**

Quick summary:
1. **No git operations** — Static check: `grep -rn 'Command::new.*git' src/sync/`
2. **Path allowlist** — Verify only `.beads/` files are touched
3. **Run safety tests** — `mbx test e2e_sync --release`
4. **Review logs** — Check for unexpected safety events
5. **Update docs** — If behavior changed

Related documentation:
- [SYNC_SAFETY.md](docs/SYNC_SAFETY.md) — User-facing safety model
- [E2E_SYNC_TESTS.md](docs/E2E_SYNC_TESTS.md) — Test execution guide
- [.beads/SYNC_SAFETY_INVARIANTS.md](.beads/SYNC_SAFETY_INVARIANTS.md) — Technical invariants

---

## Output Modes

Modes (Rich / Plain / JSON / Toon / Quiet) and the agent flag rule (always `--json` or `--robot`; bare `br` is the human TUI) live in the br skill. Schema discovery: `br schema all --format json`. Agent integration defaults: [docs/AGENT_INTEGRATION.md](docs/AGENT_INTEGRATION.md).

---

## TUI — bare `br` (hacker-night, tui-design skill)

Bare `br` in a TTY opens the interactive dashboard; agents never run it (use `br --robot-triage` / `br triage|next|plan` with `--format json|toon`). Code: `src/tui/{mod,app,ui,keys,theme}.rs`; theme single source is `src/tui/theme.rs` (`HackerNight`, semantic slots only); `keys.rs:34` `REGISTRY` is the binding doc. Consult the tui-design skill before touching TUI code; verify with `mbx test --lib` plus manual runs at 80×24 / 120×40 / 200×60, in tmux, with `NO_COLOR=1` and `COLORTERM=truecolor`.

