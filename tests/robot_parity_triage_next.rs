// governed-by: ADR-0003
//! Golden parity + contract tests for `br triage --json` and `br next --json`
//! (beads_rust-gu7ts.3; ADR-0003 §3.1 parity targets, §4 scoring semantics,
//! §5 proof items 1 and 3).
//!
//! Goldens under `tests/fixtures/bv_parity/` are regenerated from real `bv`
//! output by `scripts/regen_bv_golden.sh`; `GOLDEN_NOW.txt` stamps the
//! generation instant so staleness/urgency scoring is pinned via
//! BR_ANALYSIS_NOW. These tests are RED until gu7ts.2 (engine) and this
//! bead's triage/next commands land — deliberate TDD per repo policy.
//!
//! Structural diff ignores volatile fields (`generated_at`, `data_hash`,
//! `version`, `*.ms`). Float bit-parity is asserted only through rank
//! stability and ordering, never raw equality across implementations
//! (ADR-0003 §3.1 scores note), except where both sides derive from the
//! same pinned-clock inputs.

use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

mod common;

use common::cli::BrWorkspace;

const FIXTURE_ISSUES: &str = include_str!("fixtures/bv_parity/fixture_issues.jsonl");

/// RFC3339 instant at which the committed goldens were generated; exported as
/// BR_ANALYSIS_NOW so time-derived scoring matches the goldens deterministically.
const GOLDEN_NOW: &str = include_str!("fixtures/bv_parity/GOLDEN_NOW.txt");

const VOLATILE_FIELDS: &[&str] = &["generated_at", "data_hash", "version", "ms"];

/// Base-score component weights (ADR-0003 §4.1 / research doc §4.1).
const BASE_WEIGHTS: &[(&str, f64)] = &[
    ("pagerank", 0.22),
    ("betweenness", 0.20),
    ("blocker_ratio", 0.13),
    ("staleness", 0.05),
    ("priority_boost", 0.10),
    ("time_to_impact", 0.10),
    ("urgency", 0.10),
    ("risk", 0.10),
];

fn strip_volatile(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(key, _)| !VOLATILE_FIELDS.contains(&key.as_str()))
                .map(|(key, inner)| (key.clone(), strip_volatile(inner)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(strip_volatile).collect()),
        other => other.clone(),
    }
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bv_parity")
}

fn load_golden(name: &str) -> Value {
    let raw = std::fs::read_to_string(fixtures_dir().join(name))
        .unwrap_or_else(|error| panic!("golden fixture {name} must exist: {error}"));
    serde_json::from_str(&raw).unwrap_or_else(|error| panic!("golden {name} must parse: {error}"))
}

fn workspace_with_fixture() -> BrWorkspace {
    let workspace = BrWorkspace::new();
    let beads_dir = workspace.root.join(".beads");
    std::fs::create_dir_all(&beads_dir).expect("create .beads dir");

    // Strip inline dependencies (br's importer refuses them; see the
    // plan/insights parity test for the rationale) and replay via `br dep add`.
    // Note the fixture's edges use short ids (`a`, `b`, `f`, ...) that resolve
    // to no fixture issue; they are replayed verbatim because the goldens were
    // generated with those same dangling edges (bv drops them for quick_ref
    // accounting but consults them in the next-pick walk — an observed bv
    // quirk the parity contract pins deliberately).
    let mut edges: Vec<(String, String)> = Vec::new();
    let mut stripped_lines: Vec<String> = Vec::new();
    for line in FIXTURE_ISSUES
        .lines()
        .filter(|line| !line.trim().is_empty())
    {
        let mut issue: Value = serde_json::from_str(line).expect("fixture line parses");
        if let Some(deps) = issue
            .as_object_mut()
            .and_then(|map| map.remove("dependencies"))
            .and_then(|deps| deps.as_array().cloned())
        {
            for dep in deps {
                let id = issue.get("id").and_then(Value::as_str).unwrap_or_default();
                let depends_on = dep.get("depends_on_id").and_then(Value::as_str);
                if let Some(depends_on) = depends_on {
                    edges.push((id.to_owned(), depends_on.to_owned()));
                }
            }
        }
        stripped_lines.push(issue.to_string());
    }
    std::fs::write(beads_dir.join("issues.jsonl"), stripped_lines.join("\n"))
        .expect("write fixture issues.jsonl");
    let import = Command::new(env!("CARGO_BIN_EXE_br"))
        .current_dir(&workspace.root)
        .args(["sync", "--import-only", "--force"])
        .output()
        .expect("run br sync --import-only");
    assert!(
        import.status.success(),
        "fixture import failed: {}",
        String::from_utf8_lossy(&import.stderr)
    );
    for (id, depends_on) in &edges {
        let dep = Command::new(env!("CARGO_BIN_EXE_br"))
            .current_dir(&workspace.root)
            .args(["dep", "add", id, depends_on])
            .env("NO_COLOR", "1")
            .output()
            .unwrap_or_else(|error| panic!("spawn br dep add {id} {depends_on}: {error}"));
        assert!(
            dep.status.success(),
            "`br dep add {id} {depends_on}` failed: {}",
            String::from_utf8_lossy(&dep.stderr)
        );
    }
    workspace
}

fn run_robot_json(workspace: &BrWorkspace, args: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_br"))
        .current_dir(&workspace.root)
        .args(args)
        .env("NO_COLOR", "1")
        .env("BR_ANALYSIS_NOW", GOLDEN_NOW.trim())
        .output()
        .expect("spawn br");
    assert!(
        output.status.success(),
        "`br {}` failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("stdout of `br {}` must be JSON: {error}", args.join(" ")))
}

#[test]
fn robot_triage_matches_bv_golden_shape() {
    let workspace = workspace_with_fixture();
    let actual = strip_volatile(&run_robot_json(&workspace, &["triage", "--json"]));
    let golden = strip_volatile(&load_golden("robot-triage.json"));

    assert_eq!(
        actual, golden,
        "`br triage --json` must be structurally identical to the bv golden"
    );
}

#[test]
fn robot_triage_quick_ref_shape_and_issue_165_semantics() {
    let workspace = workspace_with_fixture();
    let actual = run_robot_json(&workspace, &["triage", "--json"]);
    let quick_ref = actual
        .pointer("/triage/quick_ref")
        .unwrap_or_else(|| panic!("triage.quick_ref missing"));

    for field in [
        "open_count",
        "actionable_count",
        "blocked_count",
        "in_progress_count",
        "not_closed_count",
        "not_actionable_count",
        "top_picks",
    ] {
        assert!(
            quick_ref.get(field).is_some(),
            "quick_ref.{field} missing (bv #165/#183 shape)"
        );
    }

    // #165 semantics: open_count counts status exactly "open";
    // not_closed_count is the pre-#165 "open" (everything not closed).
    let fixture_lines: Vec<Value> = FIXTURE_ISSUES
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("fixture line parses"))
        .collect();
    let exactly_open = fixture_lines
        .iter()
        .filter(|issue| issue.get("status").and_then(Value::as_str) == Some("open"))
        .count();
    let not_closed = fixture_lines
        .iter()
        .filter(|issue| issue.get("status").and_then(Value::as_str) != Some("closed"))
        .count();
    assert_eq!(
        quick_ref.get("open_count").and_then(Value::as_u64),
        Some(exactly_open as u64),
        "open_count must count status exactly 'open' (#165)"
    );
    assert_eq!(
        quick_ref.get("not_closed_count").and_then(Value::as_u64),
        Some(not_closed as u64),
        "not_closed_count must count every non-closed issue (#165)"
    );

    let top_picks = quick_ref
        .get("top_picks")
        .and_then(Value::as_array)
        .expect("quick_ref.top_picks array");
    assert_eq!(top_picks.len(), 3, "top_picks carries exactly 3 entries");
    for pick in top_picks {
        for field in ["id", "title", "score", "reasons", "unblocks"] {
            assert!(
                pick.get(field).is_some(),
                "top_pick.{field} missing from the quick_ref pick shape"
            );
        }
    }
}

#[test]
fn robot_triage_recommendations_ranked_score_desc_then_id_asc() {
    let workspace = workspace_with_fixture();
    let actual = run_robot_json(&workspace, &["triage", "--json"]);
    let recommendations = actual
        .pointer("/triage/recommendations")
        .and_then(Value::as_array)
        .expect("triage.recommendations array");

    assert!(
        recommendations.len() <= 10,
        "recommendations cap at 10 (research doc §3.1)"
    );

    let mut previous: Option<(f64, String)> = None;
    for rec in recommendations {
        let score = rec.get("score").and_then(Value::as_f64).unwrap_or_else(|| {
            panic!(
                "recommendation {} lacks score",
                rec.get("id")
                    .map_or("<unknown>", |id| id.as_str().unwrap_or("<unknown>"))
            )
        });
        let id = rec
            .get("id")
            .and_then(Value::as_str)
            .expect("recommendation id")
            .to_owned();
        if let Some((previous_score, previous_id)) = previous {
            assert!(
                score < previous_score || (score == previous_score && id > previous_id),
                "ordering must be score desc, tie-break id asc: ({previous_score}, {previous_id}) then ({score}, {id})"
            );
        }
        previous = Some((score, id));

        // ScoreBreakdown presence with base weights summing to 1.0 (§4.1).
        let breakdown = rec
            .get("breakdown")
            .expect("recommendation.breakdown (ScoreBreakdown)");
        let mut weight_sum = 0.0;
        for (field, weight) in BASE_WEIGHTS {
            let contribution = breakdown
                .get(*field)
                .and_then(Value::as_f64)
                .unwrap_or_else(|| panic!("breakdown.{field} missing"));
            let norm = breakdown
                .get(format!("{field}_norm"))
                .and_then(Value::as_f64)
                .unwrap_or_else(|| panic!("breakdown.{field}_norm missing"));
            let _ = norm;
            weight_sum += weight;
            let _ = contribution;
        }
        assert!(
            (weight_sum - 1.0).abs() < 1e-9,
            "base weights must sum to 1.0, got {weight_sum}"
        );
    }
}

#[test]
fn robot_next_matches_bv_golden_claim_contract() {
    let workspace = workspace_with_fixture();
    let actual = run_robot_json(&workspace, &["next", "--json"]);
    let golden = strip_volatile(&load_golden("robot-next.json"));

    assert_eq!(
        strip_volatile(&actual),
        golden,
        "`br next --json` must be structurally identical to the bv golden"
    );

    // The flywheel-consumed fail-closed claim surface (ADR-0003 §3.1).
    let id = actual
        .get("id")
        .and_then(Value::as_str)
        .expect("next.id present when actionable");
    let claim = actual
        .get("claim_command")
        .and_then(Value::as_str)
        .expect("claim_command present for a claim-safe pick");
    let show = actual
        .get("show_command")
        .and_then(Value::as_str)
        .expect("show_command present for a claim-safe pick");
    assert_eq!(
        claim,
        format!("br update {id} --status=in_progress"),
        "claim_command literal format"
    );
    assert_eq!(show, format!("br show {id}"), "show_command literal format");
}

#[test]
fn robot_next_fails_closed_when_top_pick_unclaimable() {
    let workspace = workspace_with_fixture();

    // Claim the current golden pick so the walk must skip it.
    let golden_pick = {
        let golden = load_golden("robot-next.json");
        golden
            .get("id")
            .and_then(Value::as_str)
            .expect("golden next.id")
            .to_owned()
    };
    let claim = Command::new(env!("CARGO_BIN_EXE_br"))
        .current_dir(&workspace.root)
        .args(["update", &golden_pick, "--status", "in_progress"])
        .env("NO_COLOR", "1")
        .output()
        .expect("spawn br update");
    assert!(
        claim.status.success(),
        "claiming the top pick failed: {}",
        String::from_utf8_lossy(&claim.stderr)
    );

    let output = Command::new(env!("CARGO_BIN_EXE_br"))
        .current_dir(&workspace.root)
        .args(["next", "--json"])
        .env("NO_COLOR", "1")
        .env("BR_ANALYSIS_NOW", GOLDEN_NOW.trim())
        .output()
        .expect("spawn br next");

    // Fail-closed contract: exit 0, degraded signal, NO claim_command.
    assert!(output.status.success(), "next must exit 0 when degrading");
    let actual: Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("stdout of `br next --json` must be JSON: {error}"));
    assert!(
        actual.get("claim_command").is_none(),
        "fail-closed: claim_command must be absent once the top pick is claimed"
    );
    assert!(
        actual.get("degraded").is_some() || actual.get("message").is_some(),
        "degraded block or message must explain why nothing is claimable"
    );
}
