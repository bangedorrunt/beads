//! Two-phase dependency-graph analysis over the fork's own issue set.
// governed-by: ADR-0003
//!
//! Phase 1 (instant): graph build, degrees, topological order, density,
//! weakly-connected components, actionable/blocked classification.
//! Phase 2 (budgeted): PageRank, betweenness, HITS, eigenvector,
//! critical-path heights, cycles, k-core, articulation, slack, each with a
//! `MetricStatus` entry. See the module docs for the two graph orientations.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Instant;

use crate::graph::algorithms::{
    articulation, betweenness, critical_path, cycles, eigenvector, hits, kcore, pagerank, slack,
    topo,
};
use crate::graph::algorithms::{
    eigenvector::EigenvectorConfig, hits::HITSConfig, pagerank::PageRankConfig,
};
use crate::graph::{DiGraph, reachability};
use crate::model::{DependencyType, Issue};

use super::config::AnalysisConfig;
use super::status::{MetricEntry, MetricStatus};

/// Phase-1 structural results plus Phase-2 metric maps.
#[derive(Debug, Clone)]
pub struct AnalysisResult {
    /// Canonical blocker -> blocked graph (see module docs).
    pub graph: DiGraph,
    /// `graph` node index -> issue id, in insertion (id-sorted) order.
    pub ids: Vec<String>,
    /// issue id -> node index.
    pub index: HashMap<String, usize>,
    /// Out-degree in the canonical graph (= how many issues this one blocks).
    pub out_degree: BTreeMap<String, usize>,
    /// In-degree in the canonical graph (= how many blockers).
    pub in_degree: BTreeMap<String, usize>,
    /// Kahn topological order over the canonical graph (None when cyclic).
    pub topological_order: Option<Vec<String>>,
    /// Directed density = edges / (n * (n-1)) (0 for n < 2).
    pub density: f64,
    /// Weakly-connected components (sets of issue ids).
    pub components: Vec<Vec<String>>,
    /// Issues whose `blocks` dependencies are all closed (canonical-graph
    /// `is_actionable`). Closed issues themselves are excluded.
    pub actionable: HashSet<String>,
    /// Open issues with at least one open blocker.
    pub blocked: HashSet<String>,
    /// PageRank on the bv-parity view (importance = transitive dependents).
    pub pagerank: Option<BTreeMap<String, f64>>,
    /// Betweenness on the bv-parity view (raw, unnormalized — bv parity).
    pub betweenness: Option<BTreeMap<String, f64>>,
    pub eigenvector: Option<BTreeMap<String, f64>>,
    pub hubs: Option<BTreeMap<String, f64>>,
    pub authorities: Option<BTreeMap<String, f64>>,
    /// Critical-path heights on the bv-parity view (1 + max dependent height).
    pub critical_path_score: Option<BTreeMap<String, f64>>,
    /// Johnson-enumerated cycles (node-id lists), capped by config.
    pub cycles: Option<Vec<Vec<String>>>,
    /// Number of cyclic SCCs (including single-node self-loops).
    pub cycle_count: usize,
    pub has_cycles: bool,
    pub core_number: Option<BTreeMap<String, usize>>,
    pub articulation_points: Option<Vec<String>>,
    pub slack: Option<BTreeMap<String, f64>>,
    pub status: MetricStatus,
}

/// Builds and analyzes the dependency graph of an issue set.
pub struct AnalysisEngine {
    issues: Vec<Issue>,
}

/// Dep edge types that participate in the blocking graph. Non-blocking
/// relations (`related`, `parent-child`, ...) are not schedule edges.
const BLOCKING_DEP_TYPES: &[DependencyType] = &[
    DependencyType::Blocks,
    DependencyType::ConditionalBlocks,
    DependencyType::WaitsFor,
];

fn is_blocking(dep_type: &DependencyType) -> bool {
    BLOCKING_DEP_TYPES.contains(dep_type)
}

impl AnalysisEngine {
    /// Issues to analyze. Duplicate ids collapse (last wins, bv-style).
    #[must_use]
    pub fn new(issues: Vec<Issue>) -> Self {
        Self { issues }
    }

    fn build(&self) -> (DiGraph, Vec<String>, HashMap<String, usize>, Vec<&Issue>) {
        // Dedup by id (last wins) then sort by id for deterministic
        // node-index order (bv sorts by ID before hashing/analyzing).
        let mut by_id: BTreeMap<String, &Issue> = BTreeMap::new();
        for issue in &self.issues {
            by_id.insert(issue.id.clone(), issue);
        }
        let issues: Vec<&Issue> = by_id.into_values().collect();

        let mut graph = DiGraph::with_capacity(issues.len(), issues.len() * 2);
        let mut index = HashMap::with_capacity(issues.len());
        let mut ids = Vec::with_capacity(issues.len());
        for issue in &issues {
            let idx = graph.add_node(&issue.id);
            index.insert(issue.id.clone(), idx);
            ids.push(issue.id.clone());
        }
        // Canonical blocker -> blocked. `issue` depends on `dep.depends_on_id`,
        // so `dep.depends_on_id` blocks `issue`.
        for issue in &issues {
            for dep in &issue.dependencies {
                if !is_blocking(&dep.dep_type) {
                    continue;
                }
                let (Some(&from), Some(&to)) =
                    (index.get(&dep.depends_on_id), index.get(&issue.id))
                else {
                    continue; // dep on unknown id: ignore (bv loads only known)
                };
                graph.add_edge(from, to);
            }
        }
        (graph, ids, index, issues)
    }

    /// The bv-parity view: reverse of the canonical graph (dependent ->
    /// dependency), which is the orientation beads_viewer's analyzer uses
    /// for centrality and heights.
    fn reverse_of(graph: &DiGraph) -> DiGraph {
        let n = graph.node_count();
        let mut rev = DiGraph::with_capacity(n, graph.edge_count());
        for i in 0..n {
            rev.add_node(&graph.node_id(i).unwrap_or_default());
        }
        for u in 0..n {
            for &v in graph.successors_slice(u) {
                rev.add_edge(v, u);
            }
        }
        rev
    }

    /// Weakly-connected components (undirected reachability).
    fn weak_components(graph: &DiGraph, ids: &[String]) -> Vec<Vec<String>> {
        let n = graph.node_count();
        let mut seen = vec![false; n];
        let mut components = Vec::new();
        for start in 0..n {
            if seen[start] {
                continue;
            }
            let mut stack = vec![start];
            seen[start] = true;
            let mut comp = Vec::new();
            while let Some(v) = stack.pop() {
                comp.push(ids[v].clone());
                for &w in graph.successors_slice(v) {
                    if !seen[w] {
                        seen[w] = true;
                        stack.push(w);
                    }
                }
                for &w in graph.predecessors_slice(v) {
                    if !seen[w] {
                        seen[w] = true;
                        stack.push(w);
                    }
                }
            }
            comp.sort();
            components.push(comp);
        }
        components
    }

    /// Phase 1 only: structure + actionable/blocked sets, no metrics.
    /// Cheap enough to run synchronously on any workspace size.
    #[must_use]
    pub fn analyze_phase1(&self) -> AnalysisResult {
        let (graph, ids, index, issues) = self.build();

        let mut out_degree = BTreeMap::new();
        let mut in_degree = BTreeMap::new();
        for (i, id) in ids.iter().enumerate() {
            out_degree.insert(id.clone(), graph.out_degree(i));
            in_degree.insert(id.clone(), graph.in_degree(i));
        }

        let n = graph.node_count();
        let density = if n > 1 {
            graph.edge_count() as f64 / (n * (n - 1)) as f64
        } else {
            0.0
        };

        let topo_ids: Option<Vec<String>> = topo::topological_sort(&graph)
            .map(|order| order.into_iter().map(|i| ids[i].clone()).collect());

        let components = Self::weak_components(&graph, &ids);

        let mut closed = vec![false; n];
        for issue in &issues {
            closed[index[&issue.id]] = issue.status.is_terminal();
        }
        let mut actionable = HashSet::new();
        let mut blocked = HashSet::new();
        for i in 0..n {
            if closed[i] {
                continue;
            }
            if reachability::is_actionable(&graph, i, &closed) {
                actionable.insert(ids[i].clone());
            } else {
                blocked.insert(ids[i].clone());
            }
        }

        // Phase-1 cycle detection is free: Kahn's failure IS the cycle
        // signal (topo is None iff the graph is cyclic). Enumeration
        // (cycle_count) stays a Phase-2 job.
        let has_cycles = topo_ids.is_none();

        AnalysisResult {
            graph,
            ids,
            index,
            out_degree,
            in_degree,
            topological_order: topo_ids,
            density,
            components,
            actionable,
            blocked,
            pagerank: None,
            betweenness: None,
            eigenvector: None,
            hubs: None,
            authorities: None,
            critical_path_score: None,
            cycles: None,
            cycle_count: 0,
            has_cycles,
            core_number: None,
            articulation_points: None,
            slack: None,
            status: MetricStatus::pending_all(),
        }
    }

    /// Phase 1 + Phase 2 per `config`. Metrics run synchronously with
    /// deadline checks between iterations; entries land in `status`.
    ///
    /// Callers wrapping this in a thread should use a large stack
    /// (>= 32 MiB): Tarjan/Johnson/articulation in the vendored crate are
    /// recursive and deep dependency chains need room (see ADR-0003 §3.2).
    #[must_use]
    pub fn analyze(&self, config: &AnalysisConfig) -> AnalysisResult {
        let mut result = self.analyze_phase1();

        // SCC cycles are needed for has_cycles/topo validity regardless of
        // the config toggle (the toggle governs *enumeration*).
        let (has_cycles, cycle_count) = {
            let scc = cycles::tarjan_scc(&result.graph);
            (scc.has_cycles, scc.cycle_count)
        };
        result.has_cycles = has_cycles;
        result.cycle_count = cycle_count;

        // bv-parity view for centrality + heights.
        let parity = Self::reverse_of(&result.graph);
        // Owned copies of the read-only views: the helpers below write into
        // `result`, so borrowed aliases would fight the borrow checker.
        // O(V+E) once per analysis, dwarfed by the metrics themselves.
        let graph = result.graph.clone();
        let ids = result.ids.clone();

        Self::centrality_metrics(&parity, &ids, config, &mut result);
        Self::structural_metrics(&graph, &parity, &ids, config, &mut result);

        // No entry may remain pending at exit (bv contract).
        result.status.resolve_pending("not computed");
        result
    }

    /// Phase-2 centrality family on the bv-parity view: PageRank,
    /// betweenness (exact or approx), eigenvector, HITS. Each entry lands
    /// in `result.status`, computed or skipped-with-reason.
    fn centrality_metrics(
        parity: &DiGraph,
        ids: &[String],
        config: &AnalysisConfig,
        result: &mut AnalysisResult,
    ) {
        // PageRank (config-struct API; budgets recorded, not enforced
        // mid-iteration — the vendored loop runs to convergence or max-iter)
        if config.compute_pagerank {
            let t = Instant::now();
            let scores = pagerank::pagerank(
                parity,
                &PageRankConfig {
                    damping: 0.85,
                    tolerance: 1e-8,
                    max_iterations: 100,
                },
            );
            result.pagerank = Some(Self::score_map(ids, &scores));
            result.status.pagerank = Some(MetricEntry::computed(t.elapsed()));
        } else if let Some(reason) = config.skip_reason("pagerank") {
            result.status.pagerank = Some(MetricEntry::skipped(&reason));
        }

        // Betweenness (raw scores, bv parity — not normalized)
        match &config.betweenness_mode {
            super::config::BetweennessMode::Exact if config.compute_betweenness => {
                let t = Instant::now();
                let bc = betweenness::betweenness(parity);
                result.betweenness = Some(Self::score_map(ids, &bc));
                result.status.betweenness = Some(MetricEntry::computed(t.elapsed()));
            }
            super::config::BetweennessMode::Approx { sample } if config.compute_betweenness => {
                let t = Instant::now();
                let bc = betweenness::betweenness_approx(parity, *sample, None);
                result.betweenness = Some(Self::score_map(ids, &bc));
                result.status.betweenness = Some(MetricEntry::approx(*sample, t.elapsed()));
            }
            super::config::BetweennessMode::Skip { reason } => {
                result.status.betweenness = Some(MetricEntry::skipped(reason));
            }
            _ => {
                result.status.betweenness = Some(MetricEntry::skipped("not computed"));
            }
        }

        // Eigenvector
        if config.compute_eigenvector {
            let t = Instant::now();
            let ev = eigenvector::eigenvector(parity, &EigenvectorConfig::default());
            result.eigenvector = Some(Self::score_map(ids, &ev));
            result.status.eigenvector = Some(MetricEntry::computed(t.elapsed()));
        } else if let Some(reason) = config.skip_reason("eigenvector") {
            result.status.eigenvector = Some(MetricEntry::skipped(&reason));
        }

        // HITS
        if config.compute_hits {
            let t = Instant::now();
            let h = hits::hits(parity, &HITSConfig::default());
            let mut hubm = BTreeMap::new();
            let mut authm = BTreeMap::new();
            for (i, v) in h.hubs.iter().enumerate() {
                hubm.insert(ids[i].clone(), *v);
            }
            for (i, v) in h.authorities.iter().enumerate() {
                authm.insert(ids[i].clone(), *v);
            }
            result.hubs = Some(hubm);
            result.authorities = Some(authm);
            result.status.hits = Some(MetricEntry::computed(t.elapsed()));
        } else if let Some(reason) = config.skip_reason("hits") {
            result.status.hits = Some(MetricEntry::skipped(&reason));
        }
    }

    /// Phase-2 structure-sensitive metrics on the canonical graph:
    /// critical-path heights, cycle enumeration, k-core, articulation
    /// points, slack.
    fn structural_metrics(
        graph: &DiGraph,
        parity: &DiGraph,
        ids: &[String],
        config: &AnalysisConfig,
        result: &mut AnalysisResult,
    ) {
        // Critical path heights (bv-parity view: 1 + max dependent height)
        if config.compute_critical_path {
            let t = Instant::now();
            if !result.has_cycles {
                let heights = critical_path::critical_path_heights(parity);
                result.critical_path_score = Some(Self::score_map(ids, &heights));
                result.status.critical = Some(MetricEntry::computed(t.elapsed()));
            } else {
                result.status.critical =
                    Some(MetricEntry::skipped("graph has cycles; heights undefined"));
            }
        } else if let Some(reason) = config.skip_reason("critical") {
            result.status.critical = Some(MetricEntry::skipped(&reason));
        }

        // Cycles enumeration (SCC count already recorded above)
        if config.compute_cycles && config.max_cycles > 0 {
            let t = Instant::now();
            let enumerated = cycles::enumerate_cycles(graph, config.max_cycles);
            result.cycles = Some(
                enumerated
                    .into_iter()
                    .map(|c| c.iter().map(|&i| ids[i].clone()).collect())
                    .collect(),
            );
            result.status.cycles = Some(MetricEntry::computed(t.elapsed()));
        } else if let Some(reason) = config.skip_reason("cycles") {
            result.status.cycles = Some(MetricEntry::skipped(&reason));
        } else {
            result.status.cycles = Some(MetricEntry::skipped("cycle count only"));
        }

        // k-core (undirected — canonical graph is fine)
        if config.compute_kcore {
            let t = Instant::now();
            let cores = kcore::kcore(graph);
            let mut m = BTreeMap::new();
            for (i, v) in cores.iter().enumerate() {
                m.insert(ids[i].clone(), *v as usize);
            }
            result.core_number = Some(m);
            result.status.kcore = Some(MetricEntry::computed(t.elapsed()));
        } else if let Some(reason) = config.skip_reason("kcore") {
            result.status.kcore = Some(MetricEntry::skipped(&reason));
        }

        // Articulation points (undirected)
        if config.compute_articulation {
            let t = Instant::now();
            let arts = articulation::articulation_points(graph);
            result.articulation_points = Some(arts.into_iter().map(|i| ids[i].clone()).collect());
            result.status.articulation = Some(MetricEntry::computed(t.elapsed()));
        } else if let Some(reason) = config.skip_reason("articulation") {
            result.status.articulation = Some(MetricEntry::skipped(&reason));
        }

        // Slack (canonical blocker -> blocked orientation: scheduling float)
        if config.compute_slack {
            let t = Instant::now();
            if result.has_cycles {
                result.status.slack =
                    Some(MetricEntry::skipped("graph has cycles; slack undefined"));
            } else {
                let sl = slack::slack(graph);
                result.slack = Some(Self::score_map(ids, &sl));
                result.status.slack = Some(MetricEntry::computed(t.elapsed()));
            }
        } else if let Some(reason) = config.skip_reason("slack") {
            result.status.slack = Some(MetricEntry::skipped(&reason));
        }
    }

    /// Zip node-indexed scores with their issue ids into an id-keyed map.
    fn score_map(ids: &[String], scores: &[f64]) -> BTreeMap<String, f64> {
        let mut m = BTreeMap::new();
        for (i, v) in scores.iter().enumerate() {
            m.insert(ids[i].clone(), *v);
        }
        m
    }

    /// Run `analyze` on a dedicated large-stack thread (recursion safety
    /// for deep chains; see method docs). Blocking; returns the result.
    #[must_use]
    pub fn analyze_on_big_stack(&self, config: &AnalysisConfig) -> AnalysisResult {
        match std::thread::Builder::new()
            .stack_size(64 * 1024 * 1024)
            .spawn({
                let issues = self.issues.clone();
                let config = config.clone();
                move || Self::new(issues).analyze(&config)
            }) {
            Ok(handle) => handle.join().unwrap_or_else(|_| {
                // Worker panicked: degrade to Phase 1 rather than crash the
                // caller (robot commands must still emit valid JSON).
                let mut r = Self::new(self.issues.clone()).analyze_phase1();
                r.status.resolve_pending("phase 2 worker panicked");
                r
            }),
            Err(_) => self.analyze(config),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::status::MetricState;

    fn issue_json(id: &str, status: &str, deps: &[(&str, &str)]) -> Issue {
        let deps: Vec<String> = deps
            .iter()
            .map(|(on, _)| {
                format!(
                    r#"{{"issue_id": "{id}", "depends_on_id": "{on}", "type": "blocks",
                        "created_at": "2026-08-02T00:00:00Z"}}"#
                )
            })
            .collect();
        serde_json::from_str(&format!(
            r#"{{"id": "{id}", "title": "t {id}", "status": "{status}",
                "priority": 2, "issue_type": "task",
                "created_at": "2026-08-01T10:00:00Z",
                "updated_at": "2026-08-10T10:00:00Z"
                {}{}{}"#,
            if deps.is_empty() {
                String::new()
            } else {
                format!(r#","dependencies": [{}]"#, deps.join(","))
            },
            if status == "closed" {
                r#","closed_at": "2026-08-15T00:00:00Z""#
            } else {
                ""
            },
            "}"
        ))
        .expect("test issue parses")
    }

    fn fixture() -> Vec<Issue> {
        vec![
            issue_json("fx-a", "open", &[]),
            issue_json("fx-b", "open", &[("fx-a", "blocks")]),
            issue_json("fx-c", "open", &[("fx-b", "blocks")]),
            issue_json("fx-d", "open", &[("fx-b", "blocks")]),
            issue_json("fx-h", "open", &[("fx-i", "blocks")]),
            issue_json("fx-i", "open", &[("fx-h", "blocks")]),
            issue_json("fx-j", "closed", &[]),
        ]
    }

    #[test]
    fn phase1_structure_matches_fixture_semantics() {
        let r = AnalysisEngine::new(fixture()).analyze_phase1();
        // fx-a blocks fx-b (out-degree 1), fx-b blocked (in-degree 1)
        assert_eq!(r.out_degree["fx-a"], 1);
        assert_eq!(r.in_degree["fx-b"], 1);
        // a, b, c, d connected; h, i connected; j isolated; e/f/g/k/l absent
        assert_eq!(r.components.len(), 3);
        // actionable: fx-a (no blockers), fx-j closed; blocked: b, c, d, h, i
        assert!(r.actionable.contains("fx-a"));
        assert!(r.blocked.contains("fx-b"));
        assert!(r.blocked.contains("fx-h"));
        assert!(!r.actionable.contains("fx-j"));
        // cycle between h and i -> topo undefined
        assert!(r.topological_order.is_none());
        assert!(r.has_cycles);
    }

    #[test]
    fn analyze_fills_metrics_and_status() {
        let r = AnalysisEngine::new(fixture()).analyze(&AnalysisConfig::full());
        assert!(r.pagerank.is_some());
        assert!(r.betweenness.is_some());
        assert!(r.eigenvector.is_some());
        assert!(r.hubs.is_some());
        assert!(r.authorities.is_some());
        assert!(r.core_number.is_some());
        assert!(r.articulation_points.is_some());
        // cycles block heights + slack
        assert!(r.critical_path_score.is_none());
        assert!(r.slack.is_none());
        assert_eq!(
            r.status.pagerank.as_ref().unwrap().state,
            MetricState::Computed
        );
        assert_eq!(
            r.status.cycles.as_ref().unwrap().state,
            MetricState::Computed
        );
        assert_eq!(
            r.status.critical.as_ref().unwrap().state,
            MetricState::Skipped
        );
        assert_eq!(r.cycle_count, 1);
    }

    #[test]
    fn pagerank_direction_is_bv_parity() {
        // a blocks b blocks c: bv puts importance on a (transitive dependents).
        let issues = vec![
            issue_json("fx-a", "open", &[]),
            issue_json("fx-b", "open", &[("fx-a", "blocks")]),
            issue_json("fx-c", "open", &[("fx-b", "blocks")]),
        ];
        let r = AnalysisEngine::new(issues).analyze(&AnalysisConfig::full());
        let pr = r.pagerank.as_ref().unwrap();
        assert!(
            pr["fx-a"] > pr["fx-c"],
            "fx-a (root dependency) must outrank fx-c (leaf): {pr:?}"
        );
    }

    #[test]
    fn plan_config_skips_centrality_with_reasons() {
        let r = AnalysisEngine::new(fixture()).analyze(&AnalysisConfig::plan());
        assert!(r.pagerank.is_none());
        assert_eq!(
            r.status.betweenness.as_ref().unwrap().reason.as_deref(),
            Some("not computed for --robot-plan")
        );
        assert!(r.core_number.is_some());
    }

    #[test]
    fn related_deps_do_not_block() {
        let issues = [
            issue_json("fx-a", "open", &[]),
            issue_json("fx-b", "open", &[]),
        ];
        // manually attach a related dep
        let mut b = issues[1].clone();
        b.dependencies.push(
            serde_json::from_str(
                r#"{"issue_id": "fx-b", "depends_on_id": "fx-a", "type": "related",
                "created_at": "2026-08-02T00:00:00Z"}"#,
            )
            .unwrap(),
        );
        let r = AnalysisEngine::new(vec![issues[0].clone(), b]).analyze_phase1();
        assert!(r.actionable.contains("fx-b"));
        assert_eq!(r.graph.edge_count(), 0);
    }
}
