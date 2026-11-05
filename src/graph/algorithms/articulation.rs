//! Articulation points (cut vertices) algorithm.
//!
//! Finds nodes whose removal disconnects the graph.
//! Uses Tarjan's algorithm on the undirected view.
//!
//! In issue tracking, articulation points are critical coordination
//! points - if those issues are blocked or deprioritized, they can
//! disconnect groups of related work.

use crate::graph::DiGraph;
use std::collections::HashSet;

/// Find articulation points (cut vertices) using Tarjan's algorithm.
///
/// An articulation point is a vertex whose removal increases the number
/// of connected components in the graph.
///
/// # Algorithm
/// Uses DFS to compute discovery times and low-link values:
/// - disc[v]: discovery time of vertex v
/// - low[v]: minimum discovery time reachable from subtree of v
///
/// A vertex v is an articulation point if:
/// 1. v is root of DFS tree and has >1 children, OR
/// 2. v is not root and has child u with low[u] >= disc[v]
///
/// # Returns
/// Vector of node indices that are articulation points.
pub fn articulation_points(graph: &DiGraph) -> Vec<usize> {
    let n = graph.len();
    if n == 0 {
        return Vec::new();
    }

    let neighbors = build_undirected_neighbors(graph);

    let mut dfs = ArticulationDfs {
        neighbors: &neighbors,
        disc: vec![0usize; n],
        low: vec![0usize; n],
        parent: vec![usize::MAX; n],
        visited: vec![false; n],
        is_ap: vec![false; n],
        time: 0usize,
    };

    for start in 0..n {
        if !dfs.visited[start] {
            dfs.visit(start);
        }
    }

    dfs.is_ap
        .iter()
        .enumerate()
        .filter_map(|(i, &ap)| if ap { Some(i) } else { None })
        .collect()
}

struct ArticulationDfs<'a> {
    neighbors: &'a [Vec<usize>],
    disc: Vec<usize>,
    low: Vec<usize>,
    parent: Vec<usize>,
    visited: Vec<bool>,
    is_ap: Vec<bool>,
    time: usize,
}

impl ArticulationDfs<'_> {
    /// DFS for Tarjan's articulation point algorithm.
    fn visit(&mut self, v: usize) {
        self.visited[v] = true;
        self.time += 1;
        self.disc[v] = self.time;
        self.low[v] = self.time;
        let mut children = 0;

        for &u in &self.neighbors[v] {
            if !self.visited[u] {
                children += 1;
                self.parent[u] = v;

                self.visit(u);

                self.low[v] = self.low[v].min(self.low[u]);

                if self.parent[v] == usize::MAX && children > 1 {
                    self.is_ap[v] = true;
                }

                if self.parent[v] != usize::MAX && self.low[u] >= self.disc[v] {
                    self.is_ap[v] = true;
                }
            } else if u != self.parent[v] {
                self.low[v] = self.low[v].min(self.disc[u]);
            }
        }
    }
}

/// Build undirected neighbor lists from directed graph.
fn build_undirected_neighbors(graph: &DiGraph) -> Vec<Vec<usize>> {
    let n = graph.len();
    let mut neighbors: Vec<HashSet<usize>> = vec![HashSet::new(); n];

    for u in 0..n {
        for &v in graph.successors_slice(u) {
            if u != v {
                neighbors[u].insert(v);
                neighbors[v].insert(u);
            }
        }
    }

    neighbors
        .into_iter()
        .map(|s| s.into_iter().collect())
        .collect()
}

/// Count bridges (cut edges) in the graph.
/// A bridge is an edge whose removal disconnects the graph.
pub fn bridges(graph: &DiGraph) -> Vec<(usize, usize)> {
    let n = graph.len();
    if n == 0 {
        return Vec::new();
    }

    let neighbors = build_undirected_neighbors(graph);

    let mut dfs = BridgeDfs {
        neighbors: &neighbors,
        disc: vec![0usize; n],
        low: vec![0usize; n],
        parent: vec![usize::MAX; n],
        visited: vec![false; n],
        bridges: Vec::new(),
        time: 0usize,
    };

    for start in 0..n {
        if !dfs.visited[start] {
            dfs.visit(start);
        }
    }

    dfs.bridges
}

struct BridgeDfs<'a> {
    neighbors: &'a [Vec<usize>],
    disc: Vec<usize>,
    low: Vec<usize>,
    parent: Vec<usize>,
    visited: Vec<bool>,
    bridges: Vec<(usize, usize)>,
    time: usize,
}

impl BridgeDfs<'_> {
    /// DFS for bridge detection.
    fn visit(&mut self, v: usize) {
        self.visited[v] = true;
        self.time += 1;
        self.disc[v] = self.time;
        self.low[v] = self.time;

        for &u in &self.neighbors[v] {
            if !self.visited[u] {
                self.parent[u] = v;
                self.visit(u);

                self.low[v] = self.low[v].min(self.low[u]);

                if self.low[u] > self.disc[v] {
                    self.bridges.push((v.min(u), v.max(u)));
                }
            } else if u != self.parent[v] {
                self.low[v] = self.low[v].min(self.disc[u]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_articulation_empty() {
        let graph = DiGraph::new();
        let ap = articulation_points(&graph);
        assert!(ap.is_empty());
    }

    #[test]
    fn test_articulation_single_node() {
        let mut graph = DiGraph::new();
        graph.add_node("a");
        let ap = articulation_points(&graph);
        assert!(ap.is_empty());
    }

    #[test]
    fn test_articulation_two_nodes() {
        let mut graph = DiGraph::new();
        let a = graph.add_node("a");
        let b = graph.add_node("b");
        graph.add_edge(a, b);

        let ap = articulation_points(&graph);
        assert!(ap.is_empty());
    }

    #[test]
    fn test_articulation_chain() {
        let mut graph = DiGraph::new();
        let a = graph.add_node("a");
        let b = graph.add_node("b");
        let c = graph.add_node("c");
        graph.add_edge(a, b);
        graph.add_edge(b, c);

        let ap = articulation_points(&graph);
        assert_eq!(ap.len(), 1);
        assert!(ap.contains(&b));
    }

    #[test]
    fn test_articulation_triangle() {
        let mut graph = DiGraph::new();
        let a = graph.add_node("a");
        let b = graph.add_node("b");
        let c = graph.add_node("c");
        graph.add_edge(a, b);
        graph.add_edge(b, c);
        graph.add_edge(c, a);

        let ap = articulation_points(&graph);
        assert!(ap.is_empty());
    }

    #[test]
    fn test_articulation_star() {
        let mut graph = DiGraph::new();
        let hub = graph.add_node("hub");
        let l1 = graph.add_node("l1");
        let l2 = graph.add_node("l2");
        let l3 = graph.add_node("l3");
        graph.add_edge(hub, l1);
        graph.add_edge(hub, l2);
        graph.add_edge(hub, l3);

        let ap = articulation_points(&graph);
        assert_eq!(ap.len(), 1);
        assert!(ap.contains(&hub));
    }

    #[test]
    fn test_articulation_bowtie() {
        let mut graph = DiGraph::new();
        let a = graph.add_node("a");
        let b = graph.add_node("b");
        let c = graph.add_node("c");
        let d = graph.add_node("d");
        let e = graph.add_node("e");
        let f = graph.add_node("f");

        graph.add_edge(a, b);
        graph.add_edge(b, c);
        graph.add_edge(c, a);

        graph.add_edge(c, e);

        graph.add_edge(d, e);
        graph.add_edge(e, f);
        graph.add_edge(f, d);

        let ap = articulation_points(&graph);
        assert!(ap.contains(&c) || ap.contains(&e));
    }

    #[test]
    fn test_articulation_bridge_chain() {
        let mut graph = DiGraph::new();
        let a = graph.add_node("a");
        let b = graph.add_node("b");
        let c = graph.add_node("c");
        let d = graph.add_node("d");
        graph.add_edge(a, b);
        graph.add_edge(b, c);
        graph.add_edge(c, d);

        let ap = articulation_points(&graph);
        assert_eq!(ap.len(), 2);
        assert!(ap.contains(&b));
        assert!(ap.contains(&c));
    }

    #[test]
    fn test_bridges_chain() {
        let mut graph = DiGraph::new();
        let a = graph.add_node("a");
        let b = graph.add_node("b");
        let c = graph.add_node("c");
        graph.add_edge(a, b);
        graph.add_edge(b, c);

        let br = bridges(&graph);
        assert_eq!(br.len(), 2);
    }

    #[test]
    fn test_bridges_triangle() {
        let mut graph = DiGraph::new();
        let a = graph.add_node("a");
        let b = graph.add_node("b");
        let c = graph.add_node("c");
        graph.add_edge(a, b);
        graph.add_edge(b, c);
        graph.add_edge(c, a);

        let br = bridges(&graph);
        assert!(br.is_empty());
    }

    #[test]
    fn test_articulation_disconnected() {
        let mut graph = DiGraph::new();
        let a = graph.add_node("a");
        let b = graph.add_node("b");
        let c = graph.add_node("c");
        let d = graph.add_node("d");
        let e = graph.add_node("e");
        let f = graph.add_node("f");

        graph.add_edge(a, b);
        graph.add_edge(b, c);
        graph.add_edge(c, a);

        graph.add_edge(d, e);
        graph.add_edge(e, f);
        graph.add_edge(f, d);

        let ap = articulation_points(&graph);
        assert!(ap.is_empty());
    }
}
