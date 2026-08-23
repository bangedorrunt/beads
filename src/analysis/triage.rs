//! Triage scoring + robot payloads (ADR-0003 §3.1/§4, bead gu7ts.3).
// governed-by: ADR-0003
//!
//! Faithful port of beads_viewer `pkg/analysis` (priority.go, triage.go,
//! risk.go) plus the `--robot-next` claim walk from
//! `cmd/bv/robot_registry.go`, computed over this fork's SQLite-backed
//! `Issue` set through [`AnalysisEngine`]. User-visible strings (reasons,
//! action hints, degraded messages) are byte-identical to bv so the golden
//! fixtures pin real output (`tests/fixtures/bv_parity/`,
//! `tests/robot_parity_triage_next.rs`).
//!
//! Deliberate bv quirks kept for parity (each pinned by a golden):
//! * the emitted betweenness status reads `computed` with reason
//!   `"approximate"` even though Brandes runs exact here;
//! * `Critical` reports `skipped` although critical-path heights feed
//!   time-to-impact scoring;
//! * the triage claimability gate ignores dangling dependency ids while
//!   robot-next's diagnostic walk treats them as `"<id> (missing)"` open
//!   blockers — the two bv surfaces genuinely disagree and both sides are
//!   reproduced.
//!
//! Clock: every time-derived signal takes an explicit `now`; the CLI sources
//! it from `BR_ANALYSIS_NOW` when set so parity tests never rot.

use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::{DateTime, Datelike, SecondsFormat, Timelike, Utc};
use serde::Serialize;

use crate::model::{DependencyType, Issue, IssueType, Status};

use super::config::{AnalysisConfig, BetweennessMode, METRIC_DEFAULT_TIMEOUT_MS};
use super::engine::AnalysisEngine;
use super::status::{MetricEntry, MetricState, MetricStatus};

// Base-score weights (bv priority.go; sum = 1.0).
const WEIGHT_PAGERANK: f64 = 0.22;
const WEIGHT_BETWEENNESS: f64 = 0.20;
const WEIGHT_BLOCKER_RATIO: f64 = 0.13;
const WEIGHT_STALENESS: f64 = 0.05;
const WEIGHT_PRIORITY_BOOST: f64 = 0.10;
const WEIGHT_TIME_TO_IMPACT: f64 = 0.10;
const WEIGHT_URGENCY: f64 = 0.10;
const WEIGHT_RISK: f64 = 0.10;

/// Urgency labels (substring match against the lowercased label).
const URGENCY_LABELS: &[&str] = &["urgent", "critical", "blocker", "hotfix", "asap"];

const DEFAULT_ESTIMATED_MINUTES: i32 = 60;
const MAX_CRITICAL_PATH_DEPTH: f64 = 10.0;
const URGENCY_DECAY_DAYS: f64 = 7.0;

// Triage-scoring options (bv-147 defaults).
const BASE_SCORE_WEIGHT: f64 = 0.70;
const UNBLOCK_BOOST_WEIGHT: f64 = 0.15;
const QUICK_WIN_WEIGHT: f64 = 0.15;
const UNBLOCK_THRESHOLD: usize = 5;
const QUICK_WIN_MAX_DEPTH: i64 = 2;

const RECOMMENDATIONS_LIMIT: usize = 10;
const QUICK_WINS_LIMIT: usize = 5;
const BLOCKERS_LIMIT: usize = 5;
const TOP_PICKS_LIMIT: usize = 3;

// ---------------------------------------------------------------------------
// JSON payload types (field-for-field with bv's Go structs)
// ---------------------------------------------------------------------------

/// Go `RFC3339Nano`: trailing-zero-trimmed fraction, omitted when zero.
pub(crate) fn go_time_nanos(t: DateTime<Utc>) -> String {
    let base = t.format("%Y-%m-%dT%H:%M:%S").to_string();
    let nanos = t.nanosecond() % 1_000_000_000;
    if nanos == 0 {
        return format!("{base}Z");
    }
    let trimmed = format!("{nanos:09}").trim_end_matches('0').to_string();
    format!("{base}.{trimmed}Z")
}

pub(crate) fn go_time_secs(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Secs, true)
}

#[derive(Debug, Clone, Serialize)]
pub struct RiskSignals {
    pub fan_variance: f64,
    pub activity_churn: f64,
    pub cross_repo_risk: f64,
    pub status_risk: f64,
    pub composite_risk: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explanation: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScoreBreakdown {
    pub pagerank: f64,
    pub betweenness: f64,
    pub blocker_ratio: f64,
    pub staleness: f64,
    pub priority_boost: f64,
    pub time_to_impact: f64,
    pub urgency: f64,
    pub risk: f64,

    pub pagerank_norm: f64,
    pub betweenness_norm: f64,
    pub blocker_ratio_norm: f64,
    pub staleness_norm: f64,
    pub priority_boost_norm: f64,
    pub time_to_impact_norm: f64,
    pub urgency_norm: f64,
    pub risk_norm: f64,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_to_impact_explanation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub urgency_explanation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk_explanation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk_signals: Option<RiskSignals>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TopPick {
    pub id: String,
    pub title: String,
    pub score: f64,
    pub reasons: Vec<String>,
    pub unblocks: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Recommendation {
    pub id: String,
    pub title: String,
    #[serde(rename = "type")]
    pub issue_type: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignee: Option<String>,
    pub priority: i64,
    pub labels: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub defer_until: Option<String>,
    pub score: f64,
    pub breakdown: ScoreBreakdown,
    pub action: String,
    pub reasons: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unblocks_ids: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub blocked_by: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct QuickWin {
    pub id: String,
    pub title: String,
    pub score: f64,
    pub reason: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unblocks_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BlockerItem {
    pub id: String,
    pub title: String,
    pub unblocks_count: usize,
    pub unblocks_ids: Vec<String>,
    pub actionable: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub blocked_by: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HealthCounts {
    pub total: usize,
    pub open: usize,
    pub closed: usize,
    pub blocked: usize,
    pub actionable: usize,
    pub not_closed: usize,
    pub dependency_blocked: usize,
    pub by_status: BTreeMap<String, usize>,
    pub by_type: BTreeMap<String, usize>,
    pub by_priority: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GraphHealth {
    pub node_count: usize,
    pub edge_count: usize,
    pub density: f64,
    pub has_cycles: bool,
    #[serde(skip_serializing_if = "is_zero")]
    pub cycle_count: usize,
    pub phase2_ready: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct VelocityWeek {
    pub week_start: String,
    pub closed: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Velocity {
    pub closed_last_7_days: usize,
    pub closed_last_30_days: usize,
    pub avg_days_to_close: f64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub weekly: Vec<VelocityWeek>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub estimated: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectHealth {
    pub counts: HealthCounts,
    pub graph: GraphHealth,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub velocity: Option<Velocity>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CommandHelpers {
    pub claim_top: String,
    pub show_top: String,
    pub list_ready: String,
    pub list_blocked: String,
    pub refresh_triage: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TriageMeta {
    pub version: String,
    pub generated_at: String,
    pub phase2_ready: bool,
    pub issue_count: usize,
    pub compute_time_ms: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct QuickRef {
    pub open_count: usize,
    pub actionable_count: usize,
    pub blocked_count: usize,
    pub in_progress_count: usize,
    pub not_closed_count: usize,
    pub not_actionable_count: usize,
    pub top_picks: Vec<TopPick>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TriageResult {
    pub meta: TriageMeta,
    pub status: MetricStatus,
    pub quick_ref: QuickRef,
    pub recommendations: Vec<Recommendation>,
    pub quick_wins: Vec<QuickWin>,
    pub blockers_to_clear: Vec<BlockerItem>,
    pub project_health: ProjectHealth,
    pub commands: CommandHelpers,
}

// ---------------------------------------------------------------------------
// Scoring primitives (priority.go / risk.go ports)
// ---------------------------------------------------------------------------

fn compute_staleness(updated_at: DateTime<Utc>, now: DateTime<Utc>) -> f64 {
    ((now - updated_at).num_hours() as f64 / 24.0 / 30.0).clamp(0.0, 1.0)
}

fn compute_priority_boost(priority: i64) -> f64 {
    match priority {
        0 => 1.0,
        1 => 0.75,
        2 => 0.5,
        3 => 0.25,
        _ => 0.0,
    }
}

fn normalize(v: f64, max: f64) -> f64 {
    if max == 0.0 { 0.0 } else { v / max }
}

fn compute_time_to_impact(
    critical_depth: f64,
    estimated_minutes: Option<i32>,
    median_minutes: i32,
) -> (f64, String) {
    let mut effective_minutes = median_minutes;
    let mut estimate_source = "median";
    if let Some(minutes) = estimated_minutes.filter(|m| *m > 0) {
        effective_minutes = minutes;
        estimate_source = "explicit";
    }

    let depth_norm = (critical_depth / MAX_CRITICAL_PATH_DEPTH).min(1.0);
    const MAX_MINUTES: f64 = 480.0;
    let time_factor = (1.0 - f64::from(effective_minutes) / MAX_MINUTES).clamp(0.0, 1.0);
    let score = depth_norm * 0.7 + time_factor * 0.3;

    let explanation = if critical_depth >= 3.0 {
        format!(
            "Deep in critical path (depth {critical_depth:.0}), {estimate_source} estimate {effective_minutes}m"
        )
    } else if critical_depth >= 1.0 {
        format!(
            "On dependency chain (depth {critical_depth:.0}), {estimate_source} estimate {effective_minutes}m"
        )
    } else {
        format!("Leaf node, {estimate_source} estimate {effective_minutes}m")
    };

    (score, explanation)
}

fn compute_urgency(issue: &Issue, now: DateTime<Utc>) -> (f64, Option<String>) {
    let mut score = 0.0_f64;
    let mut reasons: Vec<String> = Vec::new();

    'outer: for label in &issue.labels {
        let lower = label.to_lowercase();
        for urgent_label in URGENCY_LABELS {
            if lower.contains(urgent_label) {
                match *urgent_label {
                    "critical" | "blocker" => score += 1.0,
                    "urgent" | "hotfix" => score += 0.8,
                    "asap" => score += 0.6,
                    _ => {}
                }
                reasons.push(format!("has '{label}' label"));
                break 'outer;
            }
        }
    }

    let created_at = issue.created_at;
    let days_since_created = (now - created_at).num_hours() as f64 / 24.0;
    if days_since_created > 0.0 {
        score += 0.5 * (1.0 - (-days_since_created / URGENCY_DECAY_DAYS).exp());
        if days_since_created >= 14.0 {
            // Go %.0f and Rust {:.0} share IEEE round-half-even; no extra
            // rounding here or the edge cases drift apart.
            reasons.push(format!("aging ({:.0} days)", days_since_created));
        }
    }

    score = score.min(1.0);
    let explanation = if !reasons.is_empty() {
        Some(reasons.join(", "))
    } else if score > 0.1 {
        Some("moderate time pressure".to_string())
    } else {
        None
    };
    (score, explanation)
}

/// bv's blocking relation set for triage bookkeeping: parent-child counts as
/// a claimability blocker ("open blockers including parents"), unlike the
/// analysis engine's schedule-edge set. No current golden distinguishes the
/// two; this follows the Go source we ported.
pub fn is_triage_blocking(dep_type: &DependencyType) -> bool {
    matches!(
        dep_type,
        DependencyType::Blocks
            | DependencyType::ConditionalBlocks
            | DependencyType::WaitsFor
            | DependencyType::ParentChild
    )
}

fn is_closed_like(issue: &Issue) -> bool {
    matches!(issue.status, Status::Closed | Status::Tombstone)
}

fn compute_activity_churn(issue: &Issue, now: DateTime<Utc>) -> f64 {
    let created_at = issue.created_at;
    let age_days = ((now - created_at).num_hours() as f64 / 24.0).max(1.0);

    let comment_churn = issue.comments.len() as f64 / age_days;

    let mut update_recency = 0.0;
    {
        let updated_at = issue.updated_at;
        let update_span = (updated_at - created_at).num_hours() as f64 / 24.0;
        if update_span > 0.0 && age_days > 1.0 {
            update_recency = update_span / age_days;
        }
    }

    (comment_churn * 0.6 + update_recency * 0.4).min(1.0)
}

fn compute_cross_repo_risk(issue: &Issue, by_id: &HashMap<&str, &Issue>) -> f64 {
    let Some(this_repo) = issue.source_repo.as_deref().filter(|r| !r.is_empty()) else {
        return 0.0;
    };
    let mut cross_repo_count = 0usize;
    let mut total_blocking_deps = 0usize;
    for dep in &issue.dependencies {
        if !is_triage_blocking(&dep.dep_type) {
            continue;
        }
        total_blocking_deps += 1;
        if let Some(dep_issue) = by_id.get(dep.depends_on_id.as_str()) {
            if let Some(repo) = dep_issue.source_repo.as_deref().filter(|r| !r.is_empty()) {
                if repo != this_repo {
                    cross_repo_count += 1;
                }
            }
        }
    }
    if total_blocking_deps == 0 {
        return 0.0;
    }
    cross_repo_count as f64 / total_blocking_deps as f64
}

fn compute_status_risk(issue: &Issue, now: DateTime<Utc>) -> f64 {
    let days_since_update = (now - issue.updated_at).num_hours() as f64 / 24.0;
    match issue.status {
        Status::Closed | Status::Tombstone => 0.0,
        Status::Blocked if days_since_update > 7.0 => 0.9,
        Status::Blocked => 0.7,
        Status::InProgress if days_since_update > 14.0 => 0.8,
        Status::InProgress if days_since_update > 7.0 => 0.4,
        Status::InProgress => 0.1,
        Status::Open if (now - issue.created_at).num_hours() as f64 / 24.0 > 30.0 => 0.3,
        Status::Open => 0.1,
        _ => 0.0,
    }
}

fn compute_risk_signals(
    issue: &Issue,
    blocker_counts: &BTreeMap<String, usize>,
    by_id: &HashMap<&str, &Issue>,
    now: DateTime<Utc>,
) -> RiskSignals {
    let mut degrees: Vec<f64> = Vec::new();
    for dep in &issue.dependencies {
        if !is_triage_blocking(&dep.dep_type) {
            continue;
        }
        degrees.push(f64::from(
            blocker_counts.get(&dep.depends_on_id).copied().unwrap_or(0) as u32,
        ));
    }
    let fan_variance = if degrees.len() < 2 {
        0.0
    } else {
        let mean = degrees.iter().sum::<f64>() / degrees.len() as f64;
        if mean == 0.0 {
            0.0
        } else {
            let variance =
                degrees.iter().map(|d| (d - mean) * (d - mean)).sum::<f64>() / degrees.len() as f64;
            (variance.sqrt() / mean / 2.0).min(1.0)
        }
    };

    let activity_churn = compute_activity_churn(issue, now);
    let cross_repo_risk = compute_cross_repo_risk(issue, by_id);
    let status_risk = compute_status_risk(issue, now);

    let composite =
        (fan_variance * 0.30 + activity_churn * 0.30 + cross_repo_risk * 0.20 + status_risk * 0.20)
            .min(1.0);

    let explanation = if composite < 0.2 {
        Some("Low risk - stable dependency structure".to_string())
    } else {
        let mut factors: Vec<&str> = Vec::new();
        if fan_variance > 0.5 {
            factors.push("high dependency variance");
        }
        if activity_churn > 0.6 {
            factors.push("high activity churn");
        }
        if cross_repo_risk > 0.3 {
            factors.push("cross-repo dependencies");
        }
        if status_risk > 0.5 {
            factors.push("status indicates potential blockers");
        }
        Some(if factors.is_empty() {
            "Moderate risk".to_string()
        } else {
            format!("Risk factors: {}", factors.join(", "))
        })
    };

    RiskSignals {
        fan_variance,
        activity_churn,
        cross_repo_risk,
        status_risk,
        composite_risk: composite,
        explanation,
    }
}

// ---------------------------------------------------------------------------
// TriageGraph: open blockers, unblocks map, depths, parents, fan-out
// ---------------------------------------------------------------------------

pub(crate) struct TriageGraph<'a> {
    by_id: HashMap<&'a str, &'a Issue>,
    /// Known, non-closed blockers per non-closed issue (dangling ids dropped —
    /// bv's TriageContext behavior, distinct from the next-side walk).
    open_blockers: BTreeMap<String, Vec<String>>,
    /// Issues each issue uniquely unblocks (single-open-blocker rule).
    unblocks: BTreeMap<String, Vec<String>>,
    /// Fan-out: how many issues depend on this one.
    blocker_counts: BTreeMap<String, usize>,
    parents_with_open_children: HashSet<String>,
}

impl<'a> TriageGraph<'a> {
    fn new(issues: &'a [Issue]) -> Self {
        let by_id: HashMap<&str, &Issue> = issues
            .iter()
            .map(|issue| (issue.id.as_str(), issue))
            .collect();

        let mut open_blockers: BTreeMap<String, Vec<String>> = issues
            .iter()
            .filter(|issue| !is_closed_like(issue))
            .map(|issue| (issue.id.clone(), Vec::new()))
            .collect();

        let mut blocker_counts: BTreeMap<String, usize> =
            issues.iter().map(|issue| (issue.id.clone(), 0)).collect();

        for issue in issues {
            for dep in &issue.dependencies {
                if !is_triage_blocking(&dep.dep_type) {
                    continue;
                }
                let Some(blocker) = by_id.get(dep.depends_on_id.as_str()) else {
                    continue; // dangling edges ignored here (bv quirk, see mod docs)
                };
                if is_closed_like(blocker) {
                    continue;
                }
                if let Some(list) = open_blockers.get_mut(issue.id.as_str()) {
                    list.push(dep.depends_on_id.clone());
                }
                *blocker_counts.entry(dep.depends_on_id.clone()).or_insert(0) += 1;
            }
        }
        for list in open_blockers.values_mut() {
            list.sort();
        }

        let mut unblocks: BTreeMap<String, Vec<String>> = issues
            .iter()
            .filter(|issue| !is_closed_like(issue))
            .map(|issue| (issue.id.clone(), Vec::new()))
            .collect();
        for (id, blockers) in &open_blockers {
            if blockers.len() == 1 {
                unblocks
                    .entry(blockers[0].clone())
                    .or_default()
                    .push(id.clone());
            }
        }
        for list in unblocks.values_mut() {
            list.sort();
        }

        let mut parents_with_open_children: HashSet<String> = HashSet::new();
        for issue in issues.iter().filter(|issue| !is_closed_like(issue)) {
            for dep in &issue.dependencies {
                if dep.dep_type == DependencyType::ParentChild {
                    parents_with_open_children.insert(dep.depends_on_id.clone());
                }
            }
        }

        Self {
            by_id,
            open_blockers,
            unblocks,
            blocker_counts,
            parents_with_open_children,
        }
    }

    fn open_blockers_of(&self, id: &str) -> &[String] {
        self.open_blockers.get(id).map(Vec::as_slice).unwrap_or(&[])
    }

    fn unblocks_of(&self, id: &str) -> &[String] {
        self.unblocks.get(id).map(Vec::as_slice).unwrap_or(&[])
    }

    fn blocker_depth(&self, start: &str) -> i64 {
        fn recurse(
            graph: &TriageGraph<'_>,
            id: &str,
            visited: &mut HashSet<String>,
            memo: &mut HashMap<String, i64>,
        ) -> i64 {
            if let Some(val) = memo.get(id) {
                return *val;
            }
            if visited.contains(id) {
                return -1; // cycle
            }
            visited.insert(id.to_string());

            let blockers = graph.open_blockers_of(id);
            let depth = if blockers.is_empty() {
                0
            } else {
                let mut max_chain = 0;
                for blocker in blockers {
                    let d = recurse(graph, blocker, visited, memo);
                    if d == -1 {
                        return -1;
                    }
                    max_chain = max_chain.max(d + 1);
                }
                max_chain
            };

            visited.remove(id);
            memo.insert(id.to_string(), depth);
            depth
        }
        let mut visited = HashSet::new();
        let mut memo = HashMap::new();
        recurse(self, start, &mut visited, &mut memo)
    }

    fn parent_has_open_children(&self, id: &str) -> bool {
        self.parents_with_open_children.contains(id)
    }
}

// ---------------------------------------------------------------------------
// Impact + triage scoring
// ---------------------------------------------------------------------------

struct ImpactScore {
    id: String,
    title: String,
    priority: i64,
    score: f64,
    breakdown: ScoreBreakdown,
}

fn median_estimated_minutes(issues: &[Issue]) -> i32 {
    let mut estimates: Vec<i32> = issues
        .iter()
        .filter_map(|issue| issue.estimated_minutes.filter(|m| *m > 0))
        .collect();
    if estimates.is_empty() {
        return DEFAULT_ESTIMATED_MINUTES;
    }
    estimates.sort_unstable();
    let mid = estimates.len() / 2;
    if estimates.len() % 2 == 0 {
        (estimates[mid - 1] + estimates[mid]) / 2
    } else {
        estimates[mid]
    }
}

#[allow(clippy::too_many_lines)]
fn compute_impact_scores(
    issues: &[Issue],
    graph: &TriageGraph<'_>,
    pagerank: &BTreeMap<String, f64>,
    betweenness: &BTreeMap<String, f64>,
    critical_path: &BTreeMap<String, f64>,
    now: DateTime<Utc>,
) -> Vec<ImpactScore> {
    let max_pr = pagerank.values().fold(0.0_f64, |a, v| a.max(*v));
    let max_bw = betweenness.values().fold(0.0_f64, |a, v| a.max(*v));
    let max_blockers = graph.blocker_counts.values().copied().max().unwrap_or(0);
    let median_minutes = median_estimated_minutes(issues);

    let mut scores: Vec<ImpactScore> = Vec::new();
    for issue in issues.iter().filter(|issue| !is_closed_like(issue)) {
        let pr_norm = normalize(pagerank.get(&issue.id).copied().unwrap_or(0.0), max_pr);
        let bw_norm = normalize(betweenness.get(&issue.id).copied().unwrap_or(0.0), max_bw);
        let blocker_norm = normalize(
            graph.blocker_counts.get(&issue.id).copied().unwrap_or(0) as f64,
            f64::from(max_blockers as u32),
        );
        let staleness_norm = compute_staleness(issue.updated_at, now);
        let priority_norm = compute_priority_boost(issue.priority.0 as i64);

        let depth = critical_path.get(&issue.id).copied().unwrap_or(0.0);
        let (tti_norm, tti_explanation) =
            compute_time_to_impact(depth, issue.estimated_minutes, median_minutes);
        let (urgency_norm, urgency_explanation) = compute_urgency(issue, now);
        let risk_signals = compute_risk_signals(issue, &graph.blocker_counts, &graph.by_id, now);

        let breakdown = ScoreBreakdown {
            pagerank: pr_norm * WEIGHT_PAGERANK,
            betweenness: bw_norm * WEIGHT_BETWEENNESS,
            blocker_ratio: blocker_norm * WEIGHT_BLOCKER_RATIO,
            staleness: staleness_norm * WEIGHT_STALENESS,
            priority_boost: priority_norm * WEIGHT_PRIORITY_BOOST,
            time_to_impact: tti_norm * WEIGHT_TIME_TO_IMPACT,
            urgency: urgency_norm * WEIGHT_URGENCY,
            risk: risk_signals.composite_risk * WEIGHT_RISK,

            pagerank_norm: pr_norm,
            betweenness_norm: bw_norm,
            blocker_ratio_norm: blocker_norm,
            staleness_norm,
            priority_boost_norm: priority_norm,
            time_to_impact_norm: tti_norm,
            urgency_norm,
            risk_norm: risk_signals.composite_risk,

            time_to_impact_explanation: Some(tti_explanation),
            urgency_explanation,
            risk_explanation: risk_signals.explanation.clone(),
            risk_signals: Some(risk_signals),
        };

        let score = breakdown.pagerank
            + breakdown.betweenness
            + breakdown.blocker_ratio
            + breakdown.staleness
            + breakdown.priority_boost
            + breakdown.time_to_impact
            + breakdown.urgency
            + breakdown.risk;

        scores.push(ImpactScore {
            id: issue.id.clone(),
            title: issue.title.clone(),
            priority: issue.priority.0 as i64,
            score,
            breakdown,
        });
    }
    scores.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.id.cmp(&b.id)));
    scores
}

struct TriageScoreRow {
    id: String,
    triage_score: f64,
    quick_win_boost: f64,
    breakdown: ScoreBreakdown,
}

fn compute_triage_scores(
    impact: Vec<ImpactScore>,
    graph: &TriageGraph<'_>,
) -> (Vec<TriageScoreRow>, Vec<ImpactScore>) {
    let max_unblocks = graph.unblocks.values().map(Vec::len).max().unwrap_or(0);

    let mut rows: Vec<TriageScoreRow> = Vec::with_capacity(impact.len());
    for base in &impact {
        let unblock_count = graph.unblocks_of(&base.id).len();
        let unblock_boost = if unblock_count == 0 {
            0.0
        } else {
            let denom = max_unblocks.max(UNBLOCK_THRESHOLD);
            ((unblock_count as f64 / denom as f64).min(1.0)) * UNBLOCK_BOOST_WEIGHT
        };

        let issue = graph.by_id.get(base.id.as_str()).copied();
        let not_in_progress = !issue.is_some_and(|issue| issue.status == Status::InProgress);
        let depth = graph.blocker_depth(&base.id);
        let quick_win_boost = if not_in_progress && depth >= 0 && depth <= QUICK_WIN_MAX_DEPTH {
            let depth_factor =
                1.0 - depth as f64 / f64::from(i32::try_from(QUICK_WIN_MAX_DEPTH + 1).unwrap_or(3));
            (depth_factor * base.score * QUICK_WIN_WEIGHT).min(QUICK_WIN_WEIGHT)
        } else {
            0.0
        };

        let triage_score = base.score * BASE_SCORE_WEIGHT + unblock_boost + quick_win_boost;
        rows.push(TriageScoreRow {
            id: base.id.clone(),
            triage_score,
            quick_win_boost,
            breakdown: base.breakdown.clone(),
        });
    }
    rows.sort_by(|a, b| {
        b.triage_score
            .total_cmp(&a.triage_score)
            .then(a.id.cmp(&b.id))
    });
    (rows, impact)
}

// ---------------------------------------------------------------------------
// Reason generation (bv-148; byte-exact strings)
// ---------------------------------------------------------------------------

fn format_unblock_list(ids: &[String]) -> String {
    match ids.len() {
        0 => String::new(),
        1..=3 => ids.join(", "),
        _ => format!("{}, {}, +{} more", ids[0], ids[1], ids.len() - 2),
    }
}

struct TriageReasons {
    action_hint: String,
    all: Vec<String>,
}

#[allow(clippy::too_many_lines)]
fn generate_reasons(
    row: &TriageScoreRow,
    graph: &TriageGraph<'_>,
    now: DateTime<Utc>,
) -> TriageReasons {
    let issue = graph.by_id.get(row.id.as_str()).copied();
    let defer_until = issue.and_then(|issue| issue.defer_until);
    let is_future_deferred = defer_until.is_some_and(|at| at > now);

    let unblocks_ids = graph.unblocks_of(&row.id).to_vec();
    let blocked_by_ids = graph.open_blockers_of(&row.id).to_vec();
    let days_since_update = issue
        .map(|issue| ((now - issue.updated_at).num_hours() as f64 / 24.0) as i64)
        .unwrap_or(0);
    let claimed_by = issue
        .and_then(|issue| issue.assignee.as_deref())
        .filter(|assignee| !assignee.trim().is_empty())
        .map(str::to_owned);
    let is_quick_win = row.quick_win_boost > 0.05;

    let status_text = issue.map(|issue| issue.status.as_str().to_string());
    let is_in_progress = status_text.as_deref() == Some(Status::InProgress.as_str());
    let is_blocked_status = status_text.as_deref() == Some(Status::Blocked.as_str());
    let is_open_status = status_text.as_deref() == Some(Status::Open.as_str());
    let has_recorded_nonempty_status = status_text.as_deref().is_some_and(|s| !s.is_empty());

    let mut reasons: Vec<String> = Vec::new();
    let mut primary = String::new();
    let mut action_hint = "Start work on this issue".to_string();
    if is_future_deferred {
        action_hint = format!(
            "Deferred until {} - do not claim before then",
            go_time_nanos(defer_until.expect("checked above"))
        );
    } else if is_in_progress {
        action_hint = "Continue work on this issue".to_string();
    } else if is_blocked_status {
        action_hint = "Resolve blocked status before claiming this issue".to_string();
    } else if has_recorded_nonempty_status && !is_open_status {
        action_hint = format!(
            "Wait for status {:?} to become open before claiming",
            status_text
        );
    }

    // 1. Unblock cascade.
    if unblocks_ids.len() >= 3 {
        let reason = format!(
            "🎯 Completing this unblocks {} downstream issues ({})",
            unblocks_ids.len(),
            format_unblock_list(&unblocks_ids)
        );
        primary = reason.clone();
        reasons.push(reason);
    } else if !unblocks_ids.is_empty() {
        reasons.push(format!(
            "🔓 Unblocks {} item(s): {}",
            unblocks_ids.len(),
            format_unblock_list(&unblocks_ids)
        ));
    }

    // 3. Graph metrics (betweenness first, then pagerank).
    if row.breakdown.betweenness_norm > 0.5 {
        let reason = format!(
            "🔀 Critical path bottleneck (betweenness: {:.0}%)",
            row.breakdown.betweenness_norm * 100.0
        );
        if primary.is_empty() {
            primary = reason.clone();
        }
        reasons.push(reason);
    }
    if row.breakdown.pagerank_norm > 0.3 {
        reasons.push(format!(
            "📊 High centrality in dependency graph (PageRank: {:.0}%)",
            row.breakdown.pagerank_norm * 100.0
        ));
    }

    // 4. Staleness alert.
    if days_since_update > 14 {
        reasons.push(format!(
            "🕐 No activity in {days_since_update} days - may need review"
        ));
        if is_in_progress {
            action_hint = "Check if this is stuck and needs help".to_string();
        }
    } else if days_since_update > 7 {
        reasons.push(format!("📅 Last updated {days_since_update} days ago"));
        if is_in_progress {
            action_hint = "Continue work on this issue".to_string();
        }
    }

    // 5. Quick-win identification.
    if is_quick_win {
        reasons.push("⚡ Low effort, high impact - good starting point".to_string());
        if primary.is_empty() && !unblocks_ids.is_empty() {
            primary = "⚡ Low effort, high impact - good starting point".to_string();
        }
        let is_critical_stale = is_in_progress && days_since_update > 14;
        if !is_in_progress && !is_critical_stale && !is_future_deferred {
            action_hint = "Quick win - start here for fast progress".to_string();
        }
    }

    // 6. Agent claim status (#149 contract).
    if is_future_deferred {
        reasons.push(format!(
            "⏸️ Deferred until {} - not ready to claim",
            go_time_nanos(defer_until.expect("checked above"))
        ));
        if let Some(claimed_by) = &claimed_by {
            reasons.push(format!("👤 Claimed by {claimed_by}"));
        }
    } else if is_in_progress {
        if let Some(claimed_by) = &claimed_by {
            reasons.push(format!("👤 Claimed by {claimed_by}"));
            action_hint = format!("Contact {claimed_by} if you want to help");
        } else {
            reasons.push("🚧 In progress - already being worked".to_string());
        }
    } else if is_blocked_status {
        reasons.push("⛔ Status is blocked - not ready to claim".to_string());
        if let Some(claimed_by) = &claimed_by {
            reasons.push(format!("👤 Claimed by {claimed_by}"));
            action_hint = format!("Contact {claimed_by} or resolve blockers before claiming");
        }
    } else if has_recorded_nonempty_status && !is_open_status {
        reasons.push(format!(
            "⏸️ Status is {:?} - not ready to claim",
            status_text
        ));
        if let Some(claimed_by) = &claimed_by {
            reasons.push(format!("👤 Claimed by {claimed_by}"));
            action_hint = format!("Contact {claimed_by} if you want to help");
        }
    } else if is_open_status && claimed_by.is_none() {
        reasons.push("✅ Currently unclaimed - available for work".to_string());
    } else if let Some(claimed_by) = &claimed_by {
        reasons.push(format!("👤 Claimed by {claimed_by}"));
        action_hint = format!("Contact {claimed_by} if you want to help");
    }

    // 7. Blocked status context.
    if !blocked_by_ids.is_empty() {
        if blocked_by_ids.len() == 1 {
            reasons.push(format!(
                "⏳ Blocked by {} - complete that first",
                blocked_by_ids[0]
            ));
        } else {
            reasons.push(format!(
                "⏳ Blocked by {} items - need to clear dependencies",
                blocked_by_ids.len()
            ));
        }
        action_hint = format!("Work on {} first to unblock this", blocked_by_ids[0]);
    }

    // 8. Priority context.
    if issue.is_some_and(|issue| issue.priority.0 <= 1) {
        reasons.push(format!(
            "🚨 High priority (P{}) - prioritize this work",
            issue.map_or(4, |issue| issue.priority.0) as i64
        ));
    }

    if primary.is_empty() {
        primary = reasons.first().cloned().unwrap_or_else(|| {
            reasons.push("Good candidate for work".to_string());
            "Good candidate for work".to_string()
        });
    }

    TriageReasons {
        action_hint,
        all: reasons,
    }
}

// ---------------------------------------------------------------------------
// Counts / velocity / health
// ---------------------------------------------------------------------------

fn compute_counts(issues: &[Issue], graph: &TriageGraph<'_>) -> HealthCounts {
    let mut counts = HealthCounts {
        total: issues.len(),
        open: 0,
        closed: 0,
        blocked: 0,
        actionable: 0,
        not_closed: 0,
        dependency_blocked: 0,
        by_status: BTreeMap::new(),
        by_type: BTreeMap::new(),
        by_priority: BTreeMap::new(),
    };
    for issue in issues {
        *counts
            .by_status
            .entry(issue.status.as_str().to_string())
            .or_insert(0) += 1;
        *counts
            .by_type
            .entry(issue.issue_type.as_str().to_string())
            .or_insert(0) += 1;
        *counts
            .by_priority
            .entry(issue.priority.0.to_string())
            .or_insert(0) += 1;
        if is_closed_like(issue) {
            counts.closed += 1;
        } else if graph.open_blockers_of(&issue.id).is_empty() {
            counts.not_closed += 1;
            counts.actionable += 1;
        } else {
            counts.not_closed += 1;
            counts.dependency_blocked += 1;
        }
    }
    counts.open = counts
        .by_status
        .get(Status::Open.as_str())
        .copied()
        .unwrap_or(0);
    counts.blocked = counts
        .by_status
        .get(Status::Blocked.as_str())
        .copied()
        .unwrap_or(0);
    counts
}

fn monday_of_iso_week(t: DateTime<Utc>) -> DateTime<Utc> {
    let days_since_monday = i64::from(t.weekday().num_days_from_monday());
    let midnight = t.date_naive() - chrono::Duration::days(days_since_monday);
    midnight
        .and_hms_opt(0, 0, 0)
        .map(|naive| DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc))
        .expect("midnight is a valid naive datetime")
}

fn compute_velocity(issues: &[Issue], now: DateTime<Utc>) -> Velocity {
    let mut week_buckets: BTreeMap<DateTime<Utc>, usize> = BTreeMap::new();
    let (mut closed_last_7, mut closed_last_30) = (0usize, 0usize);
    let (mut total_close_hours, mut close_samples) = (0.0_f64, 0usize);
    let mut estimated = false;

    let week_ago = now - chrono::Duration::hours(168);
    let month_ago = now - chrono::Duration::hours(720);

    for issue in issues.iter().filter(|issue| issue.status == Status::Closed) {
        let (closed_at, approximated) = match issue.closed_at {
            Some(at) => (at, false),
            None => (issue.updated_at, true),
        };
        estimated |= approximated;

        if closed_at >= week_ago {
            closed_last_7 += 1;
        }
        if closed_at >= month_ago {
            closed_last_30 += 1;
        }
        *week_buckets
            .entry(monday_of_iso_week(closed_at))
            .or_insert(0) += 1;
        total_close_hours += (closed_at - issue.created_at).num_hours() as f64;
        close_samples += 1;
    }

    let mut weekly: Vec<VelocityWeek> = Vec::with_capacity(8);
    let mut cursor = monday_of_iso_week(now);
    for _ in 0..8 {
        weekly.push(VelocityWeek {
            week_start: go_time_secs(cursor),
            closed: week_buckets.get(&cursor).copied().unwrap_or(0),
        });
        cursor -= chrono::Duration::hours(168);
    }

    Velocity {
        closed_last_7_days: closed_last_7,
        closed_last_30_days: closed_last_30,
        avg_days_to_close: if close_samples > 0 {
            total_close_hours / 24.0 / close_samples as f64
        } else {
            0.0
        },
        weekly,
        estimated,
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Engine config behind both robot payloads: PageRank/Betweenness/critical
/// heights on; everything else skipped (the golden `status` shape).
fn triage_analysis_config() -> AnalysisConfig {
    AnalysisConfig {
        compute_pagerank: true,
        pagerank_timeout_ms: METRIC_DEFAULT_TIMEOUT_MS,
        compute_betweenness: true,
        betweenness_mode: BetweennessMode::Exact,
        compute_eigenvector: false,
        compute_hits: false,
        compute_critical_path: true,
        compute_cycles: false,
        max_cycles: 1_000,
        compute_kcore: false,
        compute_articulation: false,
        compute_slack: false,
    }
}

fn skipped_quiet() -> Option<MetricEntry> {
    Some(MetricEntry {
        state: MetricState::Skipped,
        reason: None,
        sample: None,
        ms: None,
    })
}

/// Emission status mirrors bv's triage shape exactly (see module docs for
/// the deliberate quirks).
fn emission_status(result: &super::engine::AnalysisResult) -> MetricStatus {
    let betweenness = result.status.betweenness.as_ref().map(|entry| MetricEntry {
        state: MetricState::Computed,
        reason: Some("approximate".to_string()),
        sample: None,
        ms: entry.ms,
    });
    MetricStatus {
        pagerank: result.status.pagerank.clone(),
        betweenness,
        eigenvector: skipped_quiet(),
        hits: skipped_quiet(),
        critical: skipped_quiet(),
        cycles: skipped_quiet(),
        kcore: skipped_quiet(),
        articulation: skipped_quiet(),
        slack: skipped_quiet(),
    }
}

/// True when PageRank/Betweenness did not finish (bv `ClaimUnsafeReasons`;
/// pending | timeout | panic | error | skipped all block claiming).
pub fn metric_claim_unsafe(status: &MetricStatus) -> bool {
    status
        .pagerank
        .as_ref()
        .is_none_or(MetricEntry::claim_unsafe)
        || status
            .betweenness
            .as_ref()
            .is_none_or(MetricEntry::claim_unsafe)
}

/// Full triage payload over an owned issue set at a pinned instant.
#[allow(clippy::too_many_lines)]
pub fn compute_triage(issues: &[Issue], now: DateTime<Utc>, version: &str) -> TriageResult {
    let started = std::time::Instant::now();
    let graph = TriageGraph::new(issues);

    let engine = AnalysisEngine::new(issues.to_vec());
    let analysis = engine.analyze_on_big_stack(&triage_analysis_config());

    let empty: BTreeMap<String, f64> = BTreeMap::new();
    let pagerank = analysis.pagerank.as_ref().unwrap_or(&empty);
    let betweenness = analysis.betweenness.as_ref().unwrap_or(&empty);
    let critical = analysis.critical_path_score.as_ref().unwrap_or(&empty);
    let phase2_ready = analysis.pagerank.is_some() && analysis.betweenness.is_some();

    let impact = compute_impact_scores(issues, &graph, pagerank, betweenness, critical, now);
    let (scored, impact) = compute_triage_scores(impact, &graph);

    // Recommendations over the FULL scored set (reasons generated once),
    // sliced afterwards — bv #146/#147 ordering contract.
    let all_recommendations: Vec<Recommendation> = scored
        .iter()
        .filter_map(|row| {
            let issue = graph.by_id.get(row.id.as_str()).copied()?;
            let reasons = generate_reasons(row, &graph, now);
            let blocked_by = graph.open_blockers_of(&row.id);
            Some(Recommendation {
                id: row.id.clone(),
                title: issue.title.clone(),
                issue_type: issue.issue_type.as_str().to_string(),
                status: issue.status.as_str().to_string(),
                assignee: issue
                    .assignee
                    .clone()
                    .filter(|assignee| !assignee.trim().is_empty()),
                priority: issue.priority.0 as i64,
                labels: issue.labels.clone(),
                defer_until: issue.defer_until.map(go_time_nanos),
                score: row.triage_score,
                breakdown: row.breakdown.clone(),
                action: reasons.action_hint,
                reasons: reasons.all,
                unblocks_ids: graph.unblocks_of(&row.id).to_vec(),
                blocked_by: blocked_by.to_vec(),
            })
        })
        .collect();
    let visible_recommendations: Vec<Recommendation> = all_recommendations
        .iter()
        .take(RECOMMENDATIONS_LIMIT)
        .cloned()
        .collect();

    // Quick wins ride the base impact ordering (bv buildQuickWins).
    let mut qw_candidates: Vec<(f64, &ImpactScore)> = impact
        .iter()
        .map(|base| {
            let unblocks_len = graph.unblocks_of(&base.id).len();
            let unblock_impact = (unblocks_len as f64 + 1.0).log2();
            let simplicity = if base.breakdown.blocker_ratio_norm < 0.2 {
                1.0
            } else if base.breakdown.blocker_ratio_norm < 0.4 {
                0.5
            } else {
                0.0
            };
            let priority_bonus = if base.priority <= 1 { 0.5 } else { 0.0 };
            (
                unblock_impact * 0.4 + simplicity * 0.4 + priority_bonus * 0.2,
                base,
            )
        })
        .collect();
    qw_candidates.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.id.cmp(&b.1.id)));
    let quick_wins: Vec<QuickWin> = qw_candidates
        .iter()
        .take(QUICK_WINS_LIMIT)
        .map(|(qw_score, base)| {
            let unblocks_len = graph.unblocks_of(&base.id).len();
            let mut reason = if unblocks_len > 0 {
                format!("Unblocks {unblocks_len} items")
            } else {
                "Low complexity".to_string()
            };
            if base.priority <= 1 {
                reason += ", high priority";
            }
            QuickWin {
                id: base.id.clone(),
                title: base.title.clone(),
                score: *qw_score,
                reason,
                unblocks_ids: graph.unblocks_of(&base.id).to_vec(),
            }
        })
        .collect();

    // Blockers to clear: non-closed issuers of non-empty unblock lists.
    let mut blocker_ids: Vec<&String> = graph
        .unblocks
        .iter()
        .filter(|(_, unblocks)| !unblocks.is_empty())
        .map(|(id, _)| id)
        .collect();
    blocker_ids.sort_by(|a, b| {
        let alen = graph.unblocks_of(a).len();
        let blen = graph.unblocks_of(b).len();
        blen.cmp(&alen).then(a.cmp(b))
    });
    let blockers_to_clear: Vec<BlockerItem> = blocker_ids
        .iter()
        .filter_map(|id| graph.by_id.get(id.as_str()).map(|issue| (*id, *issue)))
        .take(BLOCKERS_LIMIT)
        .map(|(id, issue)| {
            let unblocks_ids = graph.unblocks_of(id).to_vec();
            let actionable = graph.open_blockers_of(id).is_empty();
            BlockerItem {
                blocked_by: if actionable {
                    Vec::new()
                } else {
                    graph.open_blockers_of(id).to_vec()
                },
                unblocks_count: unblocks_ids.len(),
                actionable,
                unblocks_ids,
                id: id.clone(),
                title: issue.title.clone(),
            }
        })
        .collect();

    // Top picks walk the FULL recommendation set keeping claimable ones
    // (bv #146: blocked high-priority items must not crowd out picks).
    let top_picks: Vec<TopPick> = all_recommendations
        .iter()
        .filter(|rec| is_claimable_recommendation(rec, &graph, now))
        .take(TOP_PICKS_LIMIT)
        .map(|rec| TopPick {
            unblocks: graph.unblocks_of(&rec.id).len(),
            id: rec.id.clone(),
            title: rec.title.clone(),
            score: rec.score,
            reasons: rec.reasons.clone(),
        })
        .collect();

    let counts = compute_counts(issues, &graph);
    let top_id = top_picks
        .first()
        .map_or(String::new(), |pick| pick.id.clone());

    let meta = TriageMeta {
        version: version.to_string(),
        generated_at: go_time_nanos(now),
        phase2_ready,
        issue_count: issues.len(),
        compute_time_ms: i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX),
    };

    TriageResult {
        meta,
        status: emission_status(&analysis),
        quick_ref: QuickRef {
            open_count: counts.open,
            actionable_count: counts.actionable,
            blocked_count: counts.blocked,
            in_progress_count: counts
                .by_status
                .get(Status::InProgress.as_str())
                .copied()
                .unwrap_or(0),
            not_closed_count: counts.not_closed,
            not_actionable_count: counts.dependency_blocked,
            top_picks,
        },
        recommendations: visible_recommendations,
        quick_wins,
        blockers_to_clear,
        project_health: ProjectHealth {
            graph: GraphHealth {
                node_count: analysis.ids.len(),
                edge_count: analysis.graph.edge_count(),
                density: analysis.density,
                has_cycles: analysis.cycle_count > 0,
                cycle_count: analysis.cycle_count,
                phase2_ready,
            },
            counts,
            velocity: Some(compute_velocity(issues, now)),
        },
        commands: build_commands(&top_id),
    }
}

/// bv's triage-side claimability gate (TriageContext view: dangling deps do
/// not block). `next`'s stricter walk lives in the command layer.
fn is_claimable_recommendation(
    rec: &Recommendation,
    graph: &TriageGraph<'_>,
    now: DateTime<Utc>,
) -> bool {
    if graph.parent_has_open_children(&rec.id) {
        return false;
    }
    rec.status == Status::Open.as_str()
        && rec.issue_type != IssueType::Epic.as_str()
        && rec
            .assignee
            .as_deref()
            .map(str::trim)
            .is_none_or(str::is_empty)
        && rec.blocked_by.is_empty()
        && !graph
            .by_id
            .get(rec.id.as_str())
            .and_then(|issue| issue.defer_until)
            .is_some_and(|at| at > now)
}

fn build_commands(top_id: &str) -> CommandHelpers {
    const BASE: &str = "CI=1 ";
    let list_ready = format!("{BASE}br ready --json");
    let list_blocked = format!("{BASE}br blocked --json");
    let (claim_top, show_top) = if top_id.is_empty() {
        (
            format!("{list_ready}  # No top pick available"),
            format!("{list_ready}  # No top pick available"),
        )
    } else {
        (
            format!("{BASE}br update {top_id} --status in_progress --json"),
            format!("{BASE}br show {top_id} --json"),
        )
    };
    CommandHelpers {
        claim_top,
        show_top,
        list_ready,
        list_blocked,
        refresh_triage: "bv --robot-triage".to_string(),
    }
}

/// `skip_serializing_if` helper for numeric fields that vanish at zero
/// (Go's `omitempty` on int fields).
fn is_zero(value: &usize) -> bool {
    *value == 0
}
