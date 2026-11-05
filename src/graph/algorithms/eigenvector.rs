//! Eigenvector Centrality algorithm implementation.
//!
//! Measures node importance based on connections to other important nodes.
//! Uses the principal eigenvector of the adjacency matrix via power iteration.

use crate::graph::DiGraph;

/// Eigenvector centrality configuration.
pub struct EigenvectorConfig {
    /// Number of power iterations
    pub iterations: u32,
    /// Convergence tolerance (stop early if converged)
    pub tolerance: f64,
}

impl Default for EigenvectorConfig {
    fn default() -> Self {
        EigenvectorConfig {
            iterations: 50,
            tolerance: 1e-6,
        }
    }
}

/// Compute eigenvector centrality using power iteration.
///
/// Uses incoming edges: nodes pointed TO by important nodes are important.
/// This measures prestige - who receives attention from important sources.
///
/// Returns vector of scores in node index order, normalized to unit length.
pub fn eigenvector(graph: &DiGraph, config: &EigenvectorConfig) -> Vec<f64> {
    let n = graph.len();
    if n == 0 {
        return Vec::new();
    }

    let init_val = 1.0 / (n as f64).sqrt();
    let mut vec = vec![init_val; n];
    let mut work = vec![0.0; n];

    for _ in 0..config.iterations {
        work.fill(0.0);

        for (v, score) in work.iter_mut().enumerate() {
            for &u in graph.predecessors_slice(v) {
                *score += vec[u];
            }
        }

        let norm: f64 = work.iter().map(|x| x * x).sum::<f64>().sqrt();
        if norm < 1e-10 {
            let uniform = 1.0 / (n as f64).sqrt();
            return vec![uniform; n];
        }

        for w in &mut work {
            *w /= norm;
        }

        let diff: f64 = vec
            .iter()
            .zip(work.iter())
            .map(|(a, b)| (a - b).abs())
            .sum();

        std::mem::swap(&mut vec, &mut work);

        if diff < config.tolerance {
            break;
        }
    }

    vec
}

/// Compute eigenvector centrality with default parameters (50 iterations).
pub fn eigenvector_default(graph: &DiGraph) -> Vec<f64> {
    eigenvector(graph, &EigenvectorConfig::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_eigenvector_empty() {
        let graph = DiGraph::new();
        let scores = eigenvector(&graph, &EigenvectorConfig::default());
        assert!(scores.is_empty());
    }

    #[test]
    fn test_eigenvector_single_node() {
        let mut graph = DiGraph::new();
        graph.add_node("a");
        let scores = eigenvector(&graph, &EigenvectorConfig::default());
        assert_eq!(scores.len(), 1);
        assert!((scores[0] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_eigenvector_chain() {
        let mut graph = DiGraph::new();
        let a = graph.add_node("a");
        let b = graph.add_node("b");
        let c = graph.add_node("c");
        graph.add_edge(a, b);
        graph.add_edge(b, c);

        let scores = eigenvector(&graph, &EigenvectorConfig::default());
        let diff = (scores[a] - scores[b]).abs() + (scores[b] - scores[c]).abs();
        assert!(
            diff < 0.01,
            "DAG chain should converge to near-uniform: {:?}",
            scores
        );
    }

    #[test]
    fn test_eigenvector_cycle() {
        let mut graph = DiGraph::new();
        let a = graph.add_node("a");
        let b = graph.add_node("b");
        let c = graph.add_node("c");
        graph.add_edge(a, b);
        graph.add_edge(b, c);
        graph.add_edge(c, a);

        let scores = eigenvector(&graph, &EigenvectorConfig::default());
        let diff = (scores[a] - scores[b]).abs() + (scores[b] - scores[c]).abs();
        assert!(
            diff < 0.001,
            "Scores in cycle should be equal: {:?}",
            scores
        );
    }

    #[test]
    fn test_eigenvector_star() {
        let mut graph = DiGraph::new();
        let a = graph.add_node("a");
        let b = graph.add_node("b");
        let c = graph.add_node("c");
        let d = graph.add_node("d");
        graph.add_edge(a, b);
        graph.add_edge(a, c);
        graph.add_edge(a, d);

        let scores = eigenvector(&graph, &EigenvectorConfig::default());
        let sink_diff = (scores[b] - scores[c]).abs() + (scores[c] - scores[d]).abs();
        assert!(sink_diff < 0.001, "Sink nodes should have equal scores");
    }

    #[test]
    fn test_eigenvector_hub_with_cycle() {
        let mut graph = DiGraph::new();
        let a = graph.add_node("a");
        let b = graph.add_node("b");
        let c = graph.add_node("c");
        let d = graph.add_node("d");
        graph.add_edge(b, a);
        graph.add_edge(c, a);
        graph.add_edge(d, a);
        graph.add_edge(a, b);

        let scores = eigenvector(&graph, &EigenvectorConfig::default());
        assert!(
            scores[a] > scores[c],
            "Hub with cycle should have higher score than isolated node: {} vs {}",
            scores[a],
            scores[c]
        );
    }

    #[test]
    fn test_eigenvector_diamond() {
        let mut graph = DiGraph::new();
        let a = graph.add_node("a");
        let b = graph.add_node("b");
        let c = graph.add_node("c");
        let d = graph.add_node("d");
        graph.add_edge(a, b);
        graph.add_edge(a, c);
        graph.add_edge(b, d);
        graph.add_edge(c, d);

        let scores = eigenvector(&graph, &EigenvectorConfig::default());
        assert!(
            (scores[b] - scores[c]).abs() < 0.001,
            "b and c should have equal scores due to symmetry"
        );
        assert_eq!(scores.len(), 4);
    }

    #[test]
    fn test_eigenvector_unit_length() {
        let mut graph = DiGraph::new();
        let a = graph.add_node("a");
        let b = graph.add_node("b");
        let c = graph.add_node("c");
        graph.add_edge(a, b);
        graph.add_edge(b, c);
        graph.add_edge(c, a);

        let scores = eigenvector(&graph, &EigenvectorConfig::default());
        let length: f64 = scores.iter().map(|x| x * x).sum::<f64>().sqrt();
        assert!(
            (length - 1.0).abs() < 0.001,
            "Eigenvector should have unit length, got {}",
            length
        );
    }

    #[test]
    fn test_eigenvector_disconnected() {
        let mut graph = DiGraph::new();
        let a = graph.add_node("a");
        let b = graph.add_node("b");
        let c = graph.add_node("c");
        let d = graph.add_node("d");
        graph.add_edge(a, b);
        graph.add_edge(c, d);

        let scores = eigenvector(&graph, &EigenvectorConfig::default());
        assert_eq!(scores.len(), 4);
        for s in &scores {
            assert!(*s >= 0.0, "All scores should be non-negative");
        }
    }

    #[test]
    fn test_eigenvector_convergence() {
        let mut graph = DiGraph::new();
        for i in 0..20 {
            graph.add_node(&format!("node{}", i));
        }
        for i in 0..19 {
            graph.add_edge(i, i + 1);
        }
        graph.add_edge(0, 10);
        graph.add_edge(5, 15);
        graph.add_edge(10, 0);

        let scores = eigenvector(&graph, &EigenvectorConfig::default());
        assert_eq!(scores.len(), 20);

        let length: f64 = scores.iter().map(|x| x * x).sum::<f64>().sqrt();
        assert!(
            (length - 1.0).abs() < 0.001,
            "Should converge to unit length"
        );
    }
}
