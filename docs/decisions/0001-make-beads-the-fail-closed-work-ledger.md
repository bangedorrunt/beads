<!-- governed-by: ADR-0001 -->
---
status: accepted
date: 2026-08-21
decision-makers: Captain (bangedorrunt)
consulted: flywheel × toron ideation 2026-08-21 (work-ledger fork); ADR-0019 grilling 2026-08-15 and pre-acceptance amendment (br gate as verdict store); live toron `.beads/policy.yaml` + flywheel `verdict.rs` / `durable.rs`
informed: flywheel maintainers; toron maintainers (ADR-0019 lock 4 amendment is a *toron* follow-up bead, not this repo)
---

# ADR-0001: Make beads the fail-closed work-ledger for flywheel × toron

**Status:** Accepted (2026-08-21, captain). The fork exists (`https://github.com/bangedorrunt/beads`). This record is the whole program for it.
**Date:** 2026-08-21
**Decision-makers:** Captain (bangedorrunt)
**Extends:** nothing in this repo (first ADR). Consumes the *invariants* of toron [ADR-0019](https://github.com/bangedorrunt/toron/blob/main/docs/decisions/0019-bead-verified-execution-and-principle-playbooks.md) (bead = unit of work; proof = ledger row; isolation = reservation) and toron [ADR-0015](https://github.com/bangedorrunt/toron/blob/main/docs/decisions/0015-flywheel-toron-integration-contract.md) (one-way `flywheel → toron-workflow → toron-core`).
**Amends (in toron, separately):** ADR-0019 lock 4 ("No `br` fork"). That amendment is **one toron bead**, filed from this ADR's More Information. This file does not edit the toron tree.
**Does not amend:** toron kind registry, toron MCP surface (39 tools / 25 resources), flywheel `decide` table, reservation conflict check.
**Companion:** flywheel `src/verdict.rs` `legal_close` (the table this fork must enforce) and `src/durable.rs` `ReadyBead` (the ready predicate this fork must own).
**Tracker:** this repo. Beads in *this* workspace track fork work. Flywheel/toron beads stay in those repos until the product graph wave.

> **Scope:** turn `br` from a generic GitHub-issues-shaped tracker into the typed work unit flywheel already pretends it is. A bead is a brief + claim lease + verify command + legal close. `br close` without the *right* gate kind is an error. `br ready` is the loop's dispatchable set. Flywheel deletes markdown scrapers after the migration. Toron stays mail, reservations, jobs. `br` still never runs git.

This ADR is the whole program. Wave 1 is the first cut. Waves 2–6 are specified here so later agents do not invent a smaller vision.

---

## 1. TL;DR

`br close` is a self-report. Flywheel re-parses `## VERIFY` / `## PRINCIPLES` out of the description blob, re-filters `br ready`, and reopens illegal closes after the fact. Toron's `.beads/policy.yaml` is **advisory** (`br close` ignores gate satisfaction) and also **wrong** (`require_all` of five verdict kinds on every close). Gate rows are **not** exported to JSONL (`src/cli/commands/gate.rs`), so a clone has no verdicts.

**Decision:** this fork is the work-ledger for flywheel × toron.

1. Typed fields on `Issue` (VERIFY, PRINCIPLES, wave, pin, commit SHA, verdict kind). Markdown fences become a one-shot import, then dead.
2. Fail-closed close. The ADR-0019 legal-close table lives in `br`, default on, no `require_all` of five kinds.
3. Gate results sync through JSONL. A clone can prove a close.
4. One ready predicate. Flywheel trusts `br ready --json`.
5. Keep SQLite (fsqlite) + JSONL. Keep binary name `br`. Keep "br never runs git."
6. Absorb `bv --robot-*` into `br next` / `br triage` later. Do not add MCP. Do not track upstream features after the 2026-08-21 fast-forward.

Cite: `encode-lessons-in-structure` — close legality moves from loop-reopen into types. `type-system-discipline` — illegal close is unrepresentable. `prove-it-works` — a close is a gate row plus a SHA, not a reason string. `subtract-before-you-add` — delete the dual lint templates and the flywheel markdown parsers after migration.

---

## 2. Context and Problem Statement

How do flywheel × toron make overnight swarm output *trustable* when the tracker they use treats close as a self-report and ready as "unblocked open issues"?

### 2.1 What this repo is

`bangedorrunt/beads` is a hard fork of [`Dicklesworthstone/beads_rust`](https://github.com/Dicklesworthstone/beads_rust) (`br`, Rust, fsqlite + JSONL). Parent exists because Steve Yegge's original `bd` moved toward GasTown; Jeffrey Emanuel froze the classic SQLite+JSONL architecture. We fork *that* freeze because flywheel × toron need a **work-ledger**, not a generic agent issue tracker, and because ADR-0019's "no fork, enforce in the loop" workaround is now the bug.

Parent of this clone: `Dicklesworthstone/beads_rust` `9c45f79a` (2026-08-21), fast-forwarded on fork day from `c104d26a` (26 commits: doctor dead-edge handling #432, write-lock shapes, fsqlite 0.3.6, linked-worktree `.beads` #429, inherited_context JSON). Installed binaries at decision time: `br` 0.3.2, `bv` 0.20.0 (separate repo `Dicklesworthstone/beads_viewer`).

### 2.2 Nouns (no tribal knowledge)

| Noun | Meaning |
| :--- | :--- |
| **bead** | One unit of work. ID like `bd-compose-not-durable-oxjy`. Lives in `.beads/`. |
| **`br`** | This repo's CLI. SQLite primary, JSONL for git. Never commits. |
| **`bv`** | Companion graph/TUI (`beads_viewer`). Agents must use `--robot-*`; bare `bv` blocks. |
| **flywheel** | Swarm orchestrator. `loop run --work` claims beads, dispatches panes, checks a ledger. |
| **toron** | Nostr-native mailbox. Mail, channels, reservations, jobs. Frozen 39 MCP tools. |
| **herdr name** | Pane identity for `br` assignee, e.g. `flywheel-toron-coder`. |
| **pin** | Mail identity, adjective+noun, e.g. `DustySparrow`. Not the assignee. |
| **VERIFY** | Single-line command that proves the bead. Today scraped from `## VERIFY` fences. |
| **PRINCIPLES** | Named engineering principles with the decision they changed. Required for priority ≤ 2. |
| **wave-gate** | Label `wave-N`. A wave-N bead is held while any wave-M (M<N) is open or in_progress. |
| **gate** | `br gate report/list`. Named verdict rows. Today not in JSONL. |
| **legal close** | A close whose gate kind matches the ADR-0019 table for that bead's priority, blast, and VERIFY shape. |

### 2.3 Measured drift (2026-08-21; evidence, not vibes)

| Aspect | Intent (ADR-0019 / loop) | Code today | Consequence |
| :--- | :--- | :--- | :--- |
| Close | Fail-closed on a legal verdict | `src/close_policy.rs`: every gate is opt-in; no `policy.yaml` → close behaves as before the module existed. Toron `policy.yaml` comment: "`br close` ignores gate satisfaction" | A lying or hurried `br close --reason done` drains ready. Loop reopens after the fact. |
| Legal kinds | One kind per band (table in §4.2) | Toron `.beads/policy.yaml` `require_all` of `command-verified`, `unit-test-verified`, `live-verified`, `reviewer-signed`, `worker-receipt` | Cheap P3 cannot close honestly; policy and table contradict. |
| VERIFY | Dispatchable iff one fenced command | Flywheel `durable.rs` `parse_verify_fence` on `description`. `Issue` already has `acceptance_criteria` unused by the loop | Two sources of truth. Agents write neither reliably. |
| PRINCIPLES | Required for P≤2, `name — decision` | Same: markdown scrape into `ReadyBead.principles: String` | A P1 with no principles is "ready" to `br` and "not ready" to the loop. |
| Ready | Loop dispatchable set | `src/cli/commands/ready.rs`: open, unblocked, not deferred, not pinned, not ephemeral | Flywheel re-filters. Two ready sets. |
| Wave | Hold later waves | Labels `wave-N` parsed in flywheel | Labels collide with other uses. Not a field. |
| Verdict durability | Ledger survives clone | `src/cli/commands/gate.rs`: "Gate results are auxiliary… they are not synced through JSONL" | Git has closes without proof. |
| Lint | One brief schema | Upstream `br lint` wants `## Acceptance Criteria` / `## Steps to Reproduce`. Loop wants `## VERIFY` / `## PRINCIPLES` | Toron workspace at decision time: 9 open issues, 12 lint hits, 6 dead blocking edges. |
| Identity | herdr name claims; pin mails | `assignee` is a free string; pin lives in flywheel `agent-names.json` | Dual-name bugs are skill prose. |
| Graph | `bv --robot-triage` for agents | Separate binary, separate JSONL parse, 500ms graph timeout. This repo already has `src/cli/commands/graph.rs` (dependents only) and `scheduler.rs` | Three triage brains (`br ready`, `br scheduler`, `bv`). |
| ADR-0019 lock 4 | "No `br` fork. No new Nostr kind." | This fork exists. Enforcement stayed in flywheel | The workaround is the debt. |

### 2.4 Why a fork, not a policy file

The parent is a general tracker: 50+ CLI verbs, optional MCP (`mcp` feature, FastMCP + asupersync 0.3.x), AGENTS.md writer, GitHub-plugin install, `bd` migration, capacity exemptions, changelog-as-product. Flywheel × toron need a **smaller, stricter** machine. Staying compatible with upstream re-grows the surface we are deleting. Hard fork. Fast-forward once (done: `9c45f79a`), then diverge.

### 2.5 What already exists (do not rebuild)

| Piece | Where | Job |
| :--- | :--- | :--- |
| `Issue` | `src/model/mod.rs` | Title, description, status, priority 0–4, type, assignee, deps, labels, `acceptance_criteria` text, timestamps |
| Schema | `src/storage/schema.rs` | `CURRENT_SCHEMA_VERSION = 17`. Next cut is **18** |
| Close policy | `src/close_policy.rs` | Opt-in YAML. `--bypass-policy` + reason. Unknown YAML keys warn, do not crash (#302) |
| Gate engine | `src/cli/commands/gate.rs` | `report` / `list`. `--to` binds a target status. **Not in JSONL** |
| Ready | `src/cli/commands/ready.rs` + `SqliteStorage` | Unblocked open (plus `status_groups.ready`) |
| Sync | `src/sync/` | `--flush-only` / `--import-only` / `--merge` / `--reconcile`. Never git |
| Graph CLI | `src/cli/commands/graph.rs` | Dependents visualization, not PageRank |
| Flywheel legal_close | `../flywheel/src/verdict.rs` | Exhaustive match on `VerdictKind`. This fork must implement the same table |
| Flywheel ready parse | `../flywheel/src/main.rs` `parse_ready_beads_json` | Scrapes fences. Delete after wave 1 |
| Reservations | toron, not this repo | held=HARD, intent=SOFT. Do not duplicate |

---

## 3. Decision Drivers

* Overnight swarm output is untrustable while close is a self-report.
* Ready in the shell must equal ready in the loop. Two predicates is a false-close factory.
* Verdicts must survive `git clone` + `br sync --import-only`.
* Flywheel must shrink (delete markdown parsers), not grow (another ledger).
* Toron MCP surface stays frozen. No beads MCP.
* `br` stays non-invasive: it never commits, pushes, pulls, or installs hooks.
* Storage must survive swarm write bursts (parent issues #426 #428 #434 #435 are in-scope as later waves, not excuses to replace the engine in wave 1).
* Binary name `br` stays so existing skills, AGENTS.md, and flywheel `Command::new("br")` keep working.

---

## 4. Considered Options

* **A. Keep parent `br`, enforce in flywheel** (ADR-0019 lock 4 as written)
* **B. This fork: typed work-ledger, fail-closed close in `br`** (chosen)
* **C. Pull the tracker into toron kinds / MCP tools**
* **D. Replace JSONL+fsqlite with git-notes or rusqlite**

## 5. Decision Outcome

Chosen option: **B**, because the enforcement seam has to sit on `br close` itself. Loop-reopen (A) is after-the-fact and loses the race against `br ready`. Toron kinds (C) are frozen. Replacing storage (D) is a different ADR and does not fix close legality.

### 5.1 Layer map (each layer ships independently)

```
L1  typed Issue fields + schema v18     model + storage + JSONL
L2  fail-closed close + legal-close     close_policy default ON
L3  gate rows in JSONL                  sync flush/import
L4  unified ready predicate             ready.rs = loop dispatchable
L5  flywheel becomes a consumer         delete parse_verify_fence after L4
L6  br next / triage (absorb bv robot)  later wave
```

L1–L4 are **wave 1**. L5 is a flywheel commit after toron/flywheel upgrade `br`. L6 is wave 2.

### 5.2 Schema v18 — typed work-ledger fields

Add to `Issue` (`src/model/mod.rs`) and `CREATE TABLE issues` (`src/storage/schema.rs`). All new columns nullable / serde-default so schema-17 JSONL still imports.

```rust
/// Single-line VERIFY command. None/empty = not dispatchable.
pub verify: Option<String>,

/// Principle citations. Required non-empty for priority <= 2.
#[serde(default)]
pub principles: Vec<PrincipleCitation>,

/// Wave index. None = no wave-gate. Some(n) is held while any
/// open/in_progress bead has wave m with m < n.
pub wave: Option<u32>,

/// Mail pin (adjective+noun). Distinct from assignee (herdr name).
pub pin: Option<String>,

/// Git SHA the worker claims contains this bead id in the message.
/// Required on close. `br` does not run git to verify; flywheel / CI does.
pub commit_sha: Option<String>,

/// Last legal verdict kind that authorized close, kebab-case.
pub close_verdict: Option<String>,
```

```rust
pub struct PrincipleCitation {
    /// Canonical name from `flywheel principles list` (21-name registry).
    pub name: String,
    /// The decision this principle changed, non-empty.
    pub decision: String,
}
```

`br create` / `br update` grow `--verify`, `--principle 'name — decision'` (repeatable), `--wave N`, `--pin`, `--commit-sha`. `br close` requires `--commit-sha` unless bypass.

Bump `CURRENT_SCHEMA_VERSION` from **17 to 18**. Migration is additive `ALTER TABLE` + JSONL field defaults. `br doctor migrate-schema` must run `integrity_check` after stamp and **fail** if it disagrees (parent #428 is the anti-pattern).

**One-shot fence import** (wave 1, same cut): if `verify` is empty and `description` contains a legal `## VERIFY` fence (same rules as flywheel `parse_verify_fence`: heading + one fenced one-line command), copy that line into `verify`. Same for `## PRINCIPLES` lines matching `^([a-z0-9-]+) — (.+)$`. Do not keep using the fence after the field is set. `br lint` drops `## Acceptance Criteria` / `## Steps to Reproduce` as required sections. Lint wants: non-empty `verify`; P≤2 non-empty `principles`; each principle name is kebab-case + non-empty decision.

### 5.3 Legal-close table (copied from flywheel `verdict.rs`; this fork enforces it)

Gate names are kebab-case, matching `br gate report --gate`:

| Condition | Legal gate names (exactly one matching PASS row for the `open -> closed` or `in_progress -> closed` transition) |
| :--- | :--- |
| priority ≥ 2, blast Normal, VERIFY loop-runnable | `command-verified` **only** |
| priority ≥ 2, blast Normal, VERIFY not loop-runnable | `worker-receipt`, or `unit-test-verified`, or `live-verified` |
| priority ≤ 1 **or** blast High | `unit-test-verified` or `live-verified` only |
| AC is judgment (no checkable command) | `reviewer-signed` only |
| any | never a FAIL row, never missing |

Loop-runnable VERIFY is the cheap-band classifier already in flywheel `verdict.rs` (`cargo test`/`pytest`/`timeout`/`bash` with explicit timeout token; no `&&` composition). **Port that function into this crate** as `crate::verify::is_loop_runnable(verify: &str) -> bool` so `br close` and flywheel cannot drift. Flywheel then calls the same rules by depending on behavior, not by re-parsing.

Default blast is Normal. Judgment AC is explicit: `--ac judgment` on create/update, stored as `ac_shape: checkable | judgment` (add the field; default `checkable` when `verify` is present, `judgment` when `verify` is absent **and** `--ac judgment` was set). A missing VERIFY on a checkable bead is not-ready, not judgment.

**Default policy** (applied when `.beads/policy.yaml` is absent **or** when `workflow.strict` is unset). This replaces toron's advisory `require_all` five:

```yaml
# ADR-0001 default. Generated by `br init`. Agents cannot --bypass-policy
# unless BR_OPERATOR=1.
allow_bypass: true
workflow:
  strict: true
  statuses: [open, in_progress, closed, deferred]
  transitions:
    open: [in_progress, closed, deferred]
    in_progress: [open, closed, deferred]
    deferred: [open]
    closed: [open]   # reopen only; NOT closed -> closed
  gates:
    # Gate *name* is chosen by legal_close(bead), not require_all.
    # Implementation: evaluate_gates calls legal_close; YAML here
    # documents the chokepoint, it does not list five names.
    open -> closed: { require_legal_close: true }
    in_progress -> closed: { require_legal_close: true }
```

`require_legal_close: true` is a **new** GateRule field. When set, `close_policy::evaluate_gates` runs `legal_close` against recorded PASS rows for that issue and the candidate target. `require_all: [five names]` is **illegal** in this fork: `br doctor` errors if a policy file contains it.

`--bypass-policy` remains for the operator. It requires `--bypass-reason` (already true). In this fork it also requires `BR_OPERATOR=1` in the environment. Flywheel never sets that. Agent skills never set that.

`closed -> closed` self-transition (toron policy, so the loop could stamp a verdict after close) is **deleted**. Record the gate **before** close. Order: `br gate report … --to closed` then `br close --commit-sha <sha>`. One transaction if a later helper `br close --gate-from-report` lands; wave 1 may keep two commands as long as close refuses without the row.

### 5.4 Gate rows in JSONL

Change `src/cli/commands/gate.rs` + sync: gate results are first-class export objects (new JSONL type or a sidecar `.beads/gates.jsonl` flushed atomically with `issues.jsonl`). Pick **sidecar `.beads/gates.jsonl`** so issue rows stay one-id-per-line and merges stay line-oriented. `br sync --flush-only` writes both. `br sync --import-only` loads both. `data_hash` for robot output hashes both files.

A close that has `close_verdict` set but no matching gates.jsonl PASS row is a doctor error.

### 5.5 Unified ready predicate

`br ready` returns an issue iff **all** hold:

1. Status is in `status_groups.ready` (default: `open` only). `in_progress` is never ready.
2. No **open or in_progress** blocker. Closed/missing blockers are ignored (parent #432). `br doctor` still reports them as repair work; they do not hide the issue from ready.
3. Not deferred, not pinned, not ephemeral.
4. `verify` is `Some` with a non-empty single line and no `\n`.
5. If `priority <= 2`: `principles` has ≥1 citation, each with non-empty `name` and `decision`.
6. Wave-gate: if `wave = Some(n)`, no other non-closed issue has `wave = Some(m)` with `m < n` and status `open` or `in_progress`. Issues with `wave = None` are not held by this rule.
7. Not already assigned to a live in_progress claim (existing coordination rules stay).

JSON shape for agents (keep current array; add fields):

```json
{
  "id": "bd-…",
  "title": "…",
  "priority": 1,
  "assignee": null,
  "pin": null,
  "labels": [],
  "wave": 2,
  "verify": "cargo test -p toron-workflow compose::",
  "principles": [{"name": "prove-it-works", "decision": "…"}]
}
```

Flywheel `ReadyBead` maps 1:1 onto this. After upgrade, `parse_verify_fence` and the PRINCIPLES markdown check **delete**.

`--json` stays the agent default contract. `--format toon` stays available.

### 5.6 Identity

* `assignee` = herdr name (`flywheel-toron-coder`). This is what `br update --claim` writes.
* `pin` = mail pin (`DustySparrow`). Optional on create; flywheel fills it from `agent-names.json` when dispatching.
* `--actor` audit trail defaults to `assignee` if set, else `BR_ACTOR`, else `assistant`.
* Mail `thread_id` remains the bead id. This repo does not send mail.

### 5.7 `br` / `bv` merge (wave 2, specified now)

One cargo workspace. Binary `br` stays the write kernel + robot reads. Binary `bv` becomes a thin alias:

* `bv --robot-next` → `br next --json`
* `bv --robot-triage` → `br triage --json`
* `bv --robot-plan` → `br plan --json`
* Bare `bv` may still launch a TUI **if** stdin is a TTY; agents pass `--robot-*` or `br next`. Default agent docs mention only `br`.

Do not vendor the Go TUI in wave 1. Robot JSON is the load-bearing surface. Human TUI is allowed to lag.

### 5.8 Storage (wave 1 constraint + wave 4 work)

Wave 1 **keeps** fsqlite (`fsqlite` 0.3.6 as of the fast-forward) behind `src/franken_sync.rs`. `asupersync` stays inside that seam. It must not appear in flywheel or toron (those crates `#![forbid(unsafe_code)]` and forbid asupersync by policy). Do not switch to rusqlite in this ADR.

Wave 4 (after L1–L4 green) fixes parent swarm bugs in *this* tree:

| Upstream | Failure | This fork |
| :--- | :--- | :--- |
| #435 | `export_hashes` upsert lost (exit 0) while another process holds `beads.db` | Fail the flush; never exit 0 on a skipped hash write |
| #434 | SIGABRT when the reader closes the pipe (`br list \| head`) | Treat EPIPE as a normal stdout close |
| #428 | `migrate-schema` 16→17 reports success while `integrity_check` fails | Stamp `user_version` only after integrity_check OK |
| #426 | B-tree corruption on 264 sequential dep-remove writes | Repro as a regression test; fix or serialize writes |

### 5.9 CLI strip (wave 5)

Keep: `init`, `create`, `q`, `show`, `update`, `close`, `reopen`, `ready`, `next`, `triage`, `plan`, `blocked`, `dep`, `label`, `gate`, `sync`, `doctor`, `schema`, `lint`, `count`, `search`, `comments`, `config`, `where`, `version`, `defer`, `undefer`.

Deprecate then delete (after flywheel/toron callers gone): `agents --add` (writes AGENTS.md), MCP server feature as default, `changelog` as a product surface, `capacity` exemptions, GitHub-plugin install, `bd` migration skill. `scheduler` collapses into `br next`.

Wave 5 does not start until wave 1 ready/close proofs are green on toron + flywheel workspaces.

### 5.10 Product graph (wave 6)

Parent already has optional `routes.jsonl`. A later ADR may make toron + flywheel + toron.dev one captain graph. Per-repo `.beads/issues.jsonl` remains the git unit. Out of wave 1.

---

## 6. Non-goals

* New Nostr kinds or new toron MCP tools.
* Beads MCP (`br serve` / `mcp` feature is not in the default release; do not document it as the agent path).
* `br` running `git commit` / `push` / `pull`. `--commit-sha` is a field the caller supplies.
* Worktrees as loop isolation (ADR-0019 Spec 3). `br` may *discover* a primary-checkout `.beads` from a linked worktree (parent #429); flywheel still must not spawn worktrees for beads.
* Tracking Dicklesworthstone feature work after `9c45f79a`. Cherry-pick a storage fix if wave 4 needs it; do not merge "the parent moved."
* Replacing fsqlite, replacing JSONL, adopting GasTown, embedding flywheel inside `br`.
* Reservation enforcement inside `br` in wave 1. Toron owns the lease. Wave 3 may *display* holder+paths on `br show`; it must not grant leases.

---

## 7. Consequences

* Good, because `br close` without a legal PASS gate is a non-zero exit. False-close requires `--bypass-policy` + `BR_OPERATOR=1`.
* Good, because `br ready --json` is the loop's dispatchable set. Flywheel deletes `parse_verify_fence`.
* Good, because clones see gate rows. Proof is in git.
* Good, because the dual lint templates die. One brief schema.
* Bad, because every consumer workspace (toron, flywheel, later toron.dev) must upgrade `br` the same week as schema 18. Mixed-version swarms will fail close.
* Bad, because agents that only know `br close --reason done` will bounce until skills update.
* Bad, because we now own storage bugs the parent has not fixed (#426 #428 #434 #435).
* Neutral, because binary name `br` and JSONL-in-git stay. Skills keep working; semantics tighten.
* Neutral, because `bv` remains a name during wave 2. Do not break `bv --robot-triage` until the alias ships.

### Follow-ups (not this file's job except the first)

1. **Toron:** one bead to amend ADR-0019 lock 4 (see More Information).
2. Flywheel: map `ReadyBead` onto schema-18 JSON; delete markdown parsers after both workspaces run the new `br`.
3. Skills: `~/.agents/skills/br/SKILL.md` and toron/flywheel skills — fail-closed close, no beads MCP, `br next`.
4. Wave 2–6 beads, filed in *this* repo after wave 1 proofs.

---

## 8. Implementation Plan

An agent should implement wave 1 from this section without asking follow-up questions.

### 8.1 Blast radius

**Context (read, do not redesign):** `src/model/mod.rs` `Issue`; `src/storage/schema.rs`; `src/storage/sqlite.rs`; `src/sync/`; `src/close_policy.rs`; `src/cli/commands/{close,ready,gate,create,update,lint,doctor,init}.rs`; `src/cli/mod.rs` clap args; `agent_baseline/schemas/`; flywheel `src/verdict.rs` (copy the table + `is_loop_runnable`, do not take a crate dep on flywheel).

**Target (write):**

* `src/model/mod.rs` — new fields + `PrincipleCitation` + `AcShape`
* `src/verify.rs` — **new**: `is_loop_runnable`, `legal_close` (port from flywheel `verdict.rs`; keep kebab-case gate names)
* `src/storage/schema.rs` — v17 → v18 additive columns; `integrity_check` after stamp
* `src/close_policy.rs` — `require_legal_close`; default-ON policy; reject `require_all` of the five ADR-0019 names; `BR_OPERATOR=1` for bypass
* `src/cli/commands/close.rs` — require legal PASS + `--commit-sha`; write `close_verdict`
* `src/cli/commands/ready.rs` + storage ready query — §5.5 predicate
* `src/cli/commands/gate.rs` + sync — `.beads/gates.jsonl`
* `src/cli/commands/lint.rs` — VERIFY/PRINCIPLES fields, drop competing sections
* `src/cli/commands/init.rs` — write default `policy.yaml` from §5.3
* `src/cli/commands/create.rs` / `update.rs` — new flags
* Fence import helper used by `sync --import-only` and `doctor --repair` (one-shot)
* Tests: see §8.4
* `agent_baseline/schemas/` regenerated via existing `br schema` snapshot tests

**Forbidden:**

* `use flywheel::` or `use toron_` from this crate
* Enabling `mcp` in default features / documenting MCP as the agent path
* `Command::new("git")` in `src/sync/` or close
* Default policy `require_all` of five verdict names
* Keeping markdown fences as the live source of VERIFY after a field is set
* `closed -> closed` as a gated transition
* Changing JSONL issue `id` format
* Wave 2 TUI work, wave 4 storage rewrites, wave 5 command deletion, in the same PR as schema 18

### 8.2 Dependencies

No new crates for wave 1. Keep `fsqlite` / `asupersync` pins from the 2026-08-21 fast-forward. Do not bump fsqlite "to see."

### 8.3 Patterns to follow / avoid

**Follow:** existing `#[serde(default, skip_serializing_if = "Option::is_none")]` on `Issue`; additive schema comments that fsqlite stores faithfully (no extra SQL comments that shift token counts — see schema.rs #289 note); `--json` snapshots under `tests/snapshots/`; close-policy unknown-key warn (#302); sync allowlist (writes stay in `.beads/`).

**Avoid:** silent success when a flush/hash write is skipped; `unwrap` on close/ready paths; parsing VERIFY from description in ready after v18 fields exist (import helper is the only parser, and only when `verify` is empty); taking a dependency on flywheel or toron.

### 8.4 Tests (RED then GREEN; TDD is mandatory)

Add tests next to the module they prove. Names are the contract:

1. `legal_close_p3_loop_runnable_accepts_only_command_verified`
2. `legal_close_p3_loop_runnable_rejects_worker_receipt`
3. `legal_close_p0_rejects_command_verified`
4. `legal_close_judgment_ac_accepts_only_reviewer_signed`
5. `close_without_pass_gate_is_nonzero`
6. `close_without_commit_sha_is_nonzero`
7. `close_with_legal_gate_and_sha_sets_close_verdict`
8. `bypass_without_br_operator_is_rejected`
9. `ready_omits_missing_verify`
10. `ready_omits_p1_missing_principles`
11. `ready_holds_wave_2_while_wave_1_open`
12. `ready_ignores_closed_blocker` (parent #432 behavior stays)
13. `gates_jsonl_roundtrip_flush_import`
14. `schema18_import_of_schema17_jsonl_fills_defaults`
15. `fence_import_copies_verify_once_then_ignores_later_fence_edits`
16. `migrate_18_fails_if_integrity_check_fails`
17. `lint_does_not_require_acceptance_criteria_heading`
18. `init_writes_require_legal_close_policy`

Port loop-runnable examples from flywheel `verdict.rs` tests (the `cd x && cargo test` case is **not** loop-runnable; `cargo test -p foo bar` is).

### 8.5 Migration of live workspaces (toron, flywheel)

After the new `br` is installed:

```bash
br sync --import-only          # load JSONL
br doctor --repair             # fence import + dead-edge report
br lint --json                 # should not ask for Acceptance Criteria
br ready --json                # dispatchable set
br sync --flush-only           # issues.jsonl + gates.jsonl
```

Do not close beads as part of the import. Fence import is lossless: description text stays.

### 8.6 Configuration

* `.beads/policy.yaml` — default from §5.3 on `br init`; existing toron file **must be replaced** (it is advisory + `require_all` five). Replacement is a toron-tree change, same week as the binary upgrade.
* `BR_OPERATOR=1` — required for `--bypass-policy`.
* No new long-lived env vars for ready/close.

### 8.7 Code ↔ ADR linkage

Wave 1 files listed in Target get `// governed-by: ADR-0001` at the top. `src/verify.rs` is the canonical legal-close entry point.

---

## 9. Verification

Wave 1 is done only when every box is checked with a command, not a self-report.

- [ ] `CURRENT_SCHEMA_VERSION == 18` in `src/storage/schema.rs`
- [ ] `Issue` has `verify`, `principles`, `wave`, `pin`, `commit_sha`, `close_verdict`, `ac_shape`
- [ ] `src/verify.rs` exists and `legal_close` matches the table in §5.3 (exhaustive over gate kinds; no wildcard)
- [ ] `cargo test legal_close_ --offline` (or the crate's equivalent filter) is green
- [ ] `cargo test ready_omits_ --offline` is green
- [ ] `cargo test gates_jsonl_roundtrip_flush_import --offline` is green
- [ ] `br close` on a fixture bead with no PASS gate exits non-zero
- [ ] `br close --bypass-policy --bypass-reason x` without `BR_OPERATOR=1` exits non-zero
- [ ] `br ready --json` on a bead missing `verify` does not include it
- [ ] `br ready --json` on a P1 missing `principles` does not include it
- [ ] `.beads/gates.jsonl` is written by `br sync --flush-only` after `br gate report`
- [ ] `br lint --json` on a bug bead does not list `## Acceptance Criteria` or `## Steps to Reproduce` as missing
- [ ] `br init` in a temp dir writes `require_legal_close: true` and does not write `require_all` of five verdict names
- [ ] `rg "use flywheel::|use toron_" src/` returns no matches
- [ ] Default Cargo features do not include `mcp` (or the release docs tell agents to use CLI)
- [ ] `rg "Command::new\\(\"git\"\\)" src/sync src/cli/commands/close.rs src/cli/commands/sync.rs` returns no matches
- [ ] This ADR's filename is `docs/decisions/0001-make-beads-the-fail-closed-work-ledger.md` and the index table lists it accepted

Wave 1 is **not** done if flywheel still scrapes markdown. That deletion is a flywheel commit after both workspaces run schema 18. Record it there; do not block the `br` binary on it.

---

## 10. Pros and Cons of the Options

### A. Keep parent `br`, enforce in flywheel

What ADR-0019 lock 4 specified: `br gate` as store, loop reopens illegal closes, no fork.

* Good, because no schema migration, no consumer upgrade week
* Good, because parent keeps shipping generic tracker fixes
* Bad, because close stays a self-report (toron policy is advisory)
* Bad, because ready stays wrong until flywheel re-filters
* Bad, because gate rows are not in JSONL
* Bad, because two templates fight (`br lint` vs loop fences)
* Bad, because every new flywheel parser is another lesson not encoded in structure

Rejected: the workaround is the bug we are retiring.

### B. This fork: typed work-ledger (chosen)

* Good, because illegal close is a `br` error
* Good, because ready is one predicate
* Good, because proof is in git
* Good, because flywheel shrinks
* Bad, because we own a storage engine under swarm load
* Bad, because skills and two workspaces must upgrade together
* Neutral, because `br` as a name and JSONL-in-git stay

### C. Tracker as toron kinds / MCP tools

* Good, because one daemon
* Bad, because MCP tool names and kinds are frozen (ADR-0014 / ADR-0020)
* Bad, because beads would inherit Nostr latency and gift-wrap rules they do not need
* Bad, because `br` local-first + git JSONL is the collab unit we already have

Rejected: frozen surface. Do not launder a tracker through mail.

### D. Replace fsqlite/JSONL

* Good, because parent storage bugs are real
* Bad, because it does not make close fail-closed
* Bad, because JSONL-in-git is how toron/flywheel already collaborate
* Bad, because rusqlite-vs-fsqlite is a second ADR with its own blast radius

Rejected for *this* decision. Wave 4 may fix fsqlite usage; it may not silently swap the engine.

---

## 11. Implementation Blueprint (agent blast-radius card)

### Context

Sibling of `toron` and `flywheel` at `~/workspace/beads`. Consumers: flywheel `loop run --work`, `flywheel ledger`, toron `.beads/`. Humans still run `br show` / `bv` TUI.

### Target

`src/verify.rs`, `Issue` schema v18, close/ready/gate/sync/lint/init, default policy, gates.jsonl, tests listed in §8.4.

### Forbidden

Flywheel/toron crate deps. MCP as agent path. Git from `br`. `require_all` five verdicts. Markdown as live VERIFY. New Nostr kinds. Upstream feature merges after `9c45f79a`.

---

## 12. Waves (do not start 2–6 until 1's verification boxes are green)

| Wave | Outcome | Proof |
| :--- | :--- | :--- |
| **1** | Typed fields, fail-closed close, gates.jsonl, unified ready, default policy, fence import | §9 checkboxes |
| **2** | `br next` / `br triage` / `br plan` absorb `bv --robot-*`; `bv` alias | `br next --json` matches current `bv --robot-next` shape on a fixture workspace |
| **3** | `pin` filled by flywheel on claim; `br show` can display reservation holder/paths **read-only** (toron remains grantor) | show JSON includes pin; no lease grant in this crate |
| **4** | Parent #434 #435 #428 #426 addressed in this tree | each issue has a regression test that fails on unfixed main and passes on the fix |
| **5** | CLI strip; flywheel markdown parsers gone; skills updated | `parse_verify_fence` deleted; `br lint` is the only brief linter |
| **6** | Product graph across repos (separate ADR) | not specified here |

---

## 13. More Information

* **Toron follow-up (one task, filed):** [`bd-54ui`](https://github.com/bangedorrunt/toron) — amend [ADR-0019](https://github.com/bangedorrunt/toron/blob/main/docs/decisions/0019-bead-verified-execution-and-principle-playbooks.md) lock 4. Replace "No `br` fork" / "flywheel-side enforcement" with: close legality lives in `bangedorrunt/beads` (`br`); flywheel consumes `br ready` / `br gate` / `br close` and does not reopen as the primary enforcement. Keep "no new Nostr kind." Do not implement the amendment in this repo. Do not implement schema v18 in that bead.
* Flywheel `src/verdict.rs` legal-close table is the behavioral original. After `src/verify.rs` ships, flywheel should call the same rules (copy or a shared crate later). Drift is a bug.
* Revisit if: schema 18 cannot import toron's 488-row JSONL losslessly; or swarm write bursts corrupt v18 the way #426 corrupted deps; or captain restores lock 4 (then supersede this ADR).
* Fast-forward baseline: `9c45f79a` `docs(agents): require OpenAI File Downloader user-agent on curl/web fetches`.
* Parent issues left open on purpose: #419 WSL DrvFS, #413 Windows JSONL route, #411 minisign key, #402 reconcile path. Not our hosts; not wave 1.
