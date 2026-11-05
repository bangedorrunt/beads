//! TopK Set algorithm: Greedy submodular selection for maximum unlock.
//!
//! Finds the k issues that, when completed, maximize the total downstream
//! unlocks. Uses greedy submodular optimization where each iteration picks
//! the node with maximum marginal gain.

use crate::graph::DiGraph;
use crate::graph::whatif::what_if_close;
use serde::Serialize;

/// An item in the TopK Set result.
#[derive(Debug, Clone, Serialize)]
pub struct TopKSetItem {
    /// Node index in the graph
    pub node: usize,
    /// Marginal gain from selecting this node (transitive unblocks)
    pub marginal_gain: usize,
    /// IDs of nodes that become actionable after this selection
    pub unblocked_ids: Vec<usize>,
}

/// Result of the TopK Set algorithm.
#[derive(Debug, Clone, Serialize)]
pub struct TopKSetResult {
    /// Selected items in order of selection
    pub items: Vec<TopKSetItem>,
    /// Total gain across all selections
    pub total_gain: usize,
    /// Number of open (non-closed) nodes considered
    pub open_nodes: usize,
}

/// Greedy submodular selection for maximum unlock.
///
/// Iteratively selects nodes that maximize transitive unblocks.
/// Each iteration simulates closing the best node and updates
/// the closed set for the next iteration.
///
/// # Arguments
/// * `graph` - The directed dependency graph
/// * `closed_set` - Boolean array where true means the node is closed/completed
/// * `k` - Maximum number of items to select
///
/// # Returns
/// TopKSetResult with selected items sorted by selection order.
///
/// # Complexity
/// O(n * k * n) = O(n²k) where n is node count and k is selection limit.
/// Each iteration evaluates all open nodes using what_if_close.
pub fn topk_set(graph: &DiGraph, closed_set: &[bool], k: usize) -> TopKSetResult {
    let n = graph.len();
    if n == 0 || k == 0 {
        return TopKSetResult {
            items: Vec::new(),
            total_gain: 0,
            open_nodes: 0,
        };
    }

    let mut current_closed = closed_set.to_vec();
    current_closed.resize(n, false);

    let open_nodes = (0..n).filter(|&i| !current_closed[i]).count();

    let mut selected = Vec::new();
    let mut total_gain = 0;

    for _ in 0..k {
        let mut best_node: Option<usize> = None;
        let mut best_gain = 0;
        let mut best_unblocked: Vec<usize> = Vec::new();

        let mut candidates: Vec<usize> = (0..n).filter(|&i| !current_closed[i]).collect();
        candidates.sort_unstable();

        for node in candidates {
            let result = what_if_close(graph, node, &current_closed);
            let gain = result.transitive_unblocks;

            if gain > best_gain || (gain == best_gain && best_node.is_none()) {
                best_gain = gain;
                best_node = Some(node);
                best_unblocked = result.cascade_ids;
            }
        }

        match best_node {
            Some(node) if best_gain > 0 => {
                selected.push(TopKSetItem {
                    node,
                    marginal_gain: best_gain,
                    unblocked_ids: best_unblocked.clone(),
                });
                current_closed[node] = true;
                for &cascade_node in &best_unblocked {
                    if cascade_node < n {
                        current_closed[cascade_node] = true;
                    }
                }
                total_gain += best_gain;
            }
            _ => break,
        }
    }

    TopKSetResult {
        items: selected,
        total_gain,
        open_nodes,
    }
}

/// Greedy TopK Set with default limit of 5.
pub fn topk_set_default(graph: &DiGraph, closed_set: &[bool]) -> TopKSetResult {
    topk_set(graph, closed_set, 5)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_graph(edges: &[(usize, usize)]) -> DiGraph {
        let mut g = DiGraph::new();
        let max_node = edges.iter().flat_map(|(a, b)| [*a, *b]).max().unwrap_or(0);
        for i in 0..=max_node {
            g.add_node(&format!("n{}", i));
        }
        for (from, to) in edges {
            g.add_edge(*from, *to);
        }
        g
    }

    #[test]
    fn test_empty_graph() {
        let g = DiGraph::new();
        let result = topk_set(&g, &[], 5);
        assert!(result.items.is_empty());
        assert_eq!(result.total_gain, 0);
    }

    #[test]
    fn test_single_node() {
        let mut g = DiGraph::new();
        g.add_node("a");
        let result = topk_set(&g, &[false], 5);
        assert!(result.items.is_empty());
        assert_eq!(result.total_gain, 0);
    }

    #[test]
    fn test_simple_chain() {
        let g = make_graph(&[(0, 1), (1, 2), (2, 3)]);
        let result = topk_set(&g, &[false; 4], 5);

        assert!(!result.items.is_empty());
        assert_eq!(result.items[0].node, 0);
        assert_eq!(result.items[0].marginal_gain, 3);
    }

    #[test]
    fn test_fork_pattern() {
        let g = make_graph(&[(0, 1), (0, 2), (0, 3)]);
        let result = topk_set(&g, &[false; 4], 5);

        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].node, 0);
        assert_eq!(result.items[0].marginal_gain, 3);
        assert_eq!(result.total_gain, 3);
    }

    #[test]
    fn test_multiple_hubs() {
        let g = make_graph(&[(0, 1), (0, 2), (3, 4), (3, 5), (3, 6)]);
        let result = topk_set(&g, &[false; 7], 5);

        assert!(result.items.len() >= 2);
        assert_eq!(result.items[0].node, 3);
        assert_eq!(result.items[0].marginal_gain, 3);
        assert_eq!(result.items[1].node, 0);
        assert_eq!(result.items[1].marginal_gain, 2);
        assert_eq!(result.total_gain, 5);
    }

    #[test]
    fn test_submodularity() {
        let g = make_graph(&[(0, 1), (1, 2), (2, 3), (3, 4)]);
        let result = topk_set(&g, &[false; 5], 5);

        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].marginal_gain, 4);
    }

    #[test]
    fn test_partially_closed() {
        let g = make_graph(&[(0, 2), (1, 2), (2, 3)]);
        let closed = vec![true, false, false, false];
        let result = topk_set(&g, &closed, 5);

        assert!(!result.items.is_empty());
        assert_eq!(result.items[0].node, 1);
        assert_eq!(result.items[0].marginal_gain, 2);
    }

    #[test]
    fn test_limit_respected() {
        let g = make_graph(&[(0, 1), (2, 3), (4, 5), (6, 7), (8, 9)]);
        let result = topk_set(&g, &[false; 10], 2);

        assert_eq!(result.items.len(), 2);
    }

    #[test]
    fn test_deterministic() {
        let g = make_graph(&[(0, 1), (2, 3)]);

        for _ in 0..5 {
            let result = topk_set(&g, &[false; 4], 5);
            assert_eq!(result.items[0].node, 0);
        }
    }

    #[test]
    fn test_monotonic_marginal_gains() {
        let g = make_graph(&[(0, 1), (0, 2), (0, 3), (1, 4), (1, 5)]);
        let result = topk_set(&g, &[false; 6], 5);

        if result.items.len() >= 2 {
            for i in 1..result.items.len() {
                assert!(
                    result.items[i].marginal_gain <= result.items[i - 1].marginal_gain,
                    "Marginal gains should be monotonically non-increasing"
                );
            }
        }
    }

    #[test]
    fn test_open_nodes_count() {
        let g = make_graph(&[(0, 1), (1, 2)]);
        let closed = vec![true, false, false];
        let result = topk_set(&g, &closed, 5);

        assert_eq!(result.open_nodes, 2);
    }

    #[test]
    fn test_all_closed() {
        let g = make_graph(&[(0, 1), (1, 2)]);
        let closed = vec![true, true, true];
        let result = topk_set(&g, &closed, 5);

        assert!(result.items.is_empty());
        assert_eq!(result.open_nodes, 0);
    }

    #[test]
    fn test_deep_cascade() {
        let mut edges = Vec::new();
        for i in 0..9 {
            edges.push((i, i + 1));
        }
        let g = make_graph(&edges);
        let result = topk_set(&g, &[false; 10], 5);

        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].node, 0);
        assert_eq!(result.items[0].marginal_gain, 9);
    }
}
