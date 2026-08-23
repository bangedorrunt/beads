# bv Robot Surface Inventory (agent-facing, JSON)

Source of truth: Go repo `Dicklesworthstone/beads_viewer` studied at `/tmp/bv_study`.
Files: `cmd/bv/main.go` (flags, envelope, capability manifest), `cmd/bv/robot_registry.go`
(all robot handlers + output structs), `pkg/analysis/*.go` (payload structs, scoring),
`pkg/recipe/` (recipes), `pkg/correlation/` (git history), `pkg/drift/` (alerts),
`pkg/export/graph_export.go` (graph). Goal: reference for porting this surface into `br`.

## 0. Invocation model

- Robot commands are **flags**, not subcommands: `bv --robot-triage`, `bv --robot-next`, ...
- An intent-alias rewriter also accepts subcommand style `bv robot-triage --json` and rewrites it to
  `--robot-triage --format json`. `--json` is accepted as an alias for `--format json`.
- Robot mode always emits structured stdout. Format: `--format json|toon`
  (env `BV_OUTPUT_FORMAT`, fallback `TOON_DEFAULT_FORMAT`). JSON is the default.
- Stream contract: **stdout = structured data only; stderr = diagnostics/warnings**.
- Exit codes: 0 success, 1 general error (also drift critical), 2 invalid args (also drift warning).
- `bv --robot-capabilities` prints a machine-readable manifest:
  `{tool, version, contract_version:"1.0.0", default_robot_command:"bv --robot-triage",
  output_formats:["json","toon"], commands[], docs_topics, schema_command, environment_variables,
  exit_codes, stream_contract}`. Each command entry carries
  `name, flag, description, preferred_invocation, accepted_invocations, needs_issues, needs_git,
  needs_sprint, needs_baseline, mutates_state, key_fields, params`.
- Data loading: `.beads/` dir found via `BEADS_DB` > `BEADS_DIR` > `--db` > cwd walk. JSONL names in
  preference order: `issues.jsonl` (current br) > `beads.jsonl` (legacy bd) > `beads.base.jsonl`.
  Issues + inline dependencies are the core input. Comments/events/SQLite DB are NOT read by bv;
  only the JSONL export.

## 1. Common JSON envelope

Every robot output shares (from `RobotEnvelope` or hand-rolled equivalents):

| Field | Type | Notes |
|---|---|---|
| `generated_at` | string (RFC3339 UTC) | always present |
| `data_hash` | string | always present; `"empty"` for empty store |
| `output_format` | string, omitempty | `"json"` or `"toon"` |
| `version` | string, omitempty | bv binary version |
| `load_stats` | object, omitempty | only when JSONL load dropped records: `{source_path, valid:int, errors:int, skipped:int, warnings:[]string}` (#190) |
| `as_of` / `as_of_commit` | string, omitempty | present when `--as-of <ref>` was used (state loaded from git at that ref) |
| `status` | object | metric computation status (graph commands only, see §3) |
| `usage_hints` | []string | jq one-liners, most commands |
| `label_scope` / `label_context` | string / object, omitempty | when `--label <label>` scoped the analysis to a label subgraph |

There is no `schema` or top-level `schema_version` field on payloads; schema versioning lives in
`robot-capabilities.contract_version` and in `robot-schema` output
(`{schema_version, generated_at, envelope, commands}`). The documented envelope requires only
`generated_at` + `data_hash`.

### data_hash (`analysis.ComputeDataHash`)

SHA-256 over all issues **sorted by ID**, NUL-separated fields per issue:
`id, title, description, notes, design, acceptance_criteria, assignee, source_repo, external_ref,
status, issue_type, priority, estimated_minutes, created_at(RFC3339Nano), updated_at,
closed_at, defer_until(RFC3339Nano), labels(sorted), dependencies(sorted by depends_on+type+created_at+created_by)`.
Deterministic across runs and input order; used for the on-disk analysis cache key and to let agents
detect "did the data change". Note: hash is computed over the **pre-recipe/pre-label-scope** issue set
for stability (`dataHashMatchesIssues=false` marks when the emitted Issues differ from the hashed set).

## 2. Two-phase computation model

- **Phase 1 (instant, synchronous):** degree, topological sort, density, connected components,
  actionable/blocked classification, execution plan.
- **Phase 2 (async, time-budgeted):** PageRank, betweenness, HITS, eigenvector, critical-path score,
  cycles, k-core, articulation points, slack. Budgets come from `analysis.ConfigForSize`:

| graph size | betweenness | timeouts | notes |
|---|---|---|---|
| < 100 nodes | exact | 2s | everything on |
| < 500 nodes | exact | 500ms | everything on |
| < 2000 nodes | approximate if density < 0.01, else skipped (`"graph too dense (density > 0.01)"`) | 300–500ms | |
| >= 2000 nodes | approximate | 200–500ms | cycles skipped (`"graph too large (>2000 nodes)"`); HITS only if density < 0.001 |

`--force-full-analysis` overrides to `FullAnalysisConfig()` (all metrics, exact betweenness, 30s).
`--no-cache` bypasses the disk cache.

### `status` object (MetricStatus)

```json
"status": {
  "PageRank":     {"state": "computed", "ms": 12.3},
  "Betweenness":  {"state": "approx", "reason": "approximate", "sample": 512, "ms": 487.0},
  "Cycles":       {"state": "skipped", "reason": "not computed for --robot-plan"}
}
```

Keys (PascalCase, no json tags on the struct): `PageRank, Betweenness, Eigenvector, HITS, Critical,
Cycles, KCore, Articulation, Slack`. Each entry: `{state: computed|approx|timeout|skipped|pending,
reason?: string, sample?: int, ms?: float}`. Handlers that disable metrics for speed set an explicit
skip reason (e.g. robot-plan disables PageRank/betweenness/HITS/eigenvector/critical-path/cycles:
`"not computed for --robot-plan"`). At process exit no entry may still be `pending` (enforced by test).
`robot-next` fails closed: if PageRank or Betweenness is `pending|timeout|panic|error|skipped`
(`ClaimUnsafeReasons`), no `claim_command` is emitted (see §4.2).

## 3. Command-by-command inventory

Payload types are Go structs with json tags as shown. "Needs" column from the capability manifest.

### 3.1 `--robot-triage` (the mega-command)

- **Flags:** `--robot-triage` (bool). Modifiers: `--brief` (#183, compact output),
  `--robot-triage-by-track` / `--robot-triage-by-label` (bv-87 grouping),
  `--graph-root <id>` (bv-140 subgraph scope), `--robot-not-ready-labels "a,b"`
  (env `BV_ROBOT_NOT_READY_LABELS`, #173), `--robot-history-timeout-ms` (default 10000;
  `-1`=unset; 0=unbounded; env `BV_ROBOT_HISTORY_TIMEOUT_MS`), `--history-limit` (default 500 →
  triage prologue actually uses 200 when unset), `--label <label>`, `--recipe <name>`, `--as-of`.
- **Envelope:** `{generated_at, data_hash, load_stats?, as_of?, as_of_commit?, triage: TriageResult,
  feedback?: FeedbackJSON, usage_hints: []string}`.
- **TriageResult:** `{meta: TriageMeta, status?: MetricStatus, quick_ref: QuickRef,
  recommendations: [Recommendation], quick_wins: [QuickWin], blockers_to_clear: [BlockerItem],
  project_health: ProjectHealth, alerts?: [Alert], commands: CommandHelpers,
  recommendations_by_track?: [...], recommendations_by_label?: [...]}`.
  - `meta`: `{version, generated_at, phase2_ready: bool, issue_count, compute_time_ms,
    history_status?: "ok"|"error"|"timeout"}` (history_status empty when git prologue not attempted).
  - `quick_ref`: `{open_count, actionable_count, blocked_count, in_progress_count, not_closed_count,
    not_actionable_count, top_picks: [{id, title, score: float, reasons: [string], unblocks: int}] (top 3)}`.
    Note #165 semantics: `open_count` = status exactly "open"; `not_closed_count` = pre-#165 "open".
  - `recommendations[]` (top 10): `{id, title, type, status, assignee?, priority: int, labels: [],
    defer_until?, score: float, breakdown: ScoreBreakdown (see §5), action: string, reasons: [string],
    unblocks_ids?: [], blocked_by?: []}`.
  - `quick_wins[]` (top 5): `{id, title, score, reason, unblocks_ids?}`.
  - `blockers_to_clear[]` (top 5): `{id, title, unblocks_count, unblocks_ids, actionable: bool, blocked_by?}`.
  - `project_health`: `{counts: {total, open, closed, blocked, actionable, not_closed,
    dependency_blocked, by_status: map, by_type: map, by_priority: map},
    graph: {node_count, edge_count, density, has_cycles, cycle_count?, phase2_ready},
    velocity?: {closed_last_7_days, closed_last_30_days, avg_days_to_close, weekly?, estimated?},
    staleness?: {stale_count, stalest_issue_id, stalest_issue_days, threshold_days}}`.
  - `commands`: `{claim_top, show_top, list_ready, list_blocked, refresh_triage}` — literal `br` shell
    commands, e.g. `claim_top` = `br update <id> --status in_progress --json`, `refresh_triage` = `bv --robot-triage`.
  - `--brief` output instead is `{generated_at, data_hash, load_stats?, as_of?, as_of_commit?,
    brief: true, quick_ref, recommendations: [{id,title,status,assignee?,score,unblocks?,blocked_by?}],
    quick_wins?, blockers_to_clear?}`.
- **Needs:** issues+deps (JSONL); git history prologue best-effort (staleness): bounded 10s, only when
  open issues exist and a git repo is present; feedback file `.beads/feedback.json` if present.
- **Scoring:** see §5.

### 3.2 `--robot-next` (minimal claim)

- **Flags:** `--robot-next`, plus `--graph-root`, `--robot-not-ready-labels`, `--robot-history-timeout-ms`.
- **Envelope (robotNextOutput):** embeds full `RobotEnvelope` plus
  `{as_of?, as_of_commit?, actionable: bool, phase2_ready: bool, status: MetricStatus,
  message?: string, id?, title?, score?, reasons?: [], unblocks?: int,
  diagnostic_top_pick?: {id,title,score,reasons,unblocks}, claim_command?: string,
  show_command?: string, degraded?: [{code, severity, message, repair?}], usage_hints?}`.
- **Semantics:** walks `quick_ref.top_picks` in order and returns the first **claim-safe** pick:
  status == "open" (not in_progress/draft/deferred/review), not an epic, unassigned, not deferred
  (`defer_until` in future, #191), no open blockers, not carrying a not-ready label (#173).
  Emits `claim_command` = `br update <id> --status=in_progress` and `show_command` = `br show <id>`.
  Fail-closed paths (no claim command, exit 0, degraded block):
  `no_actionable_recommendation` (info), `robot_next_claim_unsafe` (warning, includes
  `diagnostic_top_pick` + reasons), `robot_next_metric_incomplete` (warning; PageRank/Betweenness
  incomplete per `ClaimUnsafeReasons`).
- **Needs:** issues+deps only (no git prologue).

### 3.3 `--robot-plan`

- **Flags:** `--robot-plan`, `--label <label>` (scope to label subgraph), `--recipe`, `--as-of`,
  `--force-full-analysis`.
- **Envelope:** `{generated_at, data_hash, as_of?, as_of_commit?, analysis_config, status,
  label_scope?, label_context?, plan: ExecutionPlan, usage_hints}`.
- **ExecutionPlan:** `{tracks: [{track_id, items: [{id, title, priority, status, unblocks: [ids]}],
  reason: string}], total_actionable: int, total_blocked: int,
  summary: {highest_impact: id, impact_reason, unblocks_count: int}}`.
- Tracks = connected components of the full graph filtered to actionable issues (parallelizable work
  streams). Phase-2 metrics are disabled for speed (explicit skip reasons in `status`).
- **Needs:** issues+deps only.

### 3.4 `--robot-priority`

- **Flags:** `--robot-priority`, `--robot-min-confidence 0.0` (filter by `confidence`),
  `--robot-max-results` (default 10), `--robot-by-label`, `--robot-by-assignee`,
  `--label`, `--recipe`, `--as-of`, `--force-full-analysis`.
- **Envelope:** `{generated_at, data_hash, as_of?, as_of_commit?, analysis_config, status,
  label_scope?, label_context?, recommendations: [EnhancedPriorityRecommendation],
  field_descriptions: map<string,string>, filters: {min_confidence?, max_results, by_label?, by_assignee?},
  summary: {total_issues, recommendations, high_confidence}, usage_hints}`.
  `high_confidence` counts items with `confidence >= 0.7`.
- **EnhancedPriorityRecommendation** = `{issue_id, title, current_priority, suggested_priority,
  impact_score: float, confidence: 0..1, reasoning: [string] (top 3), direction: "increase"|"decrease",
  what_if?: WhatIfDelta, explanation: {top_reasons: [{factor, weight, explanation, emoji}],
  what_if?, status: {computed_at, deterministic: true, phase2_ready, capped, capped_fields?}}}`.
- **WhatIfDelta:** `{direct_unblocks, transitive_unblocks, blocked_reduction, depth_reduction,
  estimated_days_saved?, unblocked_issue_ids? (capped 10), parallelization_gain?: int, explanation}`.
- Thresholds (DefaultThresholds): HighPageRank 0.3, HighBetweenness 0.5, StalenessDays 14,
  MinConfidence 0.3, SignificantDelta 0.15.
- **Needs:** issues+deps only.

### 3.5 `--robot-insights`

- **Flags:** `--robot-insights`, `--label`, `--recipe`, `--as-of`, `--force-full-analysis`,
  `BV_INSIGHTS_MAP_LIMIT` (default 200 entries per map).
- **Envelope:** `{generated_at, data_hash, load_stats?, as_of?, as_of_commit?, analysis_config,
  status, label_scope?, label_context?, <Insights fields inlined>, full_stats: {...},
  top_what_ifs?: [WhatIfEntry], advanced_insights?: {cycle_break: ...}, usage_hints}`.
- Inlined `Insights` fields (no json tags → Go names, PascalCase):
  `Bottlenecks, Keystones, Influencers, Hubs, Authorities, Cores: [{ID, Value: float}]`,
  `Articulation: [id]`, `Slack: [{ID, Value}]`, `Orphans: [id]`, `Cycles: [[id...]]`,
  `ClusterDensity: float`, `Velocity: {closed_last_7_days, closed_last_30_days, avg_days_to_close,
  weekly?: [int], estimated?}`.
- `full_stats`: `{pagerank, betweenness, eigenvector, hubs, authorities, critical_path_score:
  map[id]float, core_number: map[id]int, slack: map[id]float, articulation_points: [id]}`
  (maps trimmed to the env limit, sorted by value desc).
- `top_what_ifs`: top 10 what-if deltas (direct_unblocks etc.).
- **Needs:** issues+deps only.

### 3.6 `--robot-label-health`

- **Flags:** `--robot-label-health`.
- **Envelope:** `{generated_at, data_hash, analysis_config: LabelHealthConfig,
  results: LabelAnalysisResult, usage_hints}`.
- **LabelAnalysisResult:** `{generated_at, total_labels, healthy_count (health >= 70),
  warning_count (40–69), critical_count (< 40),
  labels: [LabelHealth], summaries: [{label, issue_count, open_count, health: 0-100,
  health_level: "healthy"|"warning"|"critical", top_issue?, needs_attention: bool}],
  cross_label_flow?: CrossLabelFlow (see 3.7), attention_needed: [label]}`.
- **LabelHealth:** `{label, issue_count, open_count, closed_count, blocked_count,
  health: int 0-100, health_level, velocity: {closed_last_7_days, closed_last_30_days,
  avg_days_to_close, trend_direction: "improving"|"stable"|"declining", trend_percent,
  velocity_score 0-100}, freshness: {most_recent_update, oldest_open_issue, avg_days_since_update,
  stale_count, stale_threshold_days (default 14), freshness_score 0-100},
  flow: {incoming_deps, outgoing_deps, incoming_labels, outgoing_labels, blocked_by_external,
  blocking_external, flow_score 0-100}, criticality: {avg_pagerank, avg_betweenness,
  max_betweenness, critical_path_count, bottleneck_count, criticality_score 0-100},
  issues?: [id]}`.
- **Needs:** issues+deps only.

### 3.7 `--robot-label-flow`

- **Flags:** `--robot-label-flow`.
- **Envelope:** `{generated_at, data_hash, load_stats?, flow: CrossLabelFlow,
  analysis_config: LabelHealthConfig, usage_hints}`.
- **CrossLabelFlow:** `{labels: [string], flow_matrix: [][]int (row=from blocking, col=to blocked;
  align with labels), dependencies: [{from_label, to_label, issue_count, issue_ids?,
  blocking_pairs?: [{blocker_id, blocked_id, blocker_label, blocked_label}]}],
  critical_paths: [{labels: [..], length, issue_count, total_weight}],
  bottleneck_labels: [label] (labels blocking the most others),
  total_cross_label_deps: int}`.
- **Needs:** issues+deps only.

### 3.8 `--robot-label-attention`

- **Flags:** `--robot-label-attention`, `--attention-limit` (default 5).
- **Envelope:** `{generated_at, data_hash, load_stats?, limit, total_labels,
  labels: [{rank: int (1-based), label, attention_score: float, normalized_score: 0-1,
  reason: string, open_count, blocked_count, stale_count, pagerank_sum, velocity_factor}],
  usage_hints}`.
- Formula: `attention = (pagerank_sum * staleness_factor * block_impact) / velocity`;
  staleness_factor = 1 + stale_count/open_count. Higher = needs more attention.
- **Needs:** issues+deps only.

### 3.9 `--robot-history`

- **Flags:** `--robot-history`, `--bead-history <id>` (single bead),
  `--history-since` (relative "30d"/"2w" or ISO date), `--history-limit` (default 500, 0 = unlimited),
  `--min-confidence 0.0`, `--id-pattern` (repeatable regex, #188).
- **Envelope:** inlines `correlation.HistoryReport` + `{output_format?, version?}`:
  `{generated_at, data_hash (of beads.jsonl), git_range (e.g. "HEAD~200..HEAD"),
  latest_commit_sha?, stats: {total_beads, beads_with_commits, total_commits, unique_authors,
  avg_commits_per_bead, avg_cycle_time_days?, method_distribution: map},
  histories: {<bead_id>: {bead_id, title, status, events: [{bead_id, event_type, timestamp,
  commit_sha, commit_message, author, author_email}], milestones: {created?, claimed?, closed?,
  reopened?}, commits: [{sha, short_sha, message, author, author_email, timestamp,
  files: [{path, action: A|M|D|R, insertions, deletions}], method: CorrelationMethod,
  confidence: 0..1, reason}], cycle_time?: {claim_to_close?, create_to_close?, create_to_claim?},
  last_author}}, commit_index: {<sha>: [bead_id...]}}`.
- Correlation methods: commit-message ID match, file-path overlap, title similarity; confidence
  score per commit with reason string. `--min-confidence` filters and rebuilds the index.
- **Needs:** issues (IDs/titles/status) + **git repo** (log with files, bounded by limit).

### 3.10 `--robot-diff`

- **Flags:** `--robot-diff` with `--diff-since <ref>` (commit SHA, branch, tag, or date).
  Auto-enables robot diff in non-TTY/robot contexts. `--as-of` analog loads historical state.
- **Envelope:** `{generated_at, resolved_revision: string, as_of?, as_of_commit?,
  from_data_hash, to_data_hash, diff: SnapshotDiff}`.
- **SnapshotDiff:** `{from_timestamp, to_timestamp, from_revision?, to_revision?,
  new_issues: [Issue], closed_issues: [Issue], removed_issues: [Issue], reopened_issues: [Issue],
  modified_issues: [{issue_id, title, changes: [{field, old_value, new_value}]}],
  new_cycles: [[id]], resolved_cycles: [[id]],
  metric_deltas: {total_issues, open_issues, closed_issues, blocked_issues, total_edges,
  cycle_count, component_count, avg_pagerank, avg_betweenness},
  summary: {total_changes, issues_added, issues_closed, issues_removed, issues_reopened,
  issues_modified, cycles_introduced, cycles_resolved, net_issue_change,
  health_trend: "improving"|"degrading"|"stable"}}`.
- Historical issues loaded from git at the ref (`loader.GitLoader.LoadAt`), diffed against current.
- **Needs:** issues + **git repo** (beads JSONL must be tracked).

### 3.11 `--robot-burndown <sprint-id|current>`

- **Flags:** `--robot-burndown` (string value; `"current"` = active sprint).
- **Envelope (BurndownOutput, embeds RobotEnvelope):** `{generated_at, data_hash, output_format?,
  version?, load_stats?, sprint_id, sprint_name, start_date, end_date, total_days, elapsed_days,
  remaining_days, total_issues, completed_issues, remaining_issues, ideal_burn_rate: float,
  actual_burn_rate: float, projected_complete?: timestamp, on_track: bool,
  daily_points: [{day, remaining, ideal}], ideal_line: [...],
  scope_changes?: [{date, issue_id, issue_title, action: "added"|"removed"}]}`.
- Exits 1 with stderr message when no sprint found / no active sprint.
- **Needs:** issues + sprints (`.bv/sprints.yaml` via `loader.LoadSprints`); scope changes via git.

### 3.12 `--robot-forecast <bead-id|all>`

- **Flags:** `--robot-forecast` (string), `--forecast-label`, `--forecast-sprint <id>`,
  `--forecast-agents` (default 1).
- **Envelope:** `{RobotEnvelope..., agents: int, filters?: {label?, sprint?}, forecast_count: int,
  forecasts: [ETAEstimate], summary?: {total_minutes, total_days (8h/day), avg_confidence,
  earliest_eta, latest_eta}}` (summary only when > 1 forecast).
- **ETAEstimate:** `{issue_id, estimated_minutes: int, estimated_days, eta_date,
  eta_date_low?, eta_date_high?, confidence: 0..1, velocity_minutes_per_day, agents,
  factors?: [string]}`. `all` skips closed issues; single-ID errors exit 1.
- **Needs:** issues+deps (uses graph stats for dependency-aware scheduling); sprints only if
  `--forecast-sprint`.

### 3.13 `--robot-alerts`

- **Flags:** `--robot-alerts`, `--severity info|warning|critical`, `--alert-type <type>`,
  `--alert-label <label>`.
- **Envelope:** `{RobotEnvelope..., alerts: [Alert], summary: {total, critical, warning, info},
  usage_hints}`.
- **drift.Alert:** `{type, severity: "critical"|"warning"|"info", message, baseline_value?,
  current_value?, delta?, details?: [string], issue_id?, label?}`.
- Alert types: `new_cycle, pagerank_change, density_growth, node_count_change, edge_count_change,
  blocked_increase, actionable_change, stale_issue, velocity_drop, blocking_cascade,
  high_impact_unblock, abandoned_claim, potential_duplicate`.
  Proactive ones (stale_issue, blocking_cascade, abandoned_claim, potential_duplicate) run even
  without a baseline; drift ones compare against `.bv/baseline.json` when present.
- **Needs:** issues+deps; baseline optional (`.bv/baseline.json`, from `--save-baseline`).

### 3.14 `--robot-suggest`

- **Flags:** `--robot-suggest`, `--suggest-type duplicate|dependency|label|cycle` (invalid → exit 1),
  `--suggest-confidence 0.0`, `--suggest-bead <id>`.
- **Envelope (RobotSuggestOutput):** `{generated_at, data_hash,
  filters: {type?, min_confidence?, bead_id?},
  suggestions: {suggestions: [Suggestion], generated_at, data_hash?, stats: {...}},
  usage_hints}`.
- **Suggestion:** `{type: missing_dependency|potential_duplicate|label_suggestion|stale_cleanup|
  cycle_warning, target_bead, related_bead?, summary, reason, confidence: 0..1,
  action_command? (ready-to-run br command), generated_at, metadata?: map}`.
  Sorted by confidence desc, capped at 50 (DefaultSuggestAllConfig.MaxSuggestions).
  Confidence bands: low < 0.4 <= medium < 0.7 <= high.
- **Needs:** issues+deps only.

### 3.15 `--robot-graph` (+ `--export-graph`)

- **Flags:** `--robot-graph`, `--graph-format json|dot|mermaid` (default json),
  `--graph-root <id>` (subgraph), `--graph-depth <n>` (0 = unlimited), `--label`.
- **Envelope (GraphExportResult):** `{format: "json"|"dot"|"mermaid", graph?: string (dot/mermaid
  text; empty for json), nodes: int, edges: int,
  filters_applied?: {label?, root?, depth?},
  explanation: {what, how_to_render?, when_to_use}, data_hash?,
  adjacency?: {nodes: [{id, title, status, priority, labels?, pagerank?}],
  edges: [{from, to, type: "blocks"|"related"|"parent-child"|"discovered-from"}]}}`.
- `--export-graph <file.html|.png|.svg>` is a separate flag (not robot-graph): renders an
  interactive HTML snapshot or static image; `--graph-preset compact|roomy`, `--graph-title`.
  The rendered artifact stamps `data_hash` in its footer.
- **Needs:** issues+deps only (graph stats enrich node pagerank).

### 3.16 Recipes (`--recipe`, `--robot-recipes`)

- **Flags:** `--recipe <name>` / `-r`; `--robot-recipes` lists them.
- Implementation (`pkg/recipe`): YAML-declared views. `Recipe = {name, description, filters, sort,
  view, export, metrics}`. Sources layered builtin < user (`~/.config/bv/recipes.yaml`) < project
  (`.bv/recipes.yaml`); `--robot-recipes` emits `{recipes: [{name, description, source:
  "builtin"|"user"|"project"}]}`.
- Filters: `status[], priority[], tags[]/exclude_tags[]` (maps to labels), `created_after/before,
  updated_after/before` (relative "14d"/"2w"/"1m"/"1y" or ISO), `has_blockers: bool`,
  `actionable: bool` (true = no open blockers), `title_contains`, `id_prefix`.
  Sort: `field: priority|created|updated|title|id|pagerank|betweenness|triage` + `direction`.
- Built-ins: `default` (open+in_progress+blocked, priority asc), `actionable` (open+in_progress,
  actionable=true, priority asc), `recent` (updated 7d), `blocked` (has_blockers=true),
  `high-impact` (open+in_progress, sort pagerank desc, max 20), `stale` (updated_before 30d),
  `triage` (actionable, sort triage desc), `closed`.
- Application in robot mode (bv-93): for `--robot-triage/--robot-next/--robot-triage-by-track/
  --robot-triage-by-label/--robot-priority/--robot-insights/--robot-plan`, recipe filters+sort are
  applied to the issue set **before** the handler runs; `data_hash` still describes the unfiltered
  set. So `bv --recipe high-impact --robot-plan` = plan over the high-impact subset.

## 4. Scoring semantics (triage + priority)

### 4.1 Base impact score (`ScoreBreakdown`, weights sum to 1.0)

| component | weight | signal |
|---|---|---|
| pagerank | 0.22 | dependency importance |
| betweenness | 0.20 | bottleneck/bridging |
| blocker_ratio | 0.13 | direct blocking count (normalized) |
| staleness | 0.05 | age-based surfacing |
| priority_boost | 0.10 | explicit priority field |
| time_to_impact | 0.10 | critical-path depth + estimated minutes |
| urgency | 0.10 | urgent labels (`urgent, critical, blocker, hotfix, asap`) + 7d decay |
| risk | 0.10 | volatility/risk signals (bv-82) |

`Recommendation.breakdown` carries both weighted contributions and `*_norm` raw values plus
explanation strings (`time_to_impact_explanation`, `urgency_explanation`, `risk_explanation`,
`risk_signals`).

### 4.2 Triage score (bv-147, `DefaultTriageScoringOptions`)

`triage_score = base_score * 0.70 + unblock_boost + quick_win_boost` where
- `unblock_boost <= 0.15` = `min(unblocks/max(max_unblocks, 5), 1) * 0.15`
- `quick_win_boost <= 0.15` = `(1 - blocker_depth/3) * base_score * 0.15` for non-in_progress items
  with blocker depth <= 2, capped at 0.15
- Reserved-but-off factors (tracked as `factors_pending`): label_health, claim_penalty (planned
  x0.1 for claimed), attention_score.

Sort: score desc, tie-break by ID asc (deterministic). Recommendations filter to non-closed items;
top picks additionally require claimability (open/unassigned/unblocked/non-epic) in `robot-next`.
Staleness analysis only when the git-history prologue returned in budget (meta.history_status).

## 5. Input requirements matrix (from robot-capabilities)

| command | issues (JSONL) | git | sprints | baseline | mutates |
|---|---|---|---|---|---|
| robot-triage / robot-next / robot-triage-by-* | yes | optional (staleness) | no | no | no |
| robot-plan / robot-priority / robot-insights | yes | no | no | no | no |
| robot-label-health / -flow / -attention | yes | no | no | no | no |
| robot-history / robot-diff | yes | **yes** | no | no | no |
| robot-burndown / robot-sprint-list / robot-sprint-show | yes | scope-changes optional | **yes** | no | no |
| robot-forecast | yes | no | optional | no | no |
| robot-alerts | yes | no | no | optional | no |
| robot-suggest | yes | no | no | no | no |
| robot-graph | yes | no | no | no | no |
| robot-recipes / robot-schema / robot-docs / robot-capabilities / robot-help | no | no | no | no | no |

Events and comments from the br store are never read; everything above the JSONL line is computed
in-process from issues+dependencies.

## 6. What flywheel actually shells out to

Verified in `/Users/bangedorrunt/workspace/flywheel/src/work.rs` + `main.rs` (`cmd_work`,
`Bv::robot(flag)` → `bv --robot-<flag>` with no extra flags, stdout parsed as JSON):

- `bv --robot-triage` (flywheel `work triage`)
- `bv --robot-next` (flywheel `work next`)
- `bv --robot-insights` (flywheel `work insights`)
- `bv --robot-plan` (flywheel `work plan`)
- `br ready --json` (flywheel `work ready` — br itself, not bv)

Plus prompt-level references only: the `triage-deep` workflow and `bv-triage` stage instruct an
agent to "run `bv --robot-triage` (and `br ready --json`)" in stage text (stages.rs:199), and
robot-next's own usage hints point agents at `scripts/br_retry.sh actionable --json`.

## 7. Port-to-br notes

- Single-command coverage: `--robot-triage` is the designated default robot command
  (capabilities manifest: `default_robot_command: "bv --robot-triage"`). It embeds quick_ref,
  recommendations, quick wins, blockers, health, alerts, and commands. What it does NOT cover:
  per-metric maps (insights), plan tracks (plan), what-if/priority deltas (priority),
  git-correlated history/diff, label analytics.
- The claimability contract of `robot-next` (fail-closed claim_command) is the part agents like
  flywheel consume for auto-claiming; port it with the same degraded-block semantics.
- `status` + `data_hash` are the trust signals agents check (metric completeness, data changes);
  keep both, keep the PascalCase status keys or normalize (no consumer depends on the case today:
  flywheel only pretty-prints the JSON).
- Exit-code discipline: stdout JSON only, stderr diagnostics, 0/1/2.
- The `--brief` variant exists purely as token-cost control (#183); worth porting for swarm loops.
