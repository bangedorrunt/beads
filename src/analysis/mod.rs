//! Dependency-graph analysis engine (ADR-0003 §3.2).
// governed-by: ADR-0003
//!
//! Port of beads_viewer's analyzer, computed from this fork's SQLite-backed
//! `Issue` set instead of re-reading JSONL. Two graph orientations matter
//! (docs/research/bv-analysis-map.md "edge-direction landmine"):
//!
//! * canonical graph — edge `blocker -> blocked` (`depends_on_id ->
//!   issue_id`). The closed-set family (`reachability::is_actionable`,
//!   `what_if_close`, `topk_set`, `parallel_cut`) wants predecessors to be
//!   blockers, so the canonical graph serves them directly.
//! * bv-parity view — the reverse (`dependent -> dependency`), which is how
//!   beads_viewer's Go analyzer builds its graph. Centrality (PageRank,
//!   betweenness, HITS, eigenvector) and critical-path heights run on this
//!   orientation so scores mean what they mean in bv: importance comes from
//!   dependents.
//!
//! Orientation-agnostic metrics (SCC cycles, k-core, articulation —
//! undirected) run on the canonical graph.

pub mod config;
pub mod data_hash;
pub mod engine;
pub mod status;

pub use config::{AnalysisConfig, BetweennessMode, METRIC_DEFAULT_TIMEOUT_MS};
pub use engine::{AnalysisEngine, AnalysisResult};
pub use status::{MetricState, MetricStatus};
