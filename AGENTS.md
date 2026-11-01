# beads (`br`)

bangedorrunt's hard fork of `beads_rust` — fail-closed work-ledger for the
flywheel × toron stack. Binary remains `br`. Governing decision:
[ADR-0001](docs/decisions/0001-make-beads-the-fail-closed-work-ledger.md).

Do not add MCP tools. Do not run git from `br` (exception: explicit user-invoked
`br vcs-status`, read-only). Upstream
`Dicklesworthstone/beads_rust` is historical — cherry-pick only on captain
**fork sync** (RULE below). Where this file conflicts with ADR-0001 or the
coordination contract, **this fork wins**.

## Coordination contract

Bead IDs are `bd-###` (`issue_prefix`). The id is the shared key across the
stack: toron mail `--thread`, reservation `--reason`, commit message citation.
Close is **one report, two channels**: commit BEFORE close with the bead id in
the message → `br gate report` + `br close --commit-sha <sha>` → mail captain
`[{id}] done` on `thread_id=<id>`. Pins are Adjective+Noun; mail the captain
`--to captain`. Full mail/reserve args: toron skill — do not restate them here.
Close ceremony detail: br skill.

## FORK SYNC IS OPT-IN, CHERRY-PICK ONLY

Do nothing with the parent until the captain's message contains **fork sync**.

1. `git fetch upstream` (`https://github.com/Dicklesworthstone/beads_rust.git`).
2. List `HEAD..upstream/main`.
3. Classify every commit `TAKE` or `SKIP` with one line why.
4. TAKE only defect fixes this fork still needs (storage/WAL/lock honesty,
   integrity vs migrate, EPIPE, JSONL flush/hash, doctor honesty, schema we use).
5. SKIP always: MCP/FastMCP, plugin install, `br agents --add` / AGENTS.md
   writer, `bd` migration, capacity exemptions, changelog-as-product, CLI growth,
   worktree-as-feature, generic tracker UX, ADR-0001 Forbidden. Mixed commits =
   SKIP.
6. Empty TAKE → stop. Non-empty → cherry-pick one-at-a-time in parent order.
   Never `git merge upstream/main`. Never rebase onto upstream.
7. Prove with the smallest relevant `mbx test` (or `br doctor` in scratch).

Baseline: founding FF `9c45f79a` (2026-08-21); TAKEs through upstream
`34ca862b`. Update the banner SHA only when a TAKE lands.

## Toolchain and checks

Edition 2024 nightly (`rust-toolchain.toml`). `#![deny(unsafe_code)]` (two
sanctioned `#[allow(unsafe_code)]` carve-outs in Cargo.toml). Prefix cargo with
`mbx`. Key deps / profile / features: read `Cargo.toml`.

```bash
mbx check --all-targets
mbx clippy --all-targets -- -D warnings
mbx fmt --check
mbx test
mbx test --all-features
```

Whole-crate check + clippy required when Rust changes. Workflow edits follow
[`docs/CI_SUPPLY_CHAIN.md`](docs/CI_SUPPLY_CHAIN.md) (`git diff --check`,
`actionlint` when available, harnesses named there).

## Where to look

| Task | Location |
|------|----------|
| CLI dispatch | `src/main.rs`, `src/cli/mod.rs`, `src/cli/commands/*.rs` |
| Elm-effect core | `src/elm/{mod,world,tests}.rs` |
| Types / hashing | `src/model/mod.rs` |
| SQLite / schema / events | `src/storage/` |
| JSONL sync | `src/sync/` |
| Config / routing | `src/config/` |
| Close policy | `src/close_policy.rs` + `CloseFold` via `src/elm/world.rs` |
| TUI | `src/tui/` — theme `theme.rs` (hacker-night); keys `keys.rs` REGISTRY |
| Tests | inline `#[cfg(test)]` + `tests/` |

Third-party docs: Context7 MCP first, then `chie qmd search --agent`.

## Elm-effect core

`update(model, msg) -> Update<model, commands>` is pure — no clock, env, or DB
inside the fold (lints in `src/elm/tests.rs`). Two streams never cross:

- Issue rows ← `BeadsMsg::Event`
- Gate verdicts ← `BeadsMsg::GatesLoaded` only

So `br sync --import-only` cannot mint a PASS. Writes stay in storage
(`run_close_core` / `BEGIN IMMEDIATE`) — a `Persist` command in the fold would
be a second journal. One-decision rule: unanswered close is a refusal, never a
pass (`CloseDecision::unanswered`). Proof:
`sh scripts/e2e-elm-fold-close.sh`. Adoption notes:
`docs/learnings/adopting-elm-effect-into-a-cli-whose-store-is-not-sync.md`.
Carrier law: sibling `../elm-effect-rs` (ADR-0002).

## VERIFY fence honesty

A fence that is a single runnable command legally closes only via
`command-verified`. If you verified via unit tests, the fence must state the
composed commands you really ran.

## Sync safety

Changes under `src/sync/` or `src/cli/commands/sync.rs` follow
[`docs/SYNC_MAINTENANCE_CHECKLIST.md`](docs/SYNC_MAINTENANCE_CHECKLIST.md):
no git from sync, `.beads/` path allowlist, `mbx test e2e_sync --release`.
Also: `docs/SYNC_SAFETY.md`, `docs/E2E_SYNC_TESTS.md`,
`.beads/SYNC_SAFETY_INVARIANTS.md`.

## Agent entry

Agents never run bare `br` (human TUI). Use `--json` / `--robot-*` /
`--format json|toon`. Schema: `br schema all --format json`. Deeper agent docs
(not contracts): `docs/agent/QUICKSTART.md`, `ROBOT_MODE.md`, `SCHEMA.md`,
`ERRORS.md`, `EXAMPLES.md`, plus `docs/AGENT_INTEGRATION.md` and
`docs/CLI_REFERENCE.md`.

## Output / TUI

Modes and agent flag rules live in the br skill. TUI theme/bindings: consult
tui-design skill; verify `mbx test --lib` plus manual sizes / `NO_COLOR=1`.
