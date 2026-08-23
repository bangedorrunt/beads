//! Per-metric computation status (ADR-0003 §3.1 envelope contract).
// governed-by: ADR-0003
//!
//! Mirrors bv's `MetricStatus`: PascalCase keys `PageRank, Betweenness,
//! Eigenvector, HITS, Critical, Cycles, KCore, Articulation, Slack`, each
//! `{state, reason?, sample?, ms?}` with `state` one of
//! `computed | approx | timeout | skipped | pending`. At process exit no
//! entry may remain `pending` (bv enforces this in tests; so do we).

use std::collections::BTreeMap;
use std::time::Duration;

use serde::Serialize;

/// Computation state for one metric.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricState {
    Computed,
    Approx,
    Timeout,
    Skipped,
    Pending,
}

/// Status entry for one metric.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct MetricEntry {
    pub state: MetricState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sample: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ms: Option<f64>,
}

impl MetricEntry {
    /// A plain computed entry with elapsed time.
    #[must_use]
    pub fn computed(ms: Duration) -> Self {
        Self {
            state: MetricState::Computed,
            reason: None,
            sample: None,
            ms: Some(ms.as_secs_f64() * 1_000.0),
        }
    }

    /// An approximate (sampled) entry.
    #[must_use]
    pub fn approx(sample: usize, ms: Duration) -> Self {
        Self {
            state: MetricState::Approx,
            reason: Some("approximate".to_string()),
            sample: Some(sample),
            ms: Some(ms.as_secs_f64() * 1_000.0),
        }
    }

    /// A skipped entry with the reason (e.g. "not computed for --robot-plan").
    #[must_use]
    pub fn skipped(reason: &str) -> Self {
        Self {
            state: MetricState::Skipped,
            reason: Some(reason.to_string()),
            sample: None,
            ms: None,
        }
    }

    /// A timed-out entry.
    #[must_use]
    pub fn timeout(ms: Duration) -> Self {
        Self {
            state: MetricState::Timeout,
            reason: Some("budget exceeded".to_string()),
            sample: None,
            ms: Some(ms.as_secs_f64() * 1_000.0),
        }
    }

    /// A pending entry (never present in final output).
    #[must_use]
    pub fn pending() -> Self {
        Self {
            state: MetricState::Pending,
            reason: None,
            sample: None,
            ms: None,
        }
    }

    /// True when the state is one of the claim-unsafe degraded states
    /// (bv `ClaimUnsafeReasons`): pending, timeout, skipped.
    #[must_use]
    pub fn claim_unsafe(&self) -> bool {
        matches!(
            self.state,
            MetricState::Pending | MetricState::Timeout | MetricState::Skipped
        )
    }
}

/// The nine tracked metrics, serialized with bv's PascalCase keys.
#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct MetricStatus {
    #[serde(rename = "PageRank")]
    pub pagerank: Option<MetricEntry>,
    #[serde(rename = "Betweenness")]
    pub betweenness: Option<MetricEntry>,
    #[serde(rename = "Eigenvector")]
    pub eigenvector: Option<MetricEntry>,
    #[serde(rename = "HITS")]
    pub hits: Option<MetricEntry>,
    #[serde(rename = "Critical")]
    pub critical: Option<MetricEntry>,
    #[serde(rename = "Cycles")]
    pub cycles: Option<MetricEntry>,
    #[serde(rename = "KCore")]
    pub kcore: Option<MetricEntry>,
    #[serde(rename = "Articulation")]
    pub articulation: Option<MetricEntry>,
    #[serde(rename = "Slack")]
    pub slack: Option<MetricEntry>,
}

impl MetricStatus {
    /// All-pending status (fresh analysis).
    #[must_use]
    pub fn pending_all() -> Self {
        Self {
            pagerank: Some(MetricEntry::pending()),
            betweenness: Some(MetricEntry::pending()),
            eigenvector: Some(MetricEntry::pending()),
            hits: Some(MetricEntry::pending()),
            critical: Some(MetricEntry::pending()),
            cycles: Some(MetricEntry::pending()),
            kcore: Some(MetricEntry::pending()),
            articulation: Some(MetricEntry::pending()),
            slack: Some(MetricEntry::pending()),
        }
    }

    /// Mark every still-pending metric skipped with a reason (used when a
    /// consumer only needs Phase 1).
    pub fn resolve_pending(&mut self, reason: &str) {
        for entry in [
            &mut self.pagerank,
            &mut self.betweenness,
            &mut self.eigenvector,
            &mut self.hits,
            &mut self.critical,
            &mut self.cycles,
            &mut self.kcore,
            &mut self.articulation,
            &mut self.slack,
        ]
        .into_iter()
        .flatten()
        {
            if entry.state == MetricState::Pending {
                *entry = MetricEntry::skipped(reason);
            }
        }
    }

    /// Claim-unsafe when PageRank or Betweenness is degraded (bv contract).
    #[must_use]
    pub fn claim_unsafe_reasons(&self) -> Vec<&'static str> {
        let mut reasons = Vec::new();
        if self.pagerank.as_ref().is_none_or(MetricEntry::claim_unsafe) {
            reasons.push("PageRank incomplete");
        }
        if self
            .betweenness
            .as_ref()
            .is_none_or(MetricEntry::claim_unsafe)
        {
            reasons.push("Betweenness incomplete");
        }
        reasons
    }
}

/// Serialize with bv's key names (PascalCase) for golden parity tests.
#[must_use]
pub fn to_value_map(status: &MetricStatus) -> BTreeMap<String, MetricEntry> {
    let mut m = BTreeMap::new();
    if let Some(e) = &status.pagerank {
        m.insert("PageRank".into(), e.clone());
    }
    if let Some(e) = &status.betweenness {
        m.insert("Betweenness".into(), e.clone());
    }
    if let Some(e) = &status.eigenvector {
        m.insert("Eigenvector".into(), e.clone());
    }
    if let Some(e) = &status.hits {
        m.insert("HITS".into(), e.clone());
    }
    if let Some(e) = &status.critical {
        m.insert("Critical".into(), e.clone());
    }
    if let Some(e) = &status.cycles {
        m.insert("Cycles".into(), e.clone());
    }
    if let Some(e) = &status.kcore {
        m.insert("KCore".into(), e.clone());
    }
    if let Some(e) = &status.articulation {
        m.insert("Articulation".into(), e.clone());
    }
    if let Some(e) = &status.slack {
        m.insert("Slack".into(), e.clone());
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_all_then_resolve() {
        let mut s = MetricStatus::pending_all();
        assert_eq!(s.pagerank.as_ref().unwrap().state, MetricState::Pending);
        s.pagerank = Some(MetricEntry::computed(Duration::from_millis(1)));
        s.resolve_pending("phase 1 only");
        assert_eq!(s.pagerank.as_ref().unwrap().state, MetricState::Computed);
        assert_eq!(s.betweenness.as_ref().unwrap().state, MetricState::Skipped);
        assert_eq!(
            s.betweenness.as_ref().unwrap().reason.as_deref(),
            Some("phase 1 only")
        );
    }

    #[test]
    fn claim_unsafe_contract() {
        let mut s = MetricStatus::pending_all();
        assert_eq!(s.claim_unsafe_reasons().len(), 2);
        s.pagerank = Some(MetricEntry::computed(Duration::from_millis(1)));
        s.betweenness = Some(MetricEntry::timeout(Duration::from_millis(500)));
        assert_eq!(s.claim_unsafe_reasons(), vec!["Betweenness incomplete"]);
    }

    #[test]
    fn pascal_case_keys() {
        let s = MetricStatus {
            cycles: Some(MetricEntry::skipped("not computed for --robot-plan")),
            ..MetricStatus::default()
        };
        let v = serde_json::to_value(to_value_map(&s)).unwrap();
        assert!(v.get("Cycles").is_some());
        assert_eq!(v["Cycles"]["state"], "skipped");
        assert_eq!(v["Cycles"]["reason"], "not computed for --robot-plan");
    }
}
