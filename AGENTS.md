# AGENTS.md — beads (bangedorrunt fork of beads_rust)

> **Governing decision:** [ADR-0001](docs/decisions/0001-make-beads-the-fail-closed-work-ledger.md).
> This is a dedicated work-ledger for flywheel × toron, forked from `Dicklesworthstone/beads_rust` at the 2026-08-21 fast-forward (`9c45f79a`). Close is fail-closed. Binary remains `br`. Do not add MCP tools. Do not run git from `br`.
> **Upstream:** do not fetch/merge/cherry-pick from `Dicklesworthstone/beads_rust` unless the captain says **fork sync**. Then review and take only commits this fork still needs. Never merge `upstream/main` wholesale. See **RULE 2**.
>
> Guidelines below are upstream agent rules. Where they conflict with ADR-0001 or RULE 2, **ADR-0001 and RULE 2 win**.

---

## RULE 0 - THE FUNDAMENTAL OVERRIDE PREROGATIVE

If I tell you to do something, even if it goes against what follows below, YOU MUST LISTEN TO ME. I AM IN CHARGE, NOT YOU.

---

## RULE NUMBER 1: NO FILE DELETION

**YOU ARE NEVER ALLOWED TO DELETE A FILE WITHOUT EXPRESS PERMISSION.** Even a new file that you yourself created, such as a test code file. You have a horrible track record of deleting critically important files or otherwise throwing away tons of expensive work. As a result, you have permanently lost any and all rights to determine that a file or folder should be deleted.

**YOU MUST ALWAYS ASK AND RECEIVE CLEAR, WRITTEN PERMISSION BEFORE EVER DELETING A FILE OR FOLDER OF ANY KIND.**

---

## RULE 2: FORK SYNC IS OPT-IN, CHERRY-PICK ONLY

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
8. After the cherry-picks: run the smallest relevant proof (`cargo test` on the touched modules, or `br doctor` in a scratch dir). Report TAKE/SKIP lists, new HEAD, and leftover parent bugs we still do not want.

Baseline after the founding fast-forward: `9c45f79a`. Update the banner SHA only when a fork-sync TAKE actually lands.

---

## Irreversible Git & Filesystem Actions — DO NOT EVER BREAK GLASS

1. **Absolutely forbidden commands:** `git reset --hard`, `git clean -fd`, `rm -rf`, or any command that can delete or overwrite code/data must never be run unless the user explicitly provides the exact command and states, in the same message, that they understand and want the irreversible consequences.
2. **No guessing:** If there is any uncertainty about what a command might delete or overwrite, stop immediately and ask the user for specific approval. "I think it's safe" is never acceptable.
3. **Safer alternatives first:** When cleanup or rollbacks are needed, request permission to use non-destructive options (`git status`, `git diff`, `git stash`, copying to backups) before ever considering a destructive command.
4. **Mandatory explicit plan:** Even after explicit user authorization, restate the command verbatim, list exactly what will be affected, and wait for a confirmation that your understanding is correct. Only then may you execute it—if anything remains ambiguous, refuse and escalate.
5. **Document the confirmation:** When running any approved destructive command, record (in the session notes / final response) the exact user text that authorized it, the command actually run, and the execution time. If that record is absent, the operation did not happen.

---

## Git Branch: ONLY Use `main`, NEVER `master`

**The default branch is `main`. The `master` branch exists only for legacy URL compatibility.**

- **All work happens on `main`** — commits, PRs, feature branches all merge to `main`
- **Never reference `master` in code or docs** — if you see `master` anywhere, it's a bug that needs fixing
- **The `master` branch must stay synchronized with `main`** — after pushing to `main`, also push to `master`:
  ```bash
  git push origin main:master
  ```

**If you see `master` referenced anywhere:**
1. Update it to `main`
2. Ensure `master` is synchronized: `git push origin main:master`

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
- Whole-crate `cargo check --all-targets` and
  `cargo clippy --all-targets -- -D warnings` are required when Rust code
  changes 
- Run `git diff --check`, `actionlint` when available, the relevant workflow
  harnesses, and `ubs` on changed workflow-related files before committing.

---

## Toolchain: Rust & Cargo

We only use **Cargo** in this project, NEVER any other package manager.

- **Edition:** Rust 2024 (nightly required — see `rust-toolchain.toml`)
- **Dependency versions:** Explicit versions for stability
- **Configuration:** Cargo.toml only (single crate, not a workspace)
- **Unsafe code:** Forbidden (`#![forbid(unsafe_code)]` via crate lints)

### Key Dependencies

| Crate | Purpose |
|-------|---------|
| `clap` | CLI parsing with derive macros + shell completions |
| `fsqlite` + `fsqlite-types` + `fsqlite-error` | SQLite engine facade plus shared storage types/errors (path dependencies) |
| `serde` + `serde_json` | Issue serialization and JSONL export |
| `schemars` | JSON Schema generation for robot output |
| `chrono` | Timestamp parsing and RFC3339 formatting |
| `rich_rust` | Rich terminal output (panels, tables, colors) |
| `toon_rust` | TOON format support for token-efficient schema viewing |
| `crossterm` + `indicatif` | Terminal control and progress spinners |
| `anyhow` + `thiserror` | Error handling (anyhow for CLI, thiserror for typed errors) |
| `sha2` | Content hashing for deduplication |
| `regex` | Pattern matching for search and validation |
| `semver` | Semantic version parsing |
| `tracing` | Structured logging and diagnostics |
| `self_update` | Self-update from GitHub releases (optional, feature-gated) |

### Release Profile

The release build optimizes for binary size (this is a CLI tool for distribution):

```toml
[profile.release]
opt-level = "z"     # Optimize for size (lean binary for distribution)
lto = true          # Link-time optimization
codegen-units = 1   # Single codegen unit for better optimization
panic = "abort"     # Smaller binary, no unwinding overhead
strip = true        # Remove debug symbols
```

---

## Code Editing Discipline

### No Script-Based Changes

**NEVER** run a script that processes/changes code files in this repo. Brittle regex-based transformations create far more problems than they solve.

- **Always make code changes manually**, even when there are many instances
- For many simple changes: use parallel subagents
- For subtle/complex changes: do them methodically yourself

### No File Proliferation

If you want to change something or add a feature, **revise existing code files in place**.

**NEVER** create variations like:
- `mainV2.rs`
- `main_improved.rs`
- `main_enhanced.rs`

New files are reserved for **genuinely new functionality** that makes zero sense to include in any existing file. The bar for creating new files is **incredibly high**.

---

## Backwards Compatibility

We do not care about backwards compatibility—we're in early development with no users. We want to do things the **RIGHT** way with **NO TECH DEBT**.

- Never create "compatibility shims"
- Never create wrapper functions for deprecated APIs
- Just fix the code directly

---

## Compiler Checks (CRITICAL)

**After any substantive code changes, you MUST verify no errors were introduced:**

```bash
# Check for compiler errors and warnings
cargo check --all-targets

# Check for clippy lints (pedantic + nursery are enabled)
cargo clippy --all-targets -- -D warnings

# Verify formatting
cargo fmt --check
```

If you see errors, **carefully understand and resolve each issue**. Read sufficient context to fix them the RIGHT way.

---

## Testing

### Testing Policy

Every module includes inline `#[cfg(test)]` unit tests alongside the implementation. Tests must cover:
- Happy path
- Edge cases (empty input, max values, boundary conditions)
- Error conditions

Integration and end-to-end tests live in the `tests/` directory.

### Unit Tests

```bash
# Run all tests
cargo test

# Run with output
cargo test -- --nocapture

# Run tests for a specific module
cargo test storage
cargo test cli
cargo test sync
cargo test format
cargo test model
cargo test validation

# Run tests with all features enabled
cargo test --all-features
```

### Test Categories

| Directory / Pattern | Focus Areas |
|---------------------|-------------|
| `src/` (inline `#[cfg(test)]`) | Unit tests for each module: model, storage, sync, config, error, format, util, validation |
| `tests/e2e_*.rs` | End-to-end CLI tests: lifecycle, labels, deps, sync, history, search, comments, epics, workspaces, errors, completions |
| `tests/conformance*.rs` | Go/Rust parity: schema compatibility, text output matching, edge cases, labels+comments, workflows |
| `tests/storage_*.rs` | Storage layer: CRUD, list filters, ready queries, deps, history, blocked cache, export atomicity, invariants, ID/hash parity |
| `tests/proptest_*.rs` | Property-based tests: ID generation, hash determinism, time parsing, validation rules |
| `tests/repro_*.rs` | Regression tests: specific bugs reproduced and prevented |
| `tests/jsonl_import_export.rs` | JSONL round-trip fidelity |
| `tests/markdown_import.rs` | Markdown import parsing |
| `benches/storage_perf.rs` | Storage operation benchmarks (criterion) |

### Test Fixtures

Shared test fixtures live in `tests/fixtures/` and `tests/common/` for reusable test harness helpers (temp DB creation, test data builders).

---

## Third-Party Library Usage

If you aren't 100% sure how to use a third-party library, **SEARCH ONLINE** to find the latest documentation and current best practices.

---

## beads_rust (br) — This Project

**This is the project you're working on.** beads_rust is an agent-first, dependency-aware issue tracker CLI (`br`) that stores issues in SQLite with JSONL export for git-based sync. It is a Rust port of the classic Go beads issue tracker (`bd`), designed to be non-invasive (no automatic git operations, no daemons, no hooks).

### What It Does

Provides lightweight issue tracking with dependency graphs, priority-based triage, content-addressed deduplication, and multiple output modes (rich terminal, plain text, JSON, TOON). Designed specifically for AI coding agents to select "ready work," manage task dependencies, and coordinate via structured robot output.

### Architecture

```
CLI (clap derive)
    │
    ├── Commands ────── 35+ subcommands (create, list, show, close, dep, sync, ...)
    │                       │
    │                       ▼
    ├── Storage ─────── SQLite (fsqlite stack)
    │                       │
    │                       ├── Schema (migrations, JSONL ↔ SQLite sync)
    │                       ├── Events (append-only audit log)
    │                       └── Queries (filtered list, ready, search, graph)
    │
    ├── Sync ───────── JSONL import/export (git-friendly, no auto-git)
    │                       │
    │                       ├── Path resolution (.beads/ discovery)
    │                       └── History (snapshot restore, prune)
    │
    ├── Model ──────── Issue, Dependency, Comment, Event, Label
    │                       │
    │                       └── Content hashing (SHA-256 dedup)
    │
    ├── Config ─────── Layered config (file + env + CLI flags)
    │                       │
    │                       └── Routing (project-aware config resolution)
    │
    ├── Format ─────── Rich (panels, tables, colors), Plain, CSV, Markdown, Syntax
    │
    ├── Output ─────── Mode detection (TTY → Rich, pipe → Plain, --json → JSON)
    │                       │
    │                       └── Components (reusable output widgets)
    │
    ├── Validation ─── Input validation (titles, IDs, priorities, dates)
    │
    └── Error ──────── Structured errors with exit codes (BeadsError + ErrorCode)
```

### Project Structure

```
beads_rust/
├── Cargo.toml                     # Single crate (not a workspace)
├── src/
│   ├── main.rs                    # CLI entry point, clap dispatch
│   ├── lib.rs                     # Library root, module declarations
│   ├── cli/
│   │   ├── mod.rs                 # CLI argument parsing, output mode detection
│   │   └── commands/              # 35+ subcommand implementations
│   ├── model/
│   │   └── mod.rs                 # Issue, Dependency, Comment, Event, Label types
│   ├── storage/
│   │   ├── mod.rs                 # Storage trait
│   │   ├── sqlite.rs              # SQLite backend (181KB — the core engine)
│   │   ├── schema.rs              # DDL migrations
│   │   ├── events.rs              # Append-only audit log
│   │   └── queries/               # Reusable query fragments
│   ├── sync/
│   │   ├── mod.rs                 # JSONL import/export (176KB)
│   │   ├── path.rs                # .beads/ directory discovery
│   │   └── history.rs             # Snapshot restore and prune
│   ├── config/
│   │   ├── mod.rs                 # Layered configuration
│   │   └── routing.rs             # Project-aware config resolution
│   ├── error/
│   │   ├── mod.rs                 # BeadsError enum
│   │   ├── structured.rs          # StructuredError with ErrorCode + exit codes
│   │   └── context.rs             # Error context helpers
│   ├── format/
│   │   ├── mod.rs                 # Format module root
│   │   ├── rich.rs                # Rich terminal output (panels, tables)
│   │   ├── text.rs                # Plain text formatting
│   │   ├── csv.rs                 # CSV export
│   │   ├── markdown.rs            # Markdown formatting
│   │   ├── syntax.rs              # Syntax highlighting
│   │   ├── theme.rs               # Color themes
│   │   ├── context.rs             # Format context (width, mode)
│   │   └── output.rs              # Output helpers
│   ├── output/
│   │   ├── mod.rs                 # Output mode detection (Rich/Plain/JSON/Quiet)
│   │   ├── context.rs             # Output context
│   │   ├── theme.rs               # Output theming
│   │   └── components/            # Reusable output widgets
│   ├── validation/
│   │   └── mod.rs                 # Input validation rules
│   ├── util/
│   │   ├── mod.rs                 # Utility module root
│   │   ├── id.rs                  # Hash-based short ID generation
│   │   ├── hash.rs                # SHA-256 content hashing
│   │   ├── time.rs                # Timestamp parsing/formatting
│   │   ├── progress.rs            # Progress spinners
│   │   └── markdown_import.rs     # Markdown file import
│   └── logging.rs                 # tracing-subscriber setup
├── tests/                         # Integration, conformance, property, regression tests
├── benches/                       # Criterion benchmarks
├── docs/                          # Architecture, CLI reference, troubleshooting
└── .beads/                        # Self-tracked issues (beads tracking beads)
```

### Key Files by Module

| Module | Key Files | Purpose |
|--------|-----------|---------|
| `cli` | `cli/mod.rs` | Clap argument parsing, output mode detection, 66KB dispatch logic |
| `cli/commands` | `commands/*.rs` | 35+ subcommands: create, list, show, close, update, dep, sync, search, query, ready, graph, audit, etc. |
| `model` | `model/mod.rs` | `Issue`, `Dependency`, `Comment`, `Event`, `Label` types, content hashing, serde derives |
| `storage` | `storage/sqlite.rs` | Core SQLite engine (181KB): CRUD, filtered queries, dependency graph, search, events |
| `storage` | `storage/schema.rs` | DDL migrations, table creation, index management |
| `storage` | `storage/events.rs` | Append-only audit log for all issue mutations |
| `sync` | `sync/mod.rs` | JSONL import/export engine (176KB): merge, dedup, conflict resolution |
| `sync` | `sync/path.rs` | `.beads/` directory discovery and path resolution |
| `sync` | `sync/history.rs` | Snapshot-based history: restore, prune, diff |
| `config` | `config/mod.rs` | Layered config: file + env vars + CLI flags, project-aware resolution |
| `error` | `error/structured.rs` | `StructuredError` with `ErrorCode` enum and deterministic exit codes |
| `validation` | `validation/mod.rs` | Input validation: titles, IDs, priorities, dates, labels |
| `util` | `util/id.rs` | Hash-based short ID generation (e.g., `proj-abc12`) |
| `util` | `util/hash.rs` | SHA-256 content hashing for deduplication |
| `format` | `format/rich.rs` | Rich terminal output via `rich_rust` (panels, tables, colors) |

### Feature Flags

```toml
[features]
default = ["self_update"]
self_update = ["dep:self_update"]   # Self-update from GitHub releases (rustls TLS, signature verification)
```

### Core Types Quick Reference

| Type | Purpose |
|------|---------|
| `Issue` | Core data type: title, description, status, priority, type, labels, timestamps, content hash |
| `Dependency` | Directed edge: `from` blocks `to`, with optional label |
| `Comment` | Timestamped comment attached to an issue |
| `Event` | Append-only audit entry (created, updated, closed, reopened, etc.) |
| `Label` | Categorization tag with optional color |
| `BeadsError` | Unified error enum (thiserror-derived) with structured variants |
| `ErrorCode` | Deterministic exit code mapping (e.g., `IssueNotFound` = exit 3) |
| `StructuredError` | JSON-serializable error with code, message, context |
| `OutputMode` | Enum: `Rich`, `Plain`, `Json`, `Toon`, `Quiet` — auto-detected from flags, env, and terminal state |

### Key Design Decisions

- **Non-invasive by design** — `br` NEVER executes git commands automatically; all git operations are explicit user actions
- **SQLite + JSONL hybrid** — Primary storage is SQLite for speed; JSONL export for git-based sync and human readability
- **Content-addressed deduplication** — SHA-256 content hashes prevent duplicate issues across sync boundaries
- **Hash-based short IDs** — e.g., `proj-abc12` (not auto-increment integers) for stable cross-repo references
- **Go parity** — Rust `br` produces identical output to Go `bd` for equivalent inputs; conformance tests validate this
- **Schema compatibility** — Database schema matches Go beads for potential cross-tool usage
- **Multiple output modes** — Rich (TTY), Plain (pipe/NO_COLOR), JSON (--json/--robot), Quiet (--quiet) — auto-detected
- **Append-only audit log** — Every mutation recorded in events table for full traceability
- **Layered configuration** — File + env vars + CLI flags with project-aware routing
- **`unsafe_code = "forbid"`** — Zero unsafe code via crate-level lint
- **`clippy::pedantic` + `clippy::nursery`** — Maximum lint strictness enabled

---

## Sync Safety Maintenance

When modifying sync-related code (`src/sync/`, `src/cli/commands/sync.rs`), you MUST follow the maintenance checklist:

**See: [`docs/SYNC_MAINTENANCE_CHECKLIST.md`](docs/SYNC_MAINTENANCE_CHECKLIST.md)**

Quick summary:
1. **No git operations** — Static check: `grep -rn 'Command::new.*git' src/sync/`
2. **Path allowlist** — Verify only `.beads/` files are touched
3. **Run safety tests** — `cargo test e2e_sync --release`
4. **Review logs** — Check for unexpected safety events
5. **Update docs** — If behavior changed

Related documentation:
- [SYNC_SAFETY.md](docs/SYNC_SAFETY.md) — User-facing safety model
- [E2E_SYNC_TESTS.md](docs/E2E_SYNC_TESTS.md) — Test execution guide
- [.beads/SYNC_SAFETY_INVARIANTS.md](.beads/SYNC_SAFETY_INVARIANTS.md) — Technical invariants

---

## Output Modes

br supports multiple output modes for different use cases:

| Mode | When Active | Description |
|------|-------------|-------------|
| **Rich** | TTY with colors | Colored panels, tables, styled text |
| **Plain** | `NO_COLOR` env or `--no-color` | Text output without ANSI codes |
| **JSON** | `--json` or `--robot` | Machine-readable structured output |
| **Toon** | `--format toon`, `BR_OUTPUT_FORMAT=toon`, or `TOON_DEFAULT_FORMAT=toon` | Token-efficient structured output |
| **Quiet** | `--quiet` or `-q` | Minimal output |

### Mode Detection

The output mode is automatically detected:

1. `--json` or `--robot` flags → **JSON mode**
2. `--quiet` flag → **Quiet mode**
3. `BR_OUTPUT_FORMAT` env var or `TOON_DEFAULT_FORMAT` fallback env var can force **JSON** or **Toon** mode
4. `NO_COLOR` env var or `--no-color` → **Plain mode**
5. Non-TTY stdout (piped output) → **Plain mode**
6. Otherwise → **Rich mode** (default for interactive terminals)

See [docs/AGENT_INTEGRATION.md](docs/AGENT_INTEGRATION.md) for agent-oriented
format defaults and `TOON_DEFAULT_FORMAT` examples.

### For Coding Agents

**CRITICAL:** Always use `--json` or `--robot` flags when parsing br output programmatically.

```bash
# CORRECT - stable, parseable output
br list --json | jq '.issues[0]'
br ready --robot

# WRONG - output format may vary based on terminal state
br list | head -1
```

JSON mode guarantees:
- Stable schema (changes are versioned and documented)
- No ANSI escape codes
- Clean stdout (diagnostics go to stderr)
- Exit codes for success/failure

Schema discovery:
- `br schema all --format json` emits JSON Schema documents for the main robot outputs
- `br schema issue-details --format toon` for token-efficient schema viewing

