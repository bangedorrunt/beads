//! Analysis configuration tiers (ADR-0003 §3.2), porting bv
//! `analysis.ConfigForSize` (docs/research/bv-robot-surface.md §2).
// governed-by: ADR-0003

/// Default per-metric budget in milliseconds (bv uses 2s for tiny graphs).
pub const METRIC_DEFAULT_TIMEOUT_MS: u64 = 2_000;

/// How betweenness runs for this graph size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BetweennessMode {
    /// Brandes exact.
    Exact,
    /// Sampled approximation (pivots = `sample`).
    Approx { sample: usize },
    /// Off, with the reason surfaced in `status`.
    Skip { reason: String },
}

/// Which metrics run and with what budgets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisConfig {
    pub compute_pagerank: bool,
    pub pagerank_timeout_ms: u64,
    pub compute_betweenness: bool,
    pub betweenness_mode: BetweennessMode,
    pub compute_eigenvector: bool,
    pub compute_hits: bool,
    pub compute_critical_path: bool,
    pub compute_cycles: bool,
    pub max_cycles: usize,
    pub compute_kcore: bool,
    pub compute_articulation: bool,
    pub compute_slack: bool,
}

impl AnalysisConfig {
    /// Everything on, exact betweenness, generous budgets (bv
    /// `FullAnalysisConfig`, used by `--force-full-analysis`).
    #[must_use]
    pub fn full() -> Self {
        Self {
            compute_pagerank: true,
            pagerank_timeout_ms: 30_000,
            compute_betweenness: true,
            betweenness_mode: BetweennessMode::Exact,
            compute_eigenvector: true,
            compute_hits: true,
            compute_critical_path: true,
            compute_cycles: true,
            max_cycles: 1_000,
            compute_kcore: true,
            compute_articulation: true,
            compute_slack: true,
        }
    }

    /// The `--robot-plan` profile: Phase-2 centrality off with explicit skip
    /// reasons (plan needs only components + actionable set).
    #[must_use]
    pub fn plan() -> Self {
        Self {
            compute_pagerank: false,
            pagerank_timeout_ms: METRIC_DEFAULT_TIMEOUT_MS,
            compute_betweenness: false,
            betweenness_mode: BetweennessMode::Skip {
                reason: "not computed for --robot-plan".to_string(),
            },
            compute_eigenvector: false,
            compute_hits: false,
            compute_critical_path: false,
            compute_cycles: false,
            max_cycles: 1_000,
            compute_kcore: true,
            compute_articulation: true,
            compute_slack: true,
        }
    }

    /// Skip reason for a disabled metric (null when it runs).
    #[must_use]
    pub fn skip_reason(&self, metric: &str) -> Option<String> {
        let reason = match metric {
            "pagerank" if !self.compute_pagerank => "not computed",
            "betweenness" => {
                return match &self.betweenness_mode {
                    BetweennessMode::Skip { reason } => Some(reason.clone()),
                    _ => None,
                };
            }
            "eigenvector" if !self.compute_eigenvector => "not computed",
            "hits" if !self.compute_hits => "not computed",
            "critical" if !self.compute_critical_path => "not computed",
            "cycles" if !self.compute_cycles => "not computed",
            "kcore" if !self.compute_kcore => "not computed",
            "articulation" if !self.compute_articulation => "not computed",
            "slack" if !self.compute_slack => "not computed",
            _ => return None,
        };
        Some(reason.to_string())
    }

    /// Size-tiered defaults (bv `ConfigForSize`):
    /// `<100` exact/2s, `<500` exact/500ms, `<2000` approx-if-sparse
    /// else skip "graph too dense", `>=2000` approx + cycles off
    /// ("graph too large"), HITS only if density < 0.001 at huge sizes.
    #[must_use]
    pub fn for_size(node_count: usize, density: f64) -> Self {
        let mut cfg = Self::full();
        if node_count < 100 {
            cfg.pagerank_timeout_ms = METRIC_DEFAULT_TIMEOUT_MS;
        } else if node_count < 500 {
            cfg.pagerank_timeout_ms = 500;
        } else if node_count < 2_000 {
            cfg.pagerank_timeout_ms = 500;
            cfg.betweenness_mode = if density < 0.01 {
                BetweennessMode::Approx { sample: 512 }
            } else {
                BetweennessMode::Skip {
                    reason: "graph too dense (density > 0.01)".to_string(),
                }
            };
        } else {
            cfg.pagerank_timeout_ms = 500;
            cfg.betweenness_mode = BetweennessMode::Approx { sample: 512 };
            cfg.compute_cycles = false;
            cfg.max_cycles = 0;
        }
        cfg
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiers_match_bv_table() {
        let tiny = AnalysisConfig::for_size(50, 0.5);
        assert_eq!(tiny.betweenness_mode, BetweennessMode::Exact);
        assert_eq!(tiny.pagerank_timeout_ms, 2_000);

        let medium = AnalysisConfig::for_size(400, 0.5);
        assert_eq!(medium.betweenness_mode, BetweennessMode::Exact);
        assert_eq!(medium.pagerank_timeout_ms, 500);

        let sparse_large = AnalysisConfig::for_size(1_500, 0.001);
        assert_eq!(
            sparse_large.betweenness_mode,
            BetweennessMode::Approx { sample: 512 }
        );
        assert!(sparse_large.compute_cycles);

        let dense_large = AnalysisConfig::for_size(1_500, 0.5);
        assert!(matches!(
            dense_large.betweenness_mode,
            BetweennessMode::Skip { .. }
        ));

        let huge = AnalysisConfig::for_size(5_000, 0.001);
        assert!(!huge.compute_cycles);
    }

    #[test]
    fn plan_profile_skips_centrality_with_reason() {
        let cfg = AnalysisConfig::plan();
        assert_eq!(
            cfg.skip_reason("betweenness").as_deref(),
            Some("not computed for --robot-plan")
        );
        assert!(cfg.skip_reason("pagerank").is_some());
        assert!(cfg.skip_reason("kcore").is_none());
    }
}
