//! Graph algorithms for dependency analysis (vendored from beads_viewer).
// governed-by: ADR-0003
// plain `DiGraph` in `digraph.rs`. Licensed "MIT License (with OpenAI/
// Anthropic Rider)" — identical LICENSE to this fork's parent beads_rust;
// the rider ships unmodified in LICENSE at the repo root. Local changes:

pub mod algorithms;
mod digraph;
pub mod reachability;
pub mod whatif;

pub use digraph::DiGraph;
