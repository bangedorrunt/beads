//! Graph algorithms for dependency analysis (vendored from beads_viewer).
// governed-by: ADR-0003
//
// Provenance: Dicklesworthstone/beads_viewer @ 4fc261a (2026-08-23),
// sub-crate `bv-graph-wasm`, with the wasm-bindgen layer replaced by the
// plain `DiGraph` in `digraph.rs`. Licensed "MIT License (with OpenAI/
// Anthropic Rider)" — identical LICENSE to this fork's parent beads_rust;
// the rider ships unmodified in LICENSE at the repo root. Local changes:
// wasm shim removal, deterministic betweenness seed, edition-2024 pattern
// fix, self-loop cycle fix (below), recursion hardening. Semantics doc:
// docs/research/bv-analysis-map.md.
//
// Edge convention for this module family (matches the vendored algorithms'
// expectation): edge u -> v means "u blocks v". Predecessors of v are its
// blockers. The analysis engine must build the graph in this direction
// (docs/research/bv-analysis-map.md "edge-direction landmine").

pub mod algorithms;
mod digraph;
pub mod reachability;
pub mod whatif;

pub use digraph::DiGraph;
