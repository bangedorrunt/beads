//! Plain-Rust `DiGraph` (vendored from bv-graph-wasm with the wasm layer removed).
// governed-by: ADR-0003
use std::collections::HashMap;

pub struct DiGraph {
    nodes: Vec<String>,
    node_index: HashMap<String, usize>,
    adj: Vec<Vec<usize>>,
    rev_adj: Vec<Vec<usize>>,
    edge_count: usize,
}

impl DiGraph {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            node_index: HashMap::new(),
            adj: Vec::new(),
            rev_adj: Vec::new(),
            edge_count: 0,
        }
    }
    pub fn add_node(&mut self, id: &str) -> usize {
        if let Some(&i) = self.node_index.get(id) {
            return i;
        }
        let i = self.nodes.len();
        self.nodes.push(id.to_string());
        self.node_index.insert(id.to_string(), i);
        self.adj.push(Vec::new());
        self.rev_adj.push(Vec::new());
        i
    }
    pub fn add_edge(&mut self, from: usize, to: usize) {
        if from >= self.nodes.len() || to >= self.nodes.len() {
            return;
        }
        if self.adj[from].contains(&to) {
            return;
        }
        self.adj[from].push(to);
        self.rev_adj[to].push(from);
        self.edge_count += 1;
    }
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }
    pub fn edge_count(&self) -> usize {
        self.edge_count
    }
    pub fn successors_slice(&self, n: usize) -> &[usize] {
        self.adj.get(n).map_or(&[], |v| v.as_slice())
    }
    pub fn predecessors_slice(&self, n: usize) -> &[usize] {
        self.rev_adj.get(n).map_or(&[], |v| v.as_slice())
    }
    pub(crate) fn len(&self) -> usize {
        self.nodes.len()
    }
    pub fn with_capacity(node_capacity: usize, _edge_capacity: usize) -> Self {
        Self {
            nodes: Vec::with_capacity(node_capacity),
            node_index: HashMap::with_capacity(node_capacity),
            adj: Vec::with_capacity(node_capacity),
            rev_adj: Vec::with_capacity(node_capacity),
            edge_count: 0,
        }
    }
    pub fn node_id(&self, idx: usize) -> Option<String> {
        self.nodes.get(idx).cloned()
    }
    pub fn node_idx(&self, id: &str) -> Option<usize> {
        self.node_index.get(id).copied()
    }
    pub fn out_degree(&self, n: usize) -> usize {
        self.adj.get(n).map_or(0, |v| v.len())
    }
    pub fn in_degree(&self, n: usize) -> usize {
        self.rev_adj.get(n).map_or(0, |v| v.len())
    }
}
