# bv (beads_viewer) graph-analysis engine — port map for `br`

Study of `/tmp/bv_study` (Dicklesworthstone/beads_viewer), Go `pkg/analysis/*.go` plus the
already-Rust crate `bv-graph-wasm/`. Purpose: decide reuse vs re-implement when porting into
`br`, and record exact algorithm semantics so a port can be validated against them.

Conventions used below:

- **Edge direction (Go analyzer, and how bv's JS builds the WASM graph):** `u -> v` means
  *u depends on v* (v blocks u). Only "blocking" dependency types become edges
  (`dep.Type.IsBlocking()`); "related" links are excluded from the analysis graph.
- **WASM crate closed-set family exception:** the Rust crate's `reachability.rs`, `whatif.rs`,
  `topk_set.rs`, `parallel_cut.rs` treat *predecessors as blockers* — i.e. they assume
  `u -> v` means *u blocks v*. The crate's own unit tests build graphs that way. See
  "Edge-direction landmine" in §12.
- Heights/slack/k-paths are *unweighted* everywhere: edge weight = 1. Priority and
  `estimated_minutes` never enter these algorithms.

## 1. PageRank

| Aspect | Go (`computePageRank`, graph.go) | Rust (`algorithms/pagerank.rs`) |
|---|---|---|
| Damping | 0.85 (caller passes `0.85`) | 0.85 (`PageRankConfig::default`) |
| Tolerance | 1e-6, **L2**: sqrt(sum sq diff) < tol | 1e-6, **L1**: sum abs diff < tol |
| Max iterations | **1000** | **100** |
| Init | uniform 1/n | uniform 1/n |
| Dangling nodes | mass summed, redistributed uniformly (`d * dangling / n`) | identical |
| Self-loops | no special handling (a self-edge is just an out-edge) | identical |
| Normalization | implicit: scores sum to 1 (tested) | identical |

Semantics identical; iteration cap and convergence norm differ (Rust can stop earlier on big
graphs; values agree to <1e-5 on goldens). Both are textbook power iteration.

## 2. Betweenness

- **Exact: Brandes' algorithm**, unweighted, BFS shortest paths, raw (unnormalized) scores —
  no division by `(n-1)(n-2)`. Go's exact path calls gonum `network.Betweenness` (raw Brandes,
  map of nonzero scores). Rust has its own `single_source_betweenness` — same recurrence.
- **Approx: pivot sampling.** Sample k sources (Fisher–Yates over 0..n, seeded), run Brandes
  from each pivot only, then **scale all scores by n/k**. Error O(1/sqrt(k)) (k=100 ≈ 10%
  ranking error). Falls back to exact when `k >= n`.
  - Go: pivots run in parallel goroutines (NumCPU semaphore), pooled buffers; `seed=1` from
    the analyzer; timeout can abandon it (`TimedOut`).
  - Rust: sequential, LCG shuffle, `seed: Option<u64>` (None → getrandom, replaced in port).
- **Sample size heuristic** (`RecommendSampleSize`, both): n<100 → exact (k=n);
  100–499 → max(50, n/5); 500–1999 → 100; ≥2000 → 200. (Go takes edgeCount arg, unused.)
- Mode selection (Go `ConfigForSize`): <500 nodes → exact; 500–2000 → approx only if
  density < 0.01, else **skip** ("graph too dense"); ≥2000 → approx always (HITS skipped
  unless density < 0.001).

## 3. HITS

- **Go: gonum `network.HITS(g, 1e-3)`** — tol 1e-3 on the 2-norm of per-iteration deltas,
  **no max-iteration cap** (loops until converged; only skipped when graph has 0 edges).
  Init auth=hub=1; L2-normalize both each iteration.
- **Rust: own implementation** — tol **1e-6 (L1 sum of both vectors' abs diffs)**, max **100**
  iterations, init 1/n, L2 normalize. Returns `{hubs, authorities, iterations}`.
- Same update equations; parameters differ enough that scores can differ slightly; ranking
  order is stable.

## 4. Eigenvector centrality

- Power iteration on **incoming edges** (`score[v] += score[u]` for u→v), L2-normalized each
  step, init uniform.
- Go: fixed **50 iterations**, no early-exit except zero-norm (returns uniform).
- Rust: `EigenvectorConfig{iterations: 50, tolerance: 1e-6}` with early exit on L1 diff;
  zero-norm → uniform. Values cross-validated on goldens (1e-5).

## 5. k-core

- **Undirected view** of the blocking graph; self-loops skipped; mutual edges (u→v, v→u)
  collapse to one undirected neighbor (Go: sort+dedupe; Rust: HashSet).
- **Go: Batagelj–Zaveršnik** linear-time bin-sort peeling, O(V+E). Core number = final degree.
- **Rust: bucket k-peeling** (degree buckets, remove-min loop). Same definition (max k such
  that node is in the k-core), asymptotically similar; Rust's bucket removal is
  `Vec::retain`-based (slightly worse constant) but fine at br scale.
- `degeneracy = max core number`; `nodes_in_kcore(k)` = nodes with core ≥ k.

## 6. Critical path / longest path

- `computeHeights` (Go) ≡ `critical_path_heights` (Rust): process nodes in topological order;
  `height[v] = 1 + max(height of predecessors)`; roots get 1. **Unweighted** (no priority, no
  estimates). Under the `u→v = u depends on v` convention this is the **depth of the
  downstream cascade**: how many issues (incl. v) sit in the longest chain v transitively
  blocks... precisely: predecessors(v) = dependents of v, so height counts v plus its deepest
  chain of dependents.
- **DAG handling:** Go only computes heights when `topo.Sort` succeeds (cyclic → metric
  absent). Rust returns all-zeros on cyclic graphs.
- `critical_path_nodes` = nodes within 0.001 of max height. `critical_path_length` = max
  height. Go's `TopologicalOrder` output is **reversed** topo.Sort so dependencies come first.

## 7. Cycles

- **SCC: Tarjan**, both sides (Go via gonum `topo.TarjanSCC`, Rust own).
  - `has_cycles` — **Go: SCC size > 1 OR single-node self-loop.**
  **Rust: SCC size > 1 only — self-loops are NOT counted.** Real divergence; fix on port
  (br cares: self-dependency is a user error worth flagging).
- **Cycle listing:**
  - Go `findCyclesSafe` (graph_cycles.go): **one representative cycle per non-trivial SCC**
    (bounded, no exponential blowup), then truncates to `MaxCyclesToStore`; JSON keeps a
    `truncated` reason.
  - Rust: **Johnson's algorithm** (`enumerate_cycles`, full elementary-cycle enumeration with
    `max_cycles` cap + `truncated` flag) — strictly more than Go. Recursive implementation.
- **Cycle break suggestions:**
  - Rust `cycle_break_suggestions`: for each intra-SCC edge count `cycles_broken` (membership
    in enumerated cycles) and `collateral` (degree sum of endpoints); sort by cycles_broken
    desc, collateral asc. `quick_cycle_break_edges`: SCC-membership heuristic without
    enumeration.
  - Go `generateCycleBreakSuggestions` (advanced_insights.go): works from the already-found
    cycle list + `countDependents` (no Johnson).
- **Cycle warnings** (cycle_warnings.go, Go-only): turns cycle list into human/agent advice.

## 8. Remaining families — one-line semantics

- **Articulation points** — Tarjan low-link on the **undirected view** (self-loops skipped,
  neighbors deduped); root counts if >1 DFS child, else `low[child] >= disc[v]`. Cut vertices
  = coordination points whose blocking disconnects work groups.
- **Bridges** (Rust only) — cut edges on the same undirected view; Go has no bridges.
- **Slack** — forward/backward longest-path DP over the DAG: `slack[v] = L - (dfs[v] +
  dte[v] - 1)` where L is the graph's longest path (node count, unweighted); zero-slack =
  on critical path. Go and Rust formulas differ syntactically by the same −1 on both sides,
  so values are equal (goldens 1e-6). Cyclic → Go skips, Rust zeros.
- **Coverage set** — greedy vertex cover, 2-approx: repeatedly pick the node covering most
  uncovered *blocking* edges (tie → lexicographic), until covered or `limit` (Go: open-open
  edges only, default limit 5; Rust: all edges, default 10).
- **Parallel cut** — for each open node v, `parallel_gain = (# open successors w whose other
  blockers are all closed) − 1`; report gain > 0 sorted desc (Rust also has
  `unblock_ranking` without the gain filter). Go's variant works on issue maps with
  blockerOf/blockedBy and reports `max_parallel`.
- **k_paths** — k longest paths: DP longest distance per node in topo order, pick k nodes
  with max dist, reconstruct via pred chain. Go restricts to open issues, path length cap 50,
  min-heap Kahn for determinism; Rust: whole graph, default k=5, cyclic → empty.
- **topk_set** — greedy submodular "maximum unlock": repeatedly pick the open node whose
  simulated close yields the most transitive unblocks (ties → lowest ID), mark it and its
  cascade closed, repeat k times (default 5). O(n²·k).
- **Subgraph** — induced subgraph on a node set (renumbered indices); by-IDs convenience;
  `reachable_subgraph_from`.
- **Reachability** — `reachable_from` (forward BFS), `reachable_to` (reverse BFS),
  `dependency_cone` = ancestors ∪ node ∪ descendants; direct `blockers`/`dependents`;
  `open_blockers`, `open_blocker_count`.
- **whatif** — `what_if_close(v)`: mark v closed, find successors newly actionable (direct),
  then BFS cascade simulating each unblocked node completing (`cascade_ids`,
  `transitive_unblocks`); `parallel_gain = direct − 1`. Batch and ranked variants
  (`top_what_if` over actionable, `all_what_if` over all open). Go equivalent:
  `TopWhatIfDeltas` + `computeMarginalUnblocks` on the issue model.

## 9. Rust crate vs Go — coverage, license

**Coverage.** The WASM crate already contains Rust ports of every Phase-2 numeric metric and
more graph algorithms than Go's analyzer has:

| Family | Go pkg/analysis | bv-graph-wasm |
|---|---|---|
| PageRank, eigenvector, HITS, betweenness exact+approx, k-core, articulation, slack, critical path, SCC | ✓ | ✓ (own impls, golden-tested vs Go at 1e-5) |
| topk_set, coverage, k_paths, parallel_cut, cycle break | ✓ (advanced_insights.go, issue-map based) | ✓ (index-graph based) |
| Johnson cycle enumeration, bridges, quick cycle-break, subgraph/cone | — (SCC-representative cycles only) | ✓ |
| Golden cross-validation | generator (testdata/graphs + expected) | consumer (5 graphs: chain_10, diamond_5, star_10, cycle_5, complex_20) |

**What the Rust crate is missing vs Go** is not graph math — it is everything around it:
phase/config/timeout machinery, in-process incremental cache, robot disk cache, and the
higher-level features (§10/§11 below + triage/priority/risk/eta/plan/label-health/diff/
duplicates/dependency-suggest/feedback/suggestions).

**License.** Repo root LICENSE = **"MIT License (with OpenAI/Anthropic Rider)"** — MIT plus a
rider revoking all rights to OpenAI/Anthropic and "Restricted Parties", and requiring the
rider to ship unmodified with the Software **and all Derivative Works**. `bv-graph-wasm/`
has **no LICENSE file of its own**; its Cargo.toml says `license = "MIT"` (understates the
rider). For personal/fork use by bangedorrunt this permits use, but vendoring into `br`
makes `br` a Derivative Work that must carry the rider (and thus can never be served to
those parties). Alternative: clean-room reimplementation — every algorithm here is textbook
(Brandes 2001, Tarjan, Johnson 1975, Batagelj–Zaveršnik, power iteration) and the Go+Rust
sources double as reference semantics.

## 10. Phase 1 / Phase 2 model (Go)

- **Phase 1 (synchronous, instant):** in/out degree per node, Kahn topological order
  (reversed to dependencies-first), node/edge counts, density. Stored on `GraphStats`
  immediately; returned by `AnalyzeAsync`.
- **Phase 2 (background goroutine, per-metric timeouts):** PageRank, betweenness, HITS,
  eigenvector, critical path heights, cycles, k-core, articulation (computed together),
  slack, then derived per-metric **ranks** (dense descending rank over each score map).
  Readers wait via `WaitForPhase2` / poll `IsPhase2Ready`.
- **Timeouts (500 ms default tier; per size tier):** small <100 nodes → 2 s and exact
  everything, MaxCycles 1000; medium <500 → 500 ms, cycles 100; large <2000 → 300 ms,
  cycles 50, betweenness approx-or-skip by density; XL ≥2000 → PR 200 ms, cycles skipped
  ("graph too large"), betweenness approx k=200, HITS only if density < 0.001. Forced full
  analysis: 30 s everywhere. Triage config: PR + approx betweenness only (k=50, 200 ms), all
  else off. Env overrides: `BV_SKIP_PHASE2=1`, `BV_PHASE2_TIMEOUT_S=N`.
- **Degradation:** each metric's status entry is `computed | timeout | skipped` plus
  elapsed; betweenness additionally `approx` reason + sample size; cycles add `truncated`.
  On PageRank timeout Go substitutes **uniform 1/n scores** (never leaves the metric empty).
  Betweenness/HITS/cycles timeout just leaves maps empty. Metric computation runs in a
  goroutine with a `recover` so a panic degrades to timeout.

## 11. Disk cache (robot-analysis cache v3)

- **Enabled only when** `BV_ROBOT=1` and `BV_NO_CACHE != 1`.
- **Directory:** `$BV_CACHE_DIR` or `<UserCacheDir>/bv`, subdir `analysis_cache/` — **one
  JSON file per entry** (v3; v2 single `analysis_cache.json` retired on write).
- **Key:** `fullKey = dataHash + "|" + configHash`.
  - `dataHash` = SHA-256 over issues **sorted by ID**, hashing ID, title, description,
    notes, design, acceptance criteria, assignee, repo, external ref, status, type,
    priority, estimated minutes, created/updated/closed/defer-until, sorted labels, sorted
    dependency tuples (depends_on, type, created_at, created_by), sorted comments. Empty
    input → literal `"empty"`.
  - `configHash` = first 16 hex of SHA-256 over `fmt.Sprintf("%#v", config)`.
  - Filename = first 16 bytes of SHA-256(fullKey) hex + `.json`; full key re-verified
    against the body on read (collision guard).
- **Entry:** `{version: 3, key, created_at, data_hash, config_hash, compute_duration,
  result: <flattened GraphStats>}`; ≤ 10 MB; written atomically (tmp file + fsync + rename)
  under a directory `.lock` (flock on Unix); only stored when Phase 2 completed.
- **Invalidation (read path):** entry dropped if version ≠ 3, key mismatch, JSON corrupt,
  age > **24 h**, or **`.beads/` tree mtime > entry created_at** (BEADS_DB file, else
  BEADS_DIR, else `./.beads`; only top-level files stat'ed — subdirectories deliberately
  skipped for speed; vanishing files like `*.lock` ignored).
- **Eviction:** on write, delete entries older than 24 h; if > **10 entries** remain, evict
  oldest by mtime (FIFO, tie by name); reap stale `tmp-*` files.
- **XFetch:** probabilistic early refresh (prevents stampede) — a hit may still recompute
  if `ComputeDuration` has elapsed and the coin lands on refresh.
- **In-process incremental cache** (non-robot path): key = `graphStructureHash|configHash`
  (SHA-256 over sorted node IDs + sorted deduped edges), LRU-ish with pruning; stores
  Phase-1-complete `GraphStats`.

## 12. Vendoring feasibility (proven by experiment)

I stripped the wasm layer from a copy and built it as a plain Rust crate
(`/tmp/bv_vendor_test`): **it compiles under edition 2024 and all 196 inline unit tests
pass** with exactly these changes:

1. Replace `graph.rs` (600-line wasm-bindgen wrapper) with a ~70-line plain `DiGraph`
   (same fields; expose `successors_slice`/`predecessors_slice`/`len`/`node_count`/
   `edge_count`/`add_node`/`add_edge`/`node_idx`/`node_id`/`in_degree`/`out_degree`/
   `with_capacity`).
2. New 5-line `lib.rs` (no `#[wasm_bindgen(start)]`, no panic hook).
3. `betweenness.rs`: replace the `getrandom::fill` fallback seed with a constant
   (6-line patch). Drop `getrandom`, `wasm-bindgen`, `js-sys`, `serde-wasm-bindgen`,
   `console_error_panic_hook` deps; keep `serde` + `serde_json` only.
4. `critical_path.rs`: one edition-2024 pattern fix (`|(_, &h)|` → `|&(_, &h)|`).

Every other algorithm file is byte-identical. Remaining port work beyond that:

- **Clippy debt:** 145 non-test pedantic+nursery warnings (53 missing `#[must_use]`, 35 doc
  backticks, 13 `usize→f64` casts, ...). br's `-D warnings` gate will require a cleanup
  pass. Test code adds ~94 more (float comparisons etc.).
- **Recursion:** Tarjan/Johnson/articulation DFS are recursive — deep dependency chains can
  overflow the native stack on very large graphs; convert to iterative if br needs hard
  guarantees.
- **Edge-direction landmine:** the crate's closed-set family (whatif, topk_set,
  parallel_cut, reachability.blockers/actionable) assumes **edge u→v = u blocks v**
  (predecessors are blockers, closing u unblocks successors). The crate's own doc comments,
  Go's analyzer, and bv's JS all use **u→v = u depends on v**. When wiring into br, build
  the graph blocker→blocked for these algorithms (or swap predecessor/successor calls) —
  do not trust the doc comments in `graph.rs`.
- **Self-loop detection gap:** Rust `has_cycles` misses single-node self-loop SCCs; Go
  counts them. Patch on port.
- **No LICENSE in the crate dir**; repo rider applies (§9).

## 13. Go-only features absent from the Rust crate (port candidates)

All of `pkg/analysis` beyond graph math — none exist in Rust anywhere:

- `triage.go` / `triage_context.go` — unified `--robot-triage` assembly with cached
  cross-cutting computations.
- `priority.go` — composite ImpactScore with weighted breakdown (urgency labels, blocker
  counts, PageRank/betweenness/eigenvector/HITS/critical-path blending).
- `risk.go` — volatility/risk signals per issue.
- `eta.go` — deterministic ETA: complexity minutes (explicit or median × type weight ×
  depth × description length) ÷ label-velocity (recent closures, fallback global);
  `--robot-forecast`.
- `plan.go` — parallel execution tracks from graph components, WIP-limited.
- `label_health.go` / `label_suggest.go` — per-label health (velocity, staleness, blocked
  counts), cross-label flow, keyword-based label suggestions.
- `diff.go` — snapshot diff between beads states (`--robot-diff`).
- `duplicates.go` — Jaccard keyword-similarity duplicate detection (threshold 0.7).
- `dependency_suggest.go` — potential-dependency suggestions.
- `suggestions.go` / `suggest_all.go` — hygiene suggestions with confidence levels.
- `feedback.go` — feedback sidecar file tuning recommendation weights (bv-90).
- `insights.go` — top-N insights by metric, velocity snapshot.
- `cycle_warnings.go` — human-readable cycle advice.
- Cache/config/phase machinery (§10–11) and gonum dependency itself.

## 14. Recommended port order into `br`

1. **Decide license stance first** (vendor-with-rider vs clean-room). Everything below is
   unchanged either way; only provenance differs.
2. **Core crate:** plain `DiGraph` + topo (Kahn, deterministic) + Tarjan SCC (**with
   self-loop fix**) + heights/critical path + slack. This alone powers `graph`, `ready`
   ordering, and cycle gates.
3. **Betweenness + PageRank + eigenvector + HITS** with Go's exact parameter semantics
   where they matter for interop (damping 0.85; keep Rust's iteration caps as safety) and
   Brandes-approx with seeded sampling for large graphs.
4. **k-core + articulation (+ bridges)** — cheap, self-contained.
5. **Closed-set family** (actionable, open blockers, whatif, topk_set, parallel_cut,
   unblock ranking) — build the graph blocker→blocked per §12 landmine; golden-test
   against bv's Go `computeMarginalUnblocks` outputs.
6. **Johnson cycle enumeration + cycle-break suggestions** (feeds the bead-graph hygiene
   policy: `br dep remove` advice instead of forced closes).
7. **Phase/config/timeout model + status degradation** (`computed|timeout|skipped|approx`)
   and the v3 disk cache keyed `dataHash|configHash` with `.beads/` mtime invalidation —
   adapted to br's own issue model (br already has content hashes; reuse them rather than
   copying bv's field list verbatim).
8. **Higher-level features** (triage, priority, plan, eta, label health...) only as
   needed — these are bv product logic, port selectively, not wholesale.

Provenance note: `/tmp/bv_vendor_test` scratch crate retained for reference
(`cargo test` green, 196/196).
