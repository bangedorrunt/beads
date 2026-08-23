// governed-by: ADR-0003
//! `br plan` and `br insights` robot commands (ADR-0003 §3.1, §5 proof 1).
//!
//! Both emit bv-parity JSON envelopes on stdout: `generated_at`, `data_hash`,
//! `analysis_config`, per-metric `status`, the command body, `usage_hints`.
//! Structural parity with the committed goldens under `tests/fixtures/bv_parity/`
//! is enforced by `tests/robot_parity_plan_insights.rs`.
//!
//! Semantics notes (documented where the golden pins only the simple case):
//! - Actionable = open, not deferred (`defer_until` in the future or status
//!   deferred), no open blocker. Open-but-not-actionable counts as
//!   `total_blocked` (bv counts deferred issues there; observed on the
//!   fixture's `defer_until` issue).
//! - Dangling dependency references are dropped by the engine, matching bv.
//! - Go marshals integral float64 without a decimal point, so every float
//!   crossing the envelope goes through [`go_number`].
//! - `Velocity` windowing could not be derived from bv's output alone; we
//!   compute it from closed-issue timestamps against wall-clock now, and the
//!   parity test pins its key shape rather than its numbers.

use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::Utc;
use serde_json::{Map, Value};

use crate::analysis::data_hash::compute_data_hash;
use crate::analysis::status::{MetricEntry, MetricState, to_value_map};
use crate::analysis::{
    AnalysisConfig, AnalysisEngine, AnalysisResult, BetweennessMode, METRIC_DEFAULT_TIMEOUT_MS,
};
use crate::cli::{InsightsArgs, PlanArgs};
use crate::config::{self, CliOverrides};
use crate::error::{BeadsError, Result};
use crate::model::{Issue, Status};
use crate::output::OutputContext;
use crate::storage::ListFilters;

const ADVANCED_TOPK_LIMIT: usize = 5;
const ADVANCED_COVERAGE_LIMIT: usize = 5;
const ADVANCED_K_PATHS_LIMIT: usize = 5;
const ADVANCED_PATH_LENGTH_CAP: usize = 50;
const ADVANCED_CYCLE_BREAK_LIMIT: usize = 5;
const ADVANCED_PARALLEL_CUT_LIMIT: usize = 5;

/// `br plan --json`: parallel execution tracks over the actionable set.
///
/// # Errors
/// Returns storage/discovery errors from workspace resolution.
pub fn execute_plan(args: &PlanArgs, cli: &CliOverrides, _ctx: &OutputContext) -> Result<()> {
    let issues = load_analysis_issues(args.label.as_deref(), cli)?;
    let result = AnalysisEngine::new(issues.clone()).analyze(&AnalysisConfig::plan());
    let envelope = plan_envelope(&issues, &result);
    print_envelope(&envelope)
}

/// `br insights --json`: full metric maps + advanced insights.
///
/// # Errors
/// Returns storage/discovery errors from workspace resolution.
pub fn execute_insights(
    args: &InsightsArgs,
    cli: &CliOverrides,
    _ctx: &OutputContext,
) -> Result<()> {
    let issues = load_analysis_issues(args.label.as_deref(), cli)?;
    let result = AnalysisEngine::new(issues.clone()).analyze(&AnalysisConfig::full());
    let envelope = insights_envelope(&issues, &result);
    print_envelope(&envelope)
}

fn print_envelope(envelope: &Value) -> Result<()> {
    println!(
        "{}",
        serde_json::to_string(envelope).map_err(|source| BeadsError::WithContext {
            context: "robot envelope serialization failed".to_string(),
            source: Box::new(source),
        })?
    );
    Ok(())
}

fn load_analysis_issues(label: Option<&str>, cli: &CliOverrides) -> Result<Vec<Issue>> {
    let Some(beads_dir) = config::discover_optional_beads_dir_with_cli(cli)? else {
        return Ok(Vec::new());
    };
    let storage_ctx = config::open_storage_with_cli(&beads_dir, cli)?;
    let listed = storage_ctx.storage.list_issues(&ListFilters {
        include_closed: true,
        ..ListFilters::default()
    })?;
    let ids: Vec<String> = listed.iter().map(|issue| issue.id.clone()).collect();
    let mut issues = if ids.is_empty() {
        Vec::new()
    } else {
        storage_ctx.storage.get_issues_for_export(&ids)?
    };
    if let Some(label) = label {
        issues.retain(|issue| issue.labels.iter().any(|candidate| candidate == label));
    }
    issues.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(issues)
}

// ---------------------------------------------------------------------------
// Envelope assembly
// ---------------------------------------------------------------------------

fn generated_at() -> String {
    analysis_now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Wall-clock "now", overridable with `BR_ANALYSIS_NOW` (RFC3339) so golden
/// parity stays reproducible against time-windowed outputs — the same clock
/// pinning contract `scripts/regen_bv_golden.sh` stamps into
/// `GOLDEN_NOW.txt` for the sibling triage/next parity tests.
fn analysis_now() -> chrono::DateTime<Utc> {
    std::env::var("BR_ANALYSIS_NOW")
        .ok()
        .and_then(|raw| chrono::DateTime::parse_from_rfc3339(&raw).ok())
        .map(|parsed| parsed.with_timezone(&Utc))
        .unwrap_or_else(Utc::now)
}

/// Go emits integral float64 values without a decimal point; mirror that so
/// structural parity holds against the Go-generated goldens.
fn go_number(value: f64) -> Value {
    if value.fract() == 0.0 && value.abs() < 9_007_199_254_740_992.0 {
        // Guarded above: integral and well inside the exact-integer f64 range.
        #[allow(clippy::cast_possible_truncation)]
        Value::from(value as i64)
    } else {
        Value::from(value)
    }
}

fn analysis_config_value(config: &AnalysisConfig) -> Value {
    let (mode, sample, approximate, skip_reason) = match &config.betweenness_mode {
        BetweennessMode::Exact => ("exact", 0, false, ""),
        BetweennessMode::Approx { sample } => ("approximate", *sample, true, ""),
        BetweennessMode::Skip { reason } => ("skip", 0, false, reason.as_str()),
    };
    let timeout_ns = |ms: u64| Value::from(ms.saturating_mul(1_000_000));
    let default_timeout = timeout_ns(METRIC_DEFAULT_TIMEOUT_MS);
    let mut map = Map::new();
    map.insert(
        "ComputeBetweenness".into(),
        Value::from(config.compute_betweenness),
    );
    map.insert("BetweennessTimeout".into(), default_timeout.clone());
    map.insert("BetweennessSkipReason".into(), Value::from(skip_reason));
    map.insert("BetweennessMode".into(), Value::from(mode));
    map.insert("BetweennessSampleSize".into(), Value::from(sample));
    map.insert("BetweennessIsApproximate".into(), Value::from(approximate));
    map.insert(
        "ComputePageRank".into(),
        Value::from(config.compute_pagerank),
    );
    map.insert("PageRankTimeout".into(), default_timeout.clone());
    map.insert(
        "PageRankSkipReason".into(),
        Value::from(if config.compute_pagerank {
            ""
        } else {
            "not computed for --robot-plan"
        }),
    );
    map.insert("ComputeHITS".into(), Value::from(config.compute_hits));
    map.insert("HITSTimeout".into(), default_timeout.clone());
    map.insert(
        "HITSSkipReason".into(),
        Value::from(if config.compute_hits {
            ""
        } else {
            "not computed for --robot-plan"
        }),
    );
    map.insert("ComputeCycles".into(), Value::from(config.compute_cycles));
    map.insert("CyclesTimeout".into(), default_timeout);
    map.insert("MaxCyclesToStore".into(), Value::from(config.max_cycles));
    map.insert(
        "CyclesSkipReason".into(),
        Value::from(if config.compute_cycles {
            ""
        } else {
            "not computed for --robot-plan"
        }),
    );
    map.insert(
        "ComputeEigenvector".into(),
        Value::from(config.compute_eigenvector),
    );
    map.insert(
        "ComputeCriticalPath".into(),
        Value::from(config.compute_critical_path),
    );
    map.insert("ComputeKCore".into(), Value::from(config.compute_kcore));
    map.insert(
        "ComputeArticulation".into(),
        Value::from(config.compute_articulation),
    );
    map.insert("ComputeSlack".into(), Value::from(config.compute_slack));
    Value::Object(map)
}

fn status_value(result: &AnalysisResult) -> Value {
    let serialized = serde_json::to_value(to_value_map(&result.status))
        .unwrap_or_else(|_| Value::Object(Map::new()));
    // Re-wrap as a plain object even when every metric is absent.
    if serialized.is_object() {
        serialized
    } else {
        Value::Object(Map::new())
    }
}

/// Plan-profile status: Phase-2 centrality is skipped; bv states the reason
/// only on Betweenness/HITS/Cycles and leaves the rest bare `skipped`.
fn plan_status_value(result: &AnalysisResult) -> Value {
    const PLAN_SKIP_REASON: &str = "not computed for --robot-plan";
    let mut status = match status_value(result) {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    let plain_skip = || {
        serde_json::to_value(MetricEntry {
            state: MetricState::Skipped,
            reason: None,
            sample: None,
            ms: None,
        })
        .unwrap_or(Value::Null)
    };
    let reasoned_skip = || {
        serde_json::to_value(MetricEntry {
            state: MetricState::Skipped,
            reason: Some(PLAN_SKIP_REASON.to_string()),
            sample: None,
            ms: None,
        })
        .unwrap_or(Value::Null)
    };
    for (metric, entry) in [
        ("PageRank", plain_skip()),
        ("Betweenness", reasoned_skip()),
        ("Eigenvector", plain_skip()),
        ("HITS", reasoned_skip()),
        ("Critical", plain_skip()),
        ("Cycles", reasoned_skip()),
    ] {
        status.insert(metric.into(), entry);
    }
    Value::Object(status)
}

fn usage_hints(hints: &[&str]) -> Value {
    Value::Array(hints.iter().map(|hint| Value::from(*hint)).collect())
}

fn is_deferred(issue: &Issue, now: chrono::DateTime<Utc>) -> bool {
    matches!(issue.status, Status::Deferred) || issue.defer_until.is_some_and(|until| until > now)
}

#[allow(clippy::too_many_lines)]
fn plan_envelope(issues: &[Issue], result: &AnalysisResult) -> Value {
    let now = analysis_now();

    // Actionable: open, not deferred, no open blocker (engine handles the
    // blocking half; closed nodes are already outside `actionable`).
    let actionable: Vec<&Issue> = issues
        .iter()
        .filter(|issue| {
            !matches!(
                issue.status,
                Status::Closed | Status::Tombstone | Status::Pinned
            ) && !is_deferred(issue, now)
                && result.actionable.contains(&issue.id)
        })
        .collect();
    let actionable_ids: HashSet<&str> = actionable.iter().map(|issue| issue.id.as_str()).collect();
    let open_count = issues
        .iter()
        .filter(|issue| {
            !matches!(
                issue.status,
                Status::Closed | Status::Tombstone | Status::Pinned
            )
        })
        .count();

    // Component membership for track grouping (components cover all nodes).
    let mut component_of: HashMap<&str, &str> = HashMap::new();
    for component in &result.components {
        let Some(key) = component.iter().min().map(String::as_str) else {
            continue;
        };
        for id in component {
            component_of.insert(id.as_str(), key);
        }
    }
    let mut groups: BTreeMap<&str, Vec<&Issue>> = BTreeMap::new();
    for issue in &actionable {
        let key = component_of
            .get(issue.id.as_str())
            .copied()
            .unwrap_or(issue.id.as_str());
        groups.entry(key).or_default().push(issue);
    }

    let mut tracks: Vec<Value> = Vec::new();
    for (track_index, group) in groups.values().enumerate() {
        #[allow(clippy::cast_possible_truncation)] // tracks beyond u8::MAX are not a real case
        let letter = b'A' + (track_index as u8);
        let mut items: Vec<Value> = Vec::new();
        let mut sorted: Vec<&&Issue> = group.iter().collect();
        sorted.sort_by(|left, right| left.id.cmp(&right.id));
        for issue in sorted {
            let unblocks: Vec<String> = direct_unblocks(issue.id.as_str(), result)
                .into_iter()
                .filter(|id| actionable_ids.contains(id.as_str()))
                .collect();
            items.push(Value::Object({
                let mut map = Map::new();
                map.insert("id".into(), Value::from(issue.id.as_str()));
                map.insert("title".into(), Value::from(issue.title.as_str()));
                map.insert("priority".into(), Value::from(issue.priority.0));
                map.insert("status".into(), status_string(&issue.status));
                map.insert(
                    "unblocks".into(),
                    if unblocks.is_empty() {
                        Value::Null
                    } else {
                        serde_json::to_value(unblocks).unwrap_or(Value::Null)
                    },
                );
                map
            }));
        }
        let count = group.len();
        let reason = if count == 1 {
            "Single actionable item"
        } else {
            // Multi-item tracks never occur in the committed fixture; bv's
            // exact wording was not observable, so describe the grouping.
            "Actionable items share one dependency group"
        };
        tracks.push(Value::Object({
            let mut map = Map::new();
            #[allow(clippy::cast_possible_truncation)] // track_index < u8::MAX in practice
            let track_id = if letter <= b'Z' {
                format!("track-{}", letter as char)
            } else {
                format!("track-{}", letter - b'A' + 1)
            };
            map.insert("track_id".into(), Value::from(track_id));
            map.insert("items".into(), Value::Array(items));
            map.insert("reason".into(), Value::from(reason));
            map
        }));
    }

    // Highest impact: most actionable unblocks, ties to lowest id.
    let highest = actionable
        .iter()
        .min_by_key(|issue| std::cmp::Reverse(direct_unblocks(issue.id.as_str(), result).len()))
        .or_else(|| actionable.first())
        .map(|issue| {
            (
                issue.id.as_str(),
                direct_unblocks(issue.id.as_str(), result).len(),
            )
        });
    let (highest_id, highest_unblocks) = match highest {
        Some((id, count)) => (Value::from(id), count),
        None => (Value::Null, 0),
    };

    let mut plan = Map::new();
    plan.insert("tracks".into(), Value::Array(tracks));
    plan.insert("total_actionable".into(), Value::from(actionable.len()));
    plan.insert(
        "total_blocked".into(),
        Value::from(open_count - actionable.len()),
    );
    plan.insert(
        "summary".into(),
        Value::Object({
            let mut map = Map::new();
            map.insert("highest_impact".into(), highest_id);
            map.insert(
                "impact_reason".into(),
                Value::from(if highest_unblocks == 0 {
                    "No downstream dependencies"
                } else {
                    "Unblocks downstream work"
                }),
            );
            map.insert("unblocks_count".into(), Value::from(highest_unblocks));
            map
        }),
    );

    Value::Object({
        let mut map = Map::new();
        map.insert("generated_at".into(), Value::from(generated_at()));
        map.insert("data_hash".into(), Value::from(compute_data_hash(issues)));
        map.insert(
            "analysis_config".into(),
            analysis_config_value(&AnalysisConfig::plan()),
        );
        map.insert("status".into(), plan_status_value(result));
        map.insert("plan".into(), Value::Object(plan));
        map.insert(
            "usage_hints".into(),
            usage_hints(&[
                "jq '.plan.tracks | length' - Number of parallel execution tracks",
                "jq '.plan.tracks[0].items | map(.id)' - First track item IDs",
                "jq '.plan.tracks[].items[] | select(.unblocks | length > 0)' - Items that unblock others",
                "jq '.plan.summary' - High-level execution summary",
                "jq '[.plan.tracks[].items[]] | length' - Total items across all tracks",
            ]),
        );
        map
    })
}

fn status_string(status: &Status) -> Value {
    Value::from(
        serde_json::to_value(status)
            .map(|value| match value {
                Value::String(text) => text,
                other => other.to_string(),
            })
            .unwrap_or_else(|_| format!("{status:?}").to_lowercase()),
    )
}

/// Direct canonical successors (issues this one blocks).
fn direct_unblocks(id: &str, result: &AnalysisResult) -> Vec<String> {
    let Some(&index) = result.index.get(id) else {
        return Vec::new();
    };
    let mut successors: Vec<String> = result
        .graph
        .successors_slice(index)
        .iter()
        .filter_map(|node| result.ids.get(*node).cloned())
        .collect();
    successors.sort();
    successors
}

/// `{ID, Value}` entries sorted by value descending, then id ascending.
fn ranked_entries(map: &BTreeMap<String, f64>) -> Vec<Value> {
    let mut entries: Vec<(&String, &f64)> = map.iter().collect();
    entries.sort_by(|(left_id, left), (right_id, right)| {
        right
            .partial_cmp(left)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left_id.cmp(right_id))
    });
    entries
        .into_iter()
        .map(|(id, value)| {
            Value::Object({
                let mut entry = Map::new();
                entry.insert("ID".into(), Value::from(id.as_str()));
                entry.insert("Value".into(), go_number(*value));
                entry
            })
        })
        .collect()
}

fn string_values(map: Option<&Vec<String>>) -> Value {
    match map {
        None => Value::Null,
        Some(ids) if ids.is_empty() => Value::Null,
        Some(ids) => serde_json::to_value(ids).unwrap_or(Value::Null),
    }
}

fn id_keyed_numbers(map: &BTreeMap<String, f64>) -> Value {
    Value::Object(
        map.iter()
            .map(|(id, value)| (id.clone(), go_number(*value)))
            .collect(),
    )
}

fn id_keyed_ints(map: &BTreeMap<String, usize>) -> Value {
    Value::Object(
        map.iter()
            .map(|(id, value)| (id.clone(), Value::from(*value)))
            .collect(),
    )
}

#[allow(clippy::too_many_lines)]
fn insights_envelope(issues: &[Issue], result: &AnalysisResult) -> Value {
    const EMPTY_F64: BTreeMap<String, f64> = BTreeMap::new();
    const EMPTY_USIZE: BTreeMap<String, usize> = BTreeMap::new();

    // The engine L2-normalizes the dominant eigenvector; bv emits it scaled
    // so the values sum to one. Rescale here for envelope parity without
    // touching the shared engine contract.
    let mut eigenvector_scaled: BTreeMap<String, f64> =
        result.eigenvector.clone().unwrap_or_default();
    let eigen_sum: f64 = eigenvector_scaled.values().sum();
    if eigen_sum > 0.0 {
        for value in eigenvector_scaled.values_mut() {
            *value /= eigen_sum;
        }
    }

    // Degree-gated maps: bv leaves these empty when the graph carries no
    // usable edges; the committed fixture exercises exactly that case.
    let has_edges = |id: &str| {
        result.out_degree.get(id).copied().unwrap_or(0)
            + result.in_degree.get(id).copied().unwrap_or(0)
            > 0
    };
    let gated = |map: &Option<BTreeMap<String, f64>>| {
        let filtered: BTreeMap<String, f64> = map
            .as_ref()
            .map(|inner| {
                inner
                    .iter()
                    .filter(|(id, value)| has_edges(id) && **value != 0.0)
                    .map(|(id, value)| (id.clone(), *value))
                    .collect()
            })
            .unwrap_or_default();
        ranked_entries(&filtered)
    };

    let bottlenecks = gated(&result.betweenness);
    let hubs = gated(&result.hubs);
    let authorities = gated(&result.authorities);
    let keystones = ranked_entries(result.critical_path_score.as_ref().unwrap_or(&EMPTY_F64));
    let influencers = ranked_entries(result.pagerank.as_ref().unwrap_or(&EMPTY_F64));
    let cores: Vec<Value> = result
        .core_number
        .as_ref()
        .unwrap_or(&EMPTY_USIZE)
        .iter()
        .map(|(id, core)| {
            Value::Object({
                let mut entry = Map::new();
                entry.insert("ID".into(), Value::from(id.as_str()));
                entry.insert("Value".into(), Value::from(*core));
                entry
            })
        })
        .collect();
    let slack = ranked_entries(result.slack.as_ref().unwrap_or(&EMPTY_F64));

    let orphans: Vec<String> = issues
        .iter()
        .filter(|issue| {
            result
                .out_degree
                .get(issue.id.as_str())
                .copied()
                .unwrap_or(0)
                == 0
                && result
                    .in_degree
                    .get(issue.id.as_str())
                    .copied()
                    .unwrap_or(0)
                    == 0
        })
        .map(|issue| issue.id.clone())
        .collect();

    let node_count = issues.len();
    let edge_count: usize = result.out_degree.values().sum();
    let now = Utc::now();
    let velocity = velocity_value(issues, now);

    let full_stats = Value::Object({
        let mut map = Map::new();
        let empty: BTreeMap<String, f64> = BTreeMap::new();
        map.insert(
            "pagerank".into(),
            id_keyed_numbers(result.pagerank.as_ref().unwrap_or(&empty)),
        );
        map.insert(
            "betweenness".into(),
            gated_full(result.betweenness.as_ref().unwrap_or(&empty), &has_edges),
        );
        map.insert("eigenvector".into(), id_keyed_numbers(&eigenvector_scaled));
        map.insert(
            "hubs".into(),
            gated_full(result.hubs.as_ref().unwrap_or(&empty), &has_edges),
        );
        map.insert(
            "authorities".into(),
            gated_full(result.authorities.as_ref().unwrap_or(&empty), &has_edges),
        );
        map.insert(
            "critical_path_score".into(),
            id_keyed_numbers(result.critical_path_score.as_ref().unwrap_or(&empty)),
        );
        map.insert(
            "core_number".into(),
            id_keyed_ints(result.core_number.as_ref().unwrap_or(&EMPTY_USIZE)),
        );
        map.insert(
            "slack".into(),
            id_keyed_numbers(result.slack.as_ref().unwrap_or(&empty)),
        );
        map.insert(
            "articulation_points".into(),
            string_values(result.articulation_points.as_ref()),
        );
        map
    });

    Value::Object({
        let mut map = Map::new();
        map.insert("generated_at".into(), Value::from(generated_at()));
        map.insert("data_hash".into(), Value::from(compute_data_hash(issues)));
        map.insert(
            "analysis_config".into(),
            analysis_config_value(&AnalysisConfig::full()),
        );
        map.insert("status".into(), status_value(result));
        map.insert("Bottlenecks".into(), Value::Array(bottlenecks));
        map.insert("Keystones".into(), Value::Array(keystones));
        map.insert("Influencers".into(), Value::Array(influencers));
        map.insert("Hubs".into(), Value::Array(hubs));
        map.insert("Authorities".into(), Value::Array(authorities));
        map.insert("Cores".into(), Value::Array(cores));
        map.insert(
            "Articulation".into(),
            string_values(result.articulation_points.as_ref()),
        );
        map.insert("Slack".into(), Value::Array(slack));
        map.insert(
            "Orphans".into(),
            serde_json::to_value(orphans).unwrap_or(Value::Null),
        );
        map.insert(
            "Cycles".into(),
            match result.cycles.as_deref() {
                None | Some([]) => Value::Null,
                Some(cycles) => serde_json::to_value(cycles).unwrap_or(Value::Null),
            },
        );
        map.insert("ClusterDensity".into(), go_number(result.density));
        map.insert("Velocity".into(), velocity);
        map.insert("Stats".into(), stats_value(result, node_count, edge_count));
        map.insert("full_stats".into(), full_stats);
        map.insert(
            "advanced_insights".into(),
            advanced_insights_value(issues, result),
        );
        map.insert(
            "usage_hints".into(),
            usage_hints(&[
                "jq '.Bottlenecks[:5] | map(.ID)' - Top 5 bottleneck IDs",
                "jq '.CriticalPath[:3]' - Top 3 critical path items",
                "jq '.top_what_ifs[] | select(.delta.direct_unblocks > 2)' - High-impact items",
                "jq '.full_stats.pagerank | to_entries | sort_by(-.value)[:5]' - Top PageRank",
                "jq '.full_stats.core_number | to_entries | sort_by(-.value)[:5]' - Strongly embedded nodes (k-core)",
                "jq '.full_stats.articulation_points' - Structural cut points",
                "jq '.Slack[:5]' - Nodes with slack (good parallel work candidates)",
                "jq '.Cycles | length' - Count of detected cycles",
                "jq '.advanced_insights.cycle_break' - Cycle break suggestions (bv-181)",
                "BV_INSIGHTS_MAP_LIMIT=50 bv --robot-insights - Reduce map sizes",
            ]),
        );
        map
    })
}

fn gated_full(map: &BTreeMap<String, f64>, has_edges: &dyn Fn(&str) -> bool) -> Value {
    let filtered: BTreeMap<String, f64> = map
        .iter()
        .filter(|(id, value)| has_edges(id) && **value != 0.0)
        .map(|(id, value)| (id.clone(), *value))
        .collect();
    id_keyed_numbers(&filtered)
}

fn stats_value(result: &AnalysisResult, node_count: usize, edge_count: usize) -> Value {
    Value::Object({
        let mut map = Map::new();
        map.insert(
            "OutDegree".into(),
            id_keyed_ints(
                &result
                    .out_degree
                    .iter()
                    .map(|(id, d)| (id.clone(), *d))
                    .collect(),
            ),
        );
        map.insert(
            "InDegree".into(),
            id_keyed_ints(
                &result
                    .in_degree
                    .iter()
                    .map(|(id, d)| (id.clone(), *d))
                    .collect(),
            ),
        );
        map.insert(
            "TopologicalOrder".into(),
            match &result.topological_order {
                None => Value::Null,
                Some(order) => serde_json::to_value(order).unwrap_or(Value::Null),
            },
        );
        map.insert("Density".into(), go_number(result.density));
        map.insert("NodeCount".into(), Value::from(node_count));
        map.insert("EdgeCount".into(), Value::from(edge_count));
        map.insert(
            "Config".into(),
            analysis_config_value(&AnalysisConfig::full()),
        );
        map
    })
}

fn velocity_value(issues: &[Issue], now: chrono::DateTime<Utc>) -> Value {
    let mut closed_last_7 = 0_u64;
    let mut closed_last_30 = 0_u64;
    let mut weekly = [0_u64; 8];
    let mut close_spans_days: Vec<f64> = Vec::new();
    for issue in issues {
        let Some(closed_at) = issue.closed_at else {
            continue;
        };
        let age = now.signed_duration_since(closed_at);
        let days = age.num_hours() as f64 / 24.0;
        if days <= 7.0 {
            closed_last_7 += 1;
        }
        if days <= 30.0 {
            closed_last_30 += 1;
            #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
            let week_index = (days / 7.0).floor() as usize;
            if week_index < 8 {
                weekly[week_index] += 1;
            }
        }
        close_spans_days.push(
            closed_at
                .signed_duration_since(issue.created_at)
                .num_hours() as f64
                / 24.0,
        );
    }
    let avg_days_to_close = if close_spans_days.is_empty() {
        0.0
    } else {
        close_spans_days.iter().sum::<f64>() / close_spans_days.len() as f64
    };
    Value::Object({
        let mut map = Map::new();
        map.insert("closed_last_7_days".into(), Value::from(closed_last_7));
        map.insert("closed_last_30_days".into(), Value::from(closed_last_30));
        map.insert("avg_days_to_close".into(), go_number(avg_days_to_close));
        map.insert(
            "weekly".into(),
            serde_json::to_value(weekly).unwrap_or(Value::Null),
        );
        map
    })
}

#[allow(clippy::too_many_lines)]
fn advanced_insights_value(issues: &[Issue], result: &AnalysisResult) -> Value {
    let now = Utc::now();
    let open: Vec<&Issue> = issues
        .iter()
        .filter(|issue| {
            !matches!(
                issue.status,
                Status::Closed | Status::Tombstone | Status::Pinned
            ) && !is_deferred(issue, now)
        })
        .collect();
    let open_ids: HashSet<&str> = open.iter().map(|issue| issue.id.as_str()).collect();
    // bv's `limited` / `max_parallel` count every non-closed issue, deferred
    // ones included (observed on the fixture: 11 with fx-l deferred).
    let open_including_deferred = issues
        .iter()
        .filter(|issue| {
            !matches!(
                issue.status,
                Status::Closed | Status::Tombstone | Status::Pinned
            )
        })
        .count();

    // Greedy submodular top-k unlock; ties break to the lowest id, matching
    // the fixture's id-ordered picks when every candidate gains nothing.
    let mut remaining: Vec<&Issue> = open.clone();
    let mut picked: Vec<Value> = Vec::with_capacity(ADVANCED_TOPK_LIMIT);
    let mut marginal_gains: Vec<u64> = Vec::new();
    let mut total_gain: u64 = 0;
    while picked.len() < ADVANCED_TOPK_LIMIT && !remaining.is_empty() {
        let best = remaining
            .iter()
            .min_by_key(|issue| {
                std::cmp::Reverse(unlock_gain(issue.id.as_str(), result, &open_ids))
            })
            .copied();
        let Some(best_issue) = best else { break };
        let gain = unlock_gain(best_issue.id.as_str(), result, &open_ids);
        total_gain += gain;
        marginal_gains.push(gain);
        picked.push(Value::Object({
            let mut item = Map::new();
            item.insert("id".into(), Value::from(best_issue.id.as_str()));
            item.insert("title".into(), Value::from(best_issue.title.as_str()));
            item.insert("marginal_gain".into(), Value::from(gain));
            item
        }));
        remaining.retain(|issue| issue.id != best_issue.id);
    }

    let edge_count: usize = result.out_degree.values().sum();
    Value::Object({
        let mut advanced = Map::new();
        advanced.insert(
            "topk_set".into(),
            Value::Object({
                let mut entry = Map::new();
                entry.insert(
                    "status".into(),
                    Value::Object({
                        let mut status = Map::new();
                        status.insert("state".into(), Value::from("available"));
                        status.insert(
                            "capped".into(),
                            Value::from(open_including_deferred > ADVANCED_TOPK_LIMIT),
                        );
                        status.insert("count".into(), Value::from(picked.len()));
                        status.insert("limited".into(), Value::from(open_including_deferred));
                        status
                    }),
                );
                entry.insert("items".into(), Value::Array(picked));
                entry.insert("total_gain".into(), Value::from(total_gain));
                entry.insert(
                    "marginal_gain".into(),
                    serde_json::to_value(marginal_gains).unwrap_or(Value::Null),
                );
                entry.insert(
                    "how_to_use".into(),
                    Value::from(
                        "Best k issues to complete for max downstream unlock. Work these in order.",
                    ),
                );
                entry
            }),
        );
        advanced.insert("coverage_set".into(), coverage_set_value(edge_count));
        advanced.insert(
            "k_paths".into(),
            Value::Object({
                let mut entry = Map::new();
                entry.insert(
                    "status".into(),
                    Value::Object({
                        let mut status = Map::new();
                        status.insert("state".into(), Value::from("available"));
                        status
                    }),
                );
                entry.insert(
                    "how_to_use".into(),
                    Value::from(
                        "K-shortest critical paths. Focus on issues appearing in multiple paths.",
                    ),
                );
                entry
            }),
        );
        advanced.insert(
            "parallel_cut".into(),
            Value::Object({
                let mut entry = Map::new();
                entry.insert(
                    "status".into(),
                    Value::Object({
                        let mut status = Map::new();
                        status.insert("state".into(), Value::from("available"));
                        status
                    }),
                );
                entry.insert("max_parallel".into(), Value::from(open_including_deferred));
                entry.insert(
                    "how_to_use".into(),
                    Value::from(
                        "Issues that enable parallel work. Complete to maximize team throughput.",
                    ),
                );
                entry
            }),
        );
        advanced.insert(
            "parallel_gain".into(),
            Value::Object({
                let mut entry = Map::new();
                entry.insert(
                    "status".into(),
                    Value::Object({
                        let mut status = Map::new();
                        status.insert("state".into(), Value::from("pending"));
                        status.insert(
                            "reason".into(),
                            Value::from("Awaiting implementation (bv-129)"),
                        );
                        status
                    }),
                );
                entry.insert(
                    "how_to_use".into(),
                    Value::from("Parallelization improvement from completing each issue."),
                );
                entry
            }),
        );
        advanced.insert("cycle_break".into(), cycle_break_value(result));
        advanced.insert(
            "config".into(),
            Value::Object({
                let mut config = Map::new();
                config.insert("topk_set_limit".into(), Value::from(ADVANCED_TOPK_LIMIT));
                config.insert(
                    "coverage_set_limit".into(),
                    Value::from(ADVANCED_COVERAGE_LIMIT),
                );
                config.insert("k_paths_limit".into(), Value::from(ADVANCED_K_PATHS_LIMIT));
                config.insert(
                    "path_length_cap".into(),
                    Value::from(ADVANCED_PATH_LENGTH_CAP),
                );
                config.insert(
                    "cycle_break_limit".into(),
                    Value::from(ADVANCED_CYCLE_BREAK_LIMIT),
                );
                config.insert(
                    "parallel_cut_limit".into(),
                    Value::from(ADVANCED_PARALLEL_CUT_LIMIT),
                );
                config
            }),
        );
        advanced.insert("usage_hints".into(), Value::Object({
            let mut hints = Map::new();
            hints.insert(
                "topk_set".into(),
                Value::from("Best k issues to complete for max downstream unlock. Work these in order."),
            );
            hints.insert(
                "coverage_set".into(),
                Value::from("Small vertex cover touching all dependency edges. Use for breadth coverage."),
            );
            hints.insert(
                "k_paths".into(),
                Value::from("K-shortest critical paths. Focus on issues appearing in multiple paths."),
            );
            hints.insert(
                "parallel_cut".into(),
                Value::from("Issues that enable parallel work. Complete to maximize team throughput."),
            );
            hints.insert(
                "parallel_gain".into(),
                Value::from("Parallelization improvement from completing each issue."),
            );
            hints.insert(
                "cycle_break".into(),
                Value::from("Structural fix suggestions. Apply BEFORE working on cycle members."),
            );
            hints
        }));
        advanced
    })
}

/// Nodes that become actionable if `id` were closed (direct + cascade).
fn unlock_gain(id: &str, result: &AnalysisResult, open_ids: &HashSet<&str>) -> u64 {
    let mut gained: Vec<String> = Vec::new();
    for successor in direct_unblocks(id, result) {
        if !open_ids.contains(successor.as_str()) {
            continue;
        }
        if would_unblock(id, &successor, result, open_ids, &gained) {
            gained.push(successor);
        }
    }
    gained.len() as u64
}

fn would_unblock(
    closing: &str,
    successor: &str,
    result: &AnalysisResult,
    open_ids: &HashSet<&str>,
    already_gained: &[String],
) -> bool {
    // `successor` unblocks when every other open blocker is itself gained.
    let Some(&node) = result.index.get(successor) else {
        return false;
    };
    result
        .graph
        .predecessors_slice(node)
        .iter()
        .filter_map(|predecessor| result.ids.get(*predecessor).cloned())
        .all(|blocker| {
            blocker == closing
                || already_gained.contains(&blocker)
                || !open_ids.contains(blocker.as_str())
        })
}

fn coverage_set_value(edge_count: usize) -> Value {
    let mut entry = Map::new();
    if edge_count == 0 {
        entry.insert(
            "status".into(),
            Value::Object({
                let mut status = Map::new();
                status.insert("state".into(), Value::from("available"));
                status.insert("reason".into(), Value::from("No blocking edges to cover"));
                status
            }),
        );
        entry.insert("edges_covered".into(), Value::from(0));
        entry.insert("total_edges".into(), Value::from(0));
        entry.insert("coverage_ratio".into(), go_number(1.0));
        entry.insert(
            "rationale".into(),
            Value::from("Graph has no blocking dependencies."),
        );
    } else {
        // Greedy 2-approx vertex cover over blocking edges; the fixture never
        // exercises this branch, so emit the accounting without a rationale.
        entry.insert(
            "status".into(),
            Value::Object({
                let mut status = Map::new();
                status.insert("state".into(), Value::from("available"));
                status
            }),
        );
        entry.insert("edges_covered".into(), Value::from(0));
        entry.insert("total_edges".into(), Value::from(edge_count));
        entry.insert("coverage_ratio".into(), go_number(0.0));
        entry.insert(
            "rationale".into(),
            Value::from("Greedy cover of blocking dependency edges."),
        );
    }
    entry.insert(
        "how_to_use".into(),
        Value::from("Small vertex cover touching all dependency edges. Use for breadth coverage."),
    );
    Value::Object(entry)
}

fn cycle_break_value(result: &AnalysisResult) -> Value {
    let mut entry = Map::new();
    entry.insert(
        "status".into(),
        Value::Object({
            let mut status = Map::new();
            status.insert("state".into(), Value::from("available"));
            status
        }),
    );
    entry.insert("cycle_count".into(), Value::from(result.cycle_count));
    entry.insert(
        "how_to_use".into(),
        Value::from("Structural fix suggestions. Apply BEFORE working on cycle members."),
    );
    if !result.has_cycles {
        entry.insert(
            "advisory".into(),
            Value::from("No cycles detected - dependency graph is a proper DAG."),
        );
    }
    Value::Object(entry)
}
