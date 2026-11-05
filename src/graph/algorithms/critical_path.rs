//! Critical Path Heights computation.
//!
//! Computes the longest dependency chain from roots to each node.
//! Nodes with high heights are deep in the dependency tree.

use crate::graph::DiGraph;
use crate::graph::algorithms::topo::topological_sort;

/// Compute critical path heights (depth in DAG).
///
/// Height[v] = 1 + max(height of predecessors)
/// Roots (no predecessors) have height 1.
///
/// # Arguments
/// * `graph` - The directed graph
///
/// # Returns
/// Vector of heights, indexed by node. Returns zeros for cyclic graphs.
pub fn critical_path_heights(graph: &DiGraph) -> Vec<f64> {
    let n = graph.len();
    if n == 0 {
        return Vec::new();
    }

    let order = match topological_sort(graph) {
        Some(o) => o,
        None => return vec![0.0; n],
    };

    let mut heights = vec![0.0; n];

    for &v in &order {
        let max_pred_height = graph
            .predecessors_slice(v)
            .iter()
            .map(|&u| heights[u])
            .fold(0.0, f64::max);

        heights[v] = 1.0 + max_pred_height;
    }

    heights
}

/// Get nodes on the critical path (those with maximum height).
pub fn critical_path_nodes(graph: &DiGraph) -> Vec<usize> {
    let heights = critical_path_heights(graph);
    if heights.is_empty() {
        return Vec::new();
    }

    let max_height = heights.iter().cloned().fold(0.0, f64::max);

    heights
        .iter()
        .enumerate()
        .filter(|&(_, &h)| (h - max_height).abs() < 0.001)
        .map(|(i, _)| i)
        .collect()
}

/// Get the maximum height (critical path length).
pub fn critical_path_length(graph: &DiGraph) -> f64 {
    critical_path_heights(graph).into_iter().fold(0.0, f64::max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_graph() {
        let g = DiGraph::new();
        let heights = critical_path_heights(&g);
        assert!(heights.is_empty());
    }

    #[test]
    fn test_single_node() {
        let mut g = DiGraph::new();
        g.add_node("a");
        let heights = critical_path_heights(&g);
        assert_eq!(heights, vec![1.0]);
    }

    #[test]
    fn test_linear_chain() {
        let mut g = DiGraph::new();
        let a = g.add_node("a");
        let b = g.add_node("b");
        let c = g.add_node("c");
        g.add_edge(a, b);
        g.add_edge(b, c);

        let heights = critical_path_heights(&g);
        assert!((heights[a] - 1.0).abs() < f64::EPSILON);
        assert!((heights[b] - 2.0).abs() < f64::EPSILON);
        assert!((heights[c] - 3.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_diamond() {
        let mut g = DiGraph::new();
        let a = g.add_node("a");
        let b = g.add_node("b");
        let c = g.add_node("c");
        let d = g.add_node("d");
        g.add_edge(a, b);
        g.add_edge(a, c);
        g.add_edge(b, d);
        g.add_edge(c, d);

        let heights = critical_path_heights(&g);
        assert!((heights[a] - 1.0).abs() < f64::EPSILON);
        assert!((heights[b] - 2.0).abs() < f64::EPSILON);
        assert!((heights[c] - 2.0).abs() < f64::EPSILON);
        assert!((heights[d] - 3.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_parallel_chains() {
        let mut g = DiGraph::new();
        let a = g.add_node("a");
        let b = g.add_node("b");
        let c = g.add_node("c");
        let d = g.add_node("d");
        let e = g.add_node("e");
        g.add_edge(a, b);
        g.add_edge(b, c);
        g.add_edge(d, e);

        let heights = critical_path_heights(&g);
        assert!((heights[a] - 1.0).abs() < f64::EPSILON);
        assert!((heights[b] - 2.0).abs() < f64::EPSILON);
        assert!((heights[c] - 3.0).abs() < f64::EPSILON);
        assert!((heights[d] - 1.0).abs() < f64::EPSILON);
        assert!((heights[e] - 2.0).abs() < f64::EPSILON);

        assert!((critical_path_length(&g) - 3.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_cyclic_graph() {
        let mut g = DiGraph::new();
        let a = g.add_node("a");
        let b = g.add_node("b");
        let c = g.add_node("c");
        g.add_edge(a, b);
        g.add_edge(b, c);
        g.add_edge(c, a);

        let heights = critical_path_heights(&g);
        assert_eq!(heights, vec![0.0, 0.0, 0.0]);
    }

    #[test]
    fn test_critical_path_nodes() {
        let mut g = DiGraph::new();
        let a = g.add_node("a");
        let b = g.add_node("b");
        let c = g.add_node("c");
        g.add_edge(a, b);
        g.add_edge(b, c);

        let critical = critical_path_nodes(&g);
        assert_eq!(critical, vec![c]);
    }

    #[test]
    fn test_multiple_critical_path_nodes() {
        let mut g = DiGraph::new();
        let a = g.add_node("a");
        let b = g.add_node("b");
        let c = g.add_node("c");
        let d = g.add_node("d");
        g.add_edge(a, b);
        g.add_edge(c, d);

        let critical = critical_path_nodes(&g);
        assert_eq!(critical.len(), 2);
        assert!(critical.contains(&b));
        assert!(critical.contains(&d));
    }

    #[test]
    fn test_wide_graph() {
        let mut g = DiGraph::new();
        let a = g.add_node("a");
        let b = g.add_node("b");
        let c = g.add_node("c");
        let d = g.add_node("d");
        let e = g.add_node("e");
        g.add_edge(a, b);
        g.add_edge(a, c);
        g.add_edge(a, d);
        g.add_edge(a, e);

        let heights = critical_path_heights(&g);
        assert!((heights[a] - 1.0).abs() < f64::EPSILON);
        assert!((heights[b] - 2.0).abs() < f64::EPSILON);
        assert!((heights[c] - 2.0).abs() < f64::EPSILON);
        assert!((heights[d] - 2.0).abs() < f64::EPSILON);
        assert!((heights[e] - 2.0).abs() < f64::EPSILON);
    }
}
