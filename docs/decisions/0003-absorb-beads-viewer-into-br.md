<!--
governed-by: ADR-0003
-->
---
status: proposed
date: 2026-08-23
decision-makers: Captain (bangedorrunt)
consulted: docs/research/bv-robot-surface.md, docs/research/bv-tui-ux-map.md, docs/research/bv-analysis-map.md (2026-08-23 study of Dicklesworthstone/beads_viewer @ 4fc261a)
informed: flywheel maintainers (src/work.rs Bv pass-through switch); toron (no change)
---

# ADR-0003: Absorb beads_viewer (bv) into br — robot commands, graph analysis, and TUI

**Status:** Proposed (awaiting captain acceptance). Implements ADR-0001 Wave 2 + the TUI ask.
**Companion:** ADR-0001 §12 Wave 2 ("`br next` / `br triage` / `br plan` absorb `bv --robot-*`; `bv` alias").

> **Scope:** retire the separate `bv` binary from the flywheel × toron toolchain. `br` gains (1) the robot command surface flywheel consumes, (2) the graph analysis engine that powers it, and (3) a TUI so bare `br` in a TTY opens an interactive viewer with bv's keyboard UX. One tracker binary, one storage path, no JSONL shadow-read.

---

## 1. TL;DR

Today flywheel shells out to `bv --robot-{triage,next,insights,plan}` (verified: `flywheel/src/work.rs`, no other flags), and agents use the wider `--robot-*` set ad hoc. `bv` is a Go binary that re-reads `.beads/issues.jsonl` (never our SQLite), caches analysis on disk, and drifts from the source of truth.

**Decision:** port bv's used surface into `br` natively.

1. `br triage`, `br next`, `br plan`, `br insights` subcommands, JSON parity with `bv --robot-*` shapes (envelope, `status`, `data_hash`).
2. `bv` compat: when `argv[0]` is `bv`, or via `--robot-*` flags, br maps old invocations 1:1 (busybox pattern). No wrapper scripts.
3. Graph engine: vendor `bv-graph-wasm` (same-author, same-license) as `src/graph/` plain Rust — proven viable, 196/196 tests green after removing wasm-bindgen.
4. Analysis engine `src/analysis/`: Phase 1/Phase 2 tiers, `MetricStatus`, `data_hash` — computed from SQLite, not JSONL re-read.
5. TUI `src/tui/` on ratatui 0.30 + our existing crossterm 0.29: bare `br` in a TTY opens it; piped `br` keeps help. UX contract = bv's (focus model, single-key views, split pane, footer, shortcuts sidebar) + fork-native surfaces (gate/VERIFY status, fail-closed close).
6. Flywheel switches `Bv::robot(flag)` → `br <flag> --json` in a flywheel-repo PR (their tree, not ours). Then `bv` can be uninstalled.

Cite: `subtract-before-you-add` — delete the second binary, second data path, second cache. `prove-it-works` — golden fixtures from real `bv` output gate the port.

## 2. Context

- `bv` (Dicklesworthstone/beads_viewer, Go + Bubble Tea, ~100k LOC with 62k-line UI) is a *viewer*: it parses `.beads/issues.jsonl` or legacy `beads.jsonl`, never the SQLite DB, and maintains its own `.bv/` analysis cache. Every parity bug upstream (loader drift, cache staleness, `bd` schema forks) is a maintenance tax on us.
- Flywheel's only hard dependency is four commands (`robot.rs`: triage/next/insights/plan) parsed as JSON.
- The fork charter (ADR-0001 §6) already names this absorption as Wave 2, proof: "br next --json matches current bv --robot-next shape on a fixture workspace".
- License: beads_viewer carries the same "MIT + OpenAI/Anthropic rider" as our fork parent beads_rust (verified 2026-08-23: both LICENSE files identical). The fork already lives under these terms; vendoring same-author code adds no new exposure. The rider text ships unmodified in the vendored subtree notice.
- The `bv-graph-wasm` crate inside beads_viewer is already Rust: PageRank, Brandes betweenness (exact+approx), HITS, eigenvector, k-core (BZ), articulation (+bridges), Tarjan SCC, Johnson cycles, critical path/heights, slack, reachability, what-if. Only 2 of its files touch wasm-bindgen; a plain-Rust shim compiles it clean (proven: `/tmp/bv_vendor_test`, 196/196 tests, edition 2024, no wasm deps).

## 3. Decision detail

### 3.1 Robot subcommands (parity set)

| br command | Replaces | Parity target |
| :--- | :--- | :--- |
| `br triage [--brief] [--by-track\|--by-label] [--recipe R] [--label L]` | `bv --robot-triage` | full TriageResult: meta, status, quick_ref, recommendations (score breakdown), quick_wins, blockers_to_clear, project_health, alerts, commands |
| `br next [--recipe R]` | `bv --robot-next` | fail-closed claim contract: `claim_command` only when claim-safe (open, unassigned, unblocked, non-epic, not deferred, no not-ready label) AND PageRank+Betweenness not degraded |
| `br plan [--label L] [--recipe R]` | `bv --robot-plan` | tracks = connected components over actionable set; Phase-2 metrics off with explicit skip reasons |
| `br insights [--label L]` | `bv --robot-insights` | Bottlenecks/Keystones/Influencers/Hubs/Authorities/Cores/Articulation/Slack/Orphans/Cycles/ClusterDensity/Velocity + full_stats |
| `br robot --capabilities` (and argv0 `bv` flags) | `bv --robot-capabilities` | manifest with `contract_version` |

Later (documented, not wave-blocking): `priority`, `label-health`, `label-flow`, `label-attention`, `alerts`, `suggest`, `graph`, `recipes`, `history`, `diff`, `forecast`, `burndown`. Port on demand; the compat layer maps them to a clear "not ported, use br <equivalent>" error, never silently wrong JSON.

**Envelope contract (all robot output):** stdout = JSON only, stderr = diagnostics, exit 0/1/2. Fields: `generated_at`, `data_hash`, `output_format`, `version`, `status` (PascalCase per-metric `{state: computed|approx|timeout|skipped, reason?, sample?, ms?}`), `usage_hints`. `data_hash` reproduces bv's algorithm (SHA-256 over NUL-separated, ID-sorted issue fields incl. sorted labels+deps, truncated to 16 hex) so agents can detect data changes identically; `data_hash == "empty"` for empty store.

**Scores:** structure parity is mandatory; float parity with Go is best-effort. We use the vendored Rust crate's parameter sets. Golden fixtures assert rank stability (same top pick, same ordering) and field shape, not bit-equal floats.

### 3.2 Analysis engine

- `src/graph/` — vendored algorithms (MIT+rider notice preserved), fixes applied: self-loop SCC bug (Go counts single-node self-loops as cycles; fix Tarjan + has_cycles), recursive→iterative DFS for Johnson/articulation (deep chains), getrandom removed (seeded sampling from our RNG), clippy pedantic+nursery clean (`-D warnings` gate stays).
- `src/analysis/` — builds the blocker→blocked directed graph **from SQLite** (issues + dependencies tables, the same rows `br ready` trusts), runs Phase 1 (degree, topo, density, components — instant) and Phase 2 (PageRank, betweenness, HITS, eigenvector, critical path, cycles, k-core, articulation, slack — budgeted per `ConfigForSize` tiers: <100 exact/2s, <500 exact/500ms, <2000 approx-if-sparse/300-500ms, ≥2000 approx/200-500ms + cycles off).
- No disk cache in wave one: SQLite read is fast enough at our scale (<2000 issues/workspace); `data_hash` keys any future cache. Deleting bv's cache problem is a feature.
- `src/analysis/triage.rs` — scoring per bv (base = 0.22·pagerank + 0.20·betweenness + 0.13·blocker_ratio + 0.10·priority + 0.10·time_to_impact + 0.10·urgency + 0.10·risk + 0.05·staleness; triage = 0.70·base + unblock_boost(≤0.15) + quick_win_boost(≤0.15); sort score desc, id asc).

### 3.3 TUI

- Bare `br` (no subcommand): TTY → TUI; non-TTY → today's help text. `--no-tui` flag forces help. Output-mode discipline unchanged (`--json` etc. never open a TUI).
- Stack: `ratatui` 0.30 (crossterm 0.29 — already our pinned dep), no new terminal crates.
- **UX contract (ports bv exactly; see docs/research/bv-tui-ux-map.md for the full tables):**
  - No tab bar. Single-key view toggles from anywhere (unclaimed keys fall through): `b` board, `g` graph, `a` actionable plan, `h` history, `i` insights, `E` tree, `[`/f3 label dashboard, `]`/f4 attention, `f` flow matrix, `'` recipes, `l` label picker, `!` alerts, `?`/f1 help, `` ` `` tutorial-later.
  - Globals: `ctrl+c` quit; `q`/`esc` layered close (esc clears filters at top, then quit-confirm); `tab` split-pane focus toggle; `<`/`>` resize 5%; `;`/f2 shortcuts sidebar; ctrl+j/k sidebar scroll; ctrl+r/f5 refresh.
  - Vim idioms: j/k h/l g/G ctrl+d/u gg-combo (board/tree), Enter = drill-down to detail, `/` search (fuzzy), `y` copy ID, `C` copy issue.
  - Layout: width > 100 → split list|detail with rounded-border panes, focus = purple border; else single column. One-line footer status bar (counts, filter, `◌ metrics…` until Phase 2 lands). Modals replace body.
  - Theme: Dracula dark / WCAG-AA light, adaptive; status/priority/type badge color groups from the spec (§6).
  - Two-phase: Phase 1 renders immediately; Phase 2 metrics compute on a background thread, snapshot swaps atomically, footer indicator flips.
- **Fork-native additions (bv cannot know these):** VERIFY fence + gate status shown on detail view; wave label awareness in board columns; close action surfaces the fail-closed gate requirements (ADR-0001) instead of bv's naive "mark closed".
- **Not ported (explicit):** sprints (vestigial in bv — no opening key), cass modal, update modal (br has its own self-update), repo/workspace picker (single-workspace tool), semantic search (phase later), time-travel input (needs git JSONL archaeology; later), mouse (wheel + basic click only, matching bv's minimal support).
- TUI is read-mostly: claim (`c`) is offered via a confirm modal that shells nothing — it calls the storage layer directly, same as `br update --status in_progress`. Close from TUI always routes through the ADR-0001 gate check, never a raw status write.

### 3.4 bv compatibility

- `argv[0] == "bv"` (symlink/copy/alias) or `--robot-*` flags → compat mode: flags map 1:1 (`--robot-triage` → triage subcommand logic), output identical shapes. Unknown `--robot-X` → exit 2 with `{"error": ..., "not_ported": true}` on stderr and a nonzero exit; never emit subtly-wrong JSON.
- `br capabilities` grows a `bv_compat` section declaring the mapping (machine-readable, one place).

## 4. Consequences

- One binary, one storage path. Flywheel's `Bv::robot()` wrapper becomes `br <flag> --json` (flywheel PR, mechanical). `bv` uninstall becomes safe once skills/docs update.
- We own the algorithms forever (fork policy: no upstream tracking). The vendored crate is textbook math with tests; maintenance risk is low and bounded.
- TUI adds ~8-12k LOC of UI code and ratatui dep. Rejected alternatives: keep bv as-is (two binaries, JSONL drift, Go toolchain we don't want), or port TUI to a webview (forbidden complexity).
- Parity burden: golden fixtures (`tests/fixtures/bv_parity/`) pin the four flywheel-critical shapes, generated from real bv v0.21 output on a 12-issue fixture workspace; recomputable via `scripts/regen_bv_golden.sh` (requires bv installed) but committed so CI never needs Go.

## 5. Proof (Wave-2 gate)

1. `cargo test robot_parity` — golden fixture tests: `br triage|next|plan|insights --json` on the fixture workspace produce field-shape-identical output (structural diff ignoring volatile fields: `generated_at`, `data_hash`, `version`, `*.ms`), rank-stable recommendations, and the same `claim_command`/degraded semantics as the committed bv goldens.
2. `cargo test` green; `cargo clippy --all-targets -- -D warnings` green; `cargo fmt --check` green.
3. `br next --json` on a fixture where the top pick is claimed/blocked/deferred emits the degraded block and **no** `claim_command` (fail-closed preserved).
4. TUI: `br` in a pseudo-TTY opens, all view-toggle keys switch views, `q`/`esc` layer-close matches the spec, quit-confirm at top. Tested via a headless event-driver harness (`tests/tui_harness.rs` asserts the focus state machine), plus a smoke screenshot in CI artifacts.
5. argv0 test: `ln -s br bv && ./bv --robot-next --json` equals `br next --json`.

## 6. Implementation order (filed as beads)

1. **beads: vendor graph crate** → `src/graph/` + fixes + tests (blocked-by: nothing)
2. **beads: analysis engine** → `src/analysis/` (graph from SQLite, tiers, status, data_hash) (depends 1)
3. **beads: triage+next** → scoring, quick_ref, claim-safety, golden tests (depends 2)
4. **beads: plan+insights** → tracks + metric maps, goldens (depends 2)
5. **beads: robot envelope + capabilities + argv0 compat** (depends 3,4)
6. **beads: TUI skeleton** → layout/split/footer/theme/focus, bare-br entry, list+detail (depends 2)
7. **beads: TUI views** → board/graph/actionable/insights/tree/label dashboard (depends 6)
8. **beads: TUI extras** → search/filter, shortcuts sidebar, help overlay, keybinding registry (depends 6)
9. **flywheel PR** (their repo): `Bv::robot(flag)` → `br <flag> --json`; skill/doc updates.
