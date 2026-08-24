//! `br triage` + `br next` — the robot surface flywheel consumes
//! (ADR-0003 §3.1, bead gu7ts.3). governed-by: ADR-0003.
//!
//! stdout = JSON payload only; diagnostics go to stderr; exit 0/1/2. The
//! analysis clock pins to `BR_ANALYSIS_NOW` (RFC3339) when the env var is
//! set, so golden-parity tests are reproducible (see `src/analysis/triage.rs`).

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::analysis::data_hash::compute_data_hash;
use crate::analysis::triage::{
    TopPick, TriageResult, compute_triage, go_time_nanos, go_time_secs, metric_claim_unsafe,
};
use crate::config::{self, CliOverrides};
use crate::error::{BeadsError, Result};
use crate::model::{Issue, Status};
use crate::output::OutputContext;
use crate::storage::ListFilters;

/// Resolve the analysis instant: `BR_ANALYSIS_NOW` when set (fail loud on a
/// malformed value), wall clock otherwise.
fn resolve_now() -> Result<DateTime<Utc>> {
    match std::env::var("BR_ANALYSIS_NOW") {
        Ok(raw) => {
            let trimmed = raw.trim();
            DateTime::parse_from_rfc3339(trimmed)
                .map(Into::into)
                .map_err(|error| {
                    BeadsError::Config(format!(
                        "BR_ANALYSIS_NOW must be RFC3339 (got {trimmed:?}): {error}"
                    ))
                })
        }
        Err(std::env::VarError::NotPresent) => Ok(Utc::now()),
        Err(error) => Err(BeadsError::Config(format!(
            "BR_ANALYSIS_NOW unreadable: {error}"
        ))),
    }
}

/// Every issue in the workspace, fully hydrated (labels + dependencies +
/// comments), which is exactly what bv's JSONL loader hands its analyzer.
fn load_all_issues(cli: &CliOverrides) -> Result<Vec<Issue>> {
    let Some(beads_dir) = config::discover_optional_beads_dir_with_cli(cli)? else {
        return Ok(Vec::new());
    };
    let storage_ctx = config::open_storage_with_cli(&beads_dir, cli)?;
    let filters = ListFilters {
        include_closed: true,
        include_deferred: true,
        ..Default::default()
    };
    let listed = storage_ctx.storage.list_issues(&filters)?;
    let ids: Vec<String> = listed.iter().map(|issue| issue.id.clone()).collect();
    let mut issues = if ids.is_empty() {
        Vec::new()
    } else {
        storage_ctx.storage.get_issues_for_export(&ids)?
    };
    issues.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(issues)
}

fn print_json<T: Serialize>(payload: &T) -> Result<()> {
    let serialized = serde_json::to_string(payload)
        .map_err(|error| BeadsError::Config(format!("robot payload serialize: {error}")))?;
    println!("{serialized}");
    Ok(())
}

// ---------------------------------------------------------------------------
// br triage
// ---------------------------------------------------------------------------

/// bv wraps the triage result in an envelope WITHOUT output_format/version
/// (its other robot commands carry them; the triage handler does not).
#[derive(Serialize)]
struct TriageEnvelope<'a> {
    generated_at: String,
    data_hash: String,
    triage: &'a TriageResult,
    usage_hints: [&'static str; 14],
}

/// Execute `br triage` (the mega-command; ADR-0003 §3.1).
///
/// # Errors
///
/// Returns an error when storage cannot be opened or the payload fails to
/// serialize.
pub fn execute_triage(
    _args: &crate::cli::TriageArgs,
    cli: &CliOverrides,
    outer_ctx: &OutputContext,
) -> Result<()> {
    if matches!(outer_ctx.mode(), crate::output::OutputMode::Quiet) {
        return Ok(());
    }
    let now = resolve_now()?;
    let issues = load_all_issues(cli)?;
    let result = compute_triage(&issues, now, env!("CARGO_PKG_VERSION"));
    let envelope = TriageEnvelope {
        generated_at: go_time_secs(now),
        data_hash: compute_data_hash(&issues),
        triage: &result,
        usage_hints: [
            "jq '.triage.quick_ref.top_picks[:3]' - Top 3 picks for immediate work",
            "jq '.triage.recommendations[3:10] | map({id,title,score})' - Next candidates after top picks",
            "jq '.triage.blockers_to_clear | map(.id)' - High-impact blockers to clear",
            "jq '.triage.recommendations[] | select(.type == \"bug\")' - Bug-focused recommendations",
            "jq '.triage.quick_ref.top_picks[] | select(.unblocks > 2)' - High-impact picks",
            "jq '.triage.quick_wins' - Low-effort, high-impact items",
            "--robot-next - Get only the single top recommendation",
            "--brief - Compact output: only id/title/status/assignee/blockers/unblocks (#183)",
            "--robot-triage-by-track - Group by execution track for multi-agent coordination",
            "--robot-triage-by-label - Group by label for area-focused agents",
            "jq '.triage.recommendations_by_track[].top_pick' - Top pick per track",
            "jq '.triage.recommendations_by_label[].claim_command' - Claim commands per label",
            "jq '.feedback.weight_adjustments' - View feedback-adjusted weights (bv-90)",
            "--graph-root <id> - Scope triage to subgraph rooted at a specific epic (bv-140)",
        ],
    };
    print_json(&envelope)
}

// ---------------------------------------------------------------------------
// br next — fail-closed claim contract (bv robotNext port)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct NextDegradation {
    code: &'static str,
    severity: &'static str,
    message: String,
    repair: &'static str,
}

#[derive(Debug, Serialize)]
struct NextDiagnosticPick {
    id: String,
    title: String,
    score: f64,
    reasons: Vec<String>,
    unblocks: usize,
}

#[derive(Debug, Serialize)]
struct NextOutput {
    generated_at: String,
    data_hash: String,
    output_format: &'static str,
    version: String,

    actionable: bool,
    phase2_ready: bool,
    status: crate::analysis::MetricStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    score: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasons: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    unblocks: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    diagnostic_top_pick: Option<NextDiagnosticPick>,
    #[serde(skip_serializing_if = "Option::is_none")]
    claim_command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    show_command: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    degraded: Vec<NextDegradation>,
    usage_hints: [&'static str; 3],
}

fn diagnostic_from(pick: &TopPick) -> NextDiagnosticPick {
    NextDiagnosticPick {
        id: pick.id.clone(),
        title: pick.title.clone(),
        score: pick.score,
        reasons: pick.reasons.clone(),
        unblocks: pick.unblocks,
    }
}

/// Strict claimability reasons over the RAW issue (bv
/// `robotNextClaimabilityReasons`): unlike the triage gate this counts
/// dangling dependency ids as open blockers (`"<id> (missing)"`).
fn claimability_reasons(
    pick: &TopPick,
    by_id: &HashMap<&str, &Issue>,
    now: DateTime<Utc>,
) -> Vec<String> {
    let Some(issue) = by_id.get(pick.id.as_str()) else {
        return vec![format!("{} is absent from loaded Beads records", pick.id)];
    };

    let mut reasons = Vec::new();
    if !issue
        .status
        .as_str()
        .eq_ignore_ascii_case(Status::Open.as_str())
    {
        reasons.push(format!("{} status is {:?}", pick.id, issue.status.as_str()));
    }
    if matches!(issue.issue_type, crate::model::IssueType::Epic) {
        reasons.push(format!("{} is an epic", pick.id));
    }
    if let Some(assignee) = issue
        .assignee
        .as_deref()
        .filter(|assignee| !assignee.trim().is_empty())
    {
        reasons.push(format!(
            "{} is already assigned to {}",
            pick.id,
            assignee.trim()
        ));
    }
    if issue.defer_until.is_some_and(|at| at > now) {
        reasons.push(format!(
            "{} is deferred until {}",
            pick.id,
            go_time_nanos(issue.defer_until.expect("checked above"))
        ));
    }

    let mut open_blockers: Vec<String> = Vec::new();
    for dep in &issue.dependencies {
        if !crate::analysis::triage::is_triage_blocking(&dep.dep_type) {
            continue;
        }
        let blocker_id = dep.depends_on_id.trim();
        if blocker_id.is_empty() {
            open_blockers.push("<missing blocker id>".to_string());
            continue;
        }
        match by_id.get(blocker_id) {
            None => open_blockers.push(format!("{blocker_id} (missing)")),
            Some(blocker) if !matches!(blocker.status, Status::Closed | Status::Tombstone) => {
                open_blockers.push(blocker_id.to_string());
            }
            Some(_) => {}
        }
    }
    if !open_blockers.is_empty() {
        open_blockers.sort();
        reasons.push(format!(
            "{} is blocked by {}",
            pick.id,
            open_blockers.join(", ")
        ));
    }

    reasons
}

/// Execute `br next` (fail-closed top pick; ADR-0003 §3.2).
///
/// # Errors
///
/// Returns an error when storage cannot be opened or the payload fails to
/// serialize.
pub fn execute_next(
    _args: &crate::cli::NextArgs,
    cli: &CliOverrides,
    outer_ctx: &OutputContext,
) -> Result<()> {
    if matches!(outer_ctx.mode(), crate::output::OutputMode::Quiet) {
        return Ok(());
    }
    let now = resolve_now()?;
    let issues = load_all_issues(cli)?;
    let data_hash = compute_data_hash(&issues);
    let triage = compute_triage(&issues, now, env!("CARGO_PKG_VERSION"));

    let mut output = NextOutput {
        generated_at: go_time_secs(now),
        data_hash,
        output_format: "json",
        version: env!("CARGO_PKG_VERSION").to_string(),
        actionable: false,
        phase2_ready: triage.meta.phase2_ready,
        status: triage.status.clone(),
        message: None,
        id: None,
        title: None,
        score: None,
        reasons: None,
        unblocks: None,
        diagnostic_top_pick: None,
        claim_command: None,
        show_command: None,
        degraded: Vec::new(),
        usage_hints: [
            "Use scripts/br_retry.sh actionable --json plus the claim gate before mutating Beads state in crowded swarms.",
            "No claim_command is emitted unless the item is open, unblocked, unassigned, and triage metrics are ready.",
            "Inspect .status for skipped, timeout, or pending graph phases.",
        ],
    };

    let picks = &triage.quick_ref.top_picks;
    if picks.is_empty() {
        output.message = Some("No proven actionable item available".to_string());
        output.degraded.push(NextDegradation {
            code: "no_actionable_recommendation",
            severity: "info",
            message: "No open, unblocked, unassigned non-epic recommendation passed the robot-next claimability filter.".to_string(),
            repair: "Use br ready --json or scripts/br_retry.sh actionable --json for authoritative claim candidates.",
        });
        return print_json(&output);
    }

    // Walk the picks with the STRICT raw-issue predicate; on failure the
    // diagnostic names the FIRST pick either way (bv behavior).
    let by_id: HashMap<&str, &Issue> = issues
        .iter()
        .map(|issue| (issue.id.as_str(), issue))
        .collect();
    let first_diagnostic = diagnostic_from(&picks[0]);
    let mut first_unsafe_reasons: Option<Vec<String>> = None;
    let mut chosen: Option<&TopPick> = None;
    for pick in picks {
        let reasons = claimability_reasons(pick, &by_id, now);
        if reasons.is_empty() {
            chosen = Some(pick);
            break;
        }
        if first_unsafe_reasons.is_none() {
            first_unsafe_reasons = Some(reasons);
        }
    }

    let Some(top) = chosen else {
        output.message = Some(
            "No claim command emitted because the top recommendation was not claim-safe"
                .to_string(),
        );
        output.diagnostic_top_pick = Some(first_diagnostic);
        output.degraded.push(NextDegradation {
            code: "robot_next_claim_unsafe",
            severity: "warning",
            message: first_unsafe_reasons.unwrap_or_default().join("; "),
            repair:
                "Use the authoritative Beads actionable queue plus claim gate before claiming work.",
        });
        return print_json(&output);
    };

    if metric_claim_unsafe(&triage.status) {
        output.message =
            Some("No claim command emitted because triage metrics were incomplete".to_string());
        output.diagnostic_top_pick = Some(first_diagnostic);
        output.degraded.push(NextDegradation {
            code: "robot_next_metric_incomplete",
            severity: "warning",
            message: metric_unsafe_message(&triage.status),
            repair: "Retry bv --robot-next after graph metrics are available, or use the authoritative Beads actionable queue plus claim gate.",
        });
        return print_json(&output);
    }

    output.actionable = true;
    output.id = Some(top.id.clone());
    output.title = Some(top.title.clone());
    output.score = Some(top.score);
    output.reasons = Some(top.reasons.clone());
    output.unblocks = Some(top.unblocks);
    output.claim_command = Some(format!("br update {} --status=in_progress", top.id));
    output.show_command = Some(format!("br show {}", top.id));
    print_json(&output)
}

fn metric_unsafe_message(status: &crate::analysis::MetricStatus) -> String {
    let describe = |entry: Option<&crate::analysis::MetricEntry>, name: &str| match entry {
        None => format!("{name} unavailable"),
        Some(entry) => match entry.reason.as_deref() {
            Some(reason) if !reason.is_empty() => format!("{name}: {reason}"),
            _ => format!("{name} state incomplete"),
        },
    };
    [
        describe(status.pagerank.as_ref(), "PageRank"),
        describe(status.betweenness.as_ref(), "Betweenness"),
    ]
    .join("; ")
}
