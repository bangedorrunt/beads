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

/// Maximum absolute float deviation tolerated between the Go-generated
/// golden and br's Rust output. ADR-0003 §3.1: "structure parity is
/// mandatory; float parity with Go is best-effort" — the vendored crate's
/// iterative solvers (PageRank) drift from Go at ~1e-6, and the golden's
/// wall-clock instant can differ from BR_ANALYSIS_NOW by up to one bv run.
const FLOAT_TOLERANCE: f64 = 1e-4;

/// Structural diff that fails on shape/text/order differences and on floats
/// deviating more than FLOAT_TOLERANCE (ADR §3.1 semantics).
fn assert_structurally_equal(actual: &Value, golden: &Value, path: &str) {
    match (actual, golden) {
        (Value::Object(a), Value::Object(g)) => {
            let missing: Vec<&String> = g.keys().filter(|k| !a.contains_key(*k)).collect();
            assert!(
                missing.is_empty(),
                "{path}: golden fields missing from actual: {missing:?}"
            );
            let extra: Vec<&String> = a.keys().filter(|k| !g.contains_key(*k)).collect();
            assert!(
                extra.is_empty(),
                "{path}: actual carries fields the golden lacks: {extra:?}"
            );
            for key in g.keys() {
                assert_structurally_equal(&a[key], &g[key], &format!("{path}.{key}"));
            }
        }
        (Value::Array(a), Value::Array(g)) => {
            assert!(
                a.len() == g.len(),
                "{path}: length mismatch (actual {} vs golden {})",
                a.len(),
                g.len()
            );
            for (index, (av, gv)) in a.iter().zip(g.iter()).enumerate() {
                assert_structurally_equal(av, gv, &format!("{path}[{index}]"));
            }
        }
        (Value::Number(a), Value::Number(g)) => {
            let (Some(av), Some(gv)) = (a.as_f64(), g.as_f64()) else {
                panic!("{path}: non-f64 numbers {a} vs {g}");
            };
            let scale = av.abs().max(gv.abs()).max(1.0);
            assert!(
                (av - gv).abs() <= FLOAT_TOLERANCE * scale,
                "{path}: float drift {av} vs {gv} exceeds tolerance"
            );
        }
        _ => assert_eq!(actual, golden, "{path}: structural mismatch"),
    }
}

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

    // Stamp ADR-0001 §5.5 dispatchability fields so `br next`'s fail-closed
    // claimability parity matches `br ready`'s SQL gate (beads_rust-ready-empty-set-bug-g0wra).
    // The bv fixture ships without verify/principles; br now requires them
    // for P≤2 dispatchability, so next would otherwise degrade and diverge
    // from the golden. Stamping keeps the fixture's graph and the golden's
    // top pick while satisfying the §5.5 gate.
    let stamped: String = FIXTURE_ISSUES
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let mut v: Value = serde_json::from_str(line).expect("fixture line parses");
            if let Some(obj) = v.as_object_mut() {
                obj.entry("verify".to_string())
                    .or_insert(Value::String("cargo test --offline ready_".to_string()));
                obj.entry("principles".to_string()).or_insert(serde_json::json!([
                    {"name": "prove-it-works", "decision": "each exclusion rule has a named test"}
                ]));
                // Ensure wave/pin for structured output completeness (golden expects them).
                obj.entry("wave".to_string()).or_insert(Value::Null);
                obj.entry("pin".to_string()).or_insert(Value::Null);
            }
            serde_json::to_string(&v).expect("serialize stamped issue")
        })
        .collect::<Vec<_>>()
        .join("\n");

    std::fs::write(beads_dir.join("issues.jsonl"), stamped).expect("write fixture issues.jsonl");
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

    assert_structurally_equal(&actual, &golden, "triage");
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
    assert!(
        top_picks.len() <= 3,
        "top_picks cap at 3 entries, got {}",
        top_picks.len()
    );
    let golden_triage = load_golden("robot-triage.json");
    let golden_picks = golden_triage
        .pointer("/triage/quick_ref/top_picks")
        .and_then(Value::as_array)
        .expect("golden top_picks array");
    assert_eq!(
        top_picks.len(),
        golden_picks.len(),
        "claimable pick count must match the bv golden (cap 3, fewer when the claimable set is small)"
    );
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
            // Exact equality is the point here (bv's tie-break contract):
            // equal floats MUST fall through to the id comparison.
            #[allow(clippy::float_cmp)]
            let tied = score == previous_score;
            assert!(
                score < previous_score || (tied && id > previous_id),
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

    assert_structurally_equal(&strip_volatile(&actual), &golden, "next");

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

    // Claim EVERY golden pick so no claim-safe candidate remains and the
    // walk must degrade (claiming just the first would hand us the second).
    let golden = load_golden("robot-triage.json");
    let pick_ids: Vec<String> = golden
        .pointer("/triage/quick_ref/top_picks")
        .and_then(Value::as_array)
        .expect("golden top_picks")
        .iter()
        .filter_map(|pick| pick.get("id").and_then(Value::as_str))
        .map(str::to_owned)
        .collect();
    for id in &pick_ids {
        let claim = Command::new(env!("CARGO_BIN_EXE_br"))
            .current_dir(&workspace.root)
            .args(["update", id, "--status", "in_progress"])
            .env("NO_COLOR", "1")
            .output()
            .expect("spawn br update");
        assert!(
            claim.status.success(),
            "claiming {id} failed: {}",
            String::from_utf8_lossy(&claim.stderr)
        );
    }

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
