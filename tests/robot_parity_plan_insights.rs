// governed-by: ADR-0003
//! Golden parity tests for `br plan --json` and `br insights --json`
//! (ADR-0003 §3.1 parity targets, §5 proof item 1).
//!
//! The committed goldens under `tests/fixtures/bv_parity/` were generated
//! from real `bv` output on the 12-issue fixture workspace and reproduce
//! deterministically from `fixture_issues.jsonl`. These tests assert
//! field-shape identity after stripping volatile fields (`generated_at`,
//! `data_hash`, `version`, `*.ms`), rank-stable metric maps, and plan-track
//! composition. Float bit-parity is deliberately not asserted (ADR-0003 §3.1).

use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

mod common;

use common::cli::BrWorkspace;

const FIXTURE_ISSUES: &str = include_str!("fixtures/bv_parity/fixture_issues.jsonl");

/// Fields that legitimately vary between runs and must be ignored by the
/// structural diff (ADR-0003 §5: "structural diff ignoring volatile fields").
const VOLATILE_FIELDS: &[&str] = &["generated_at", "data_hash", "version", "ms"];

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

/// Recursively compares two normalized envelopes. Floats compare within a
/// tight relative tolerance because ADR-0003 §3.1 makes float parity with
/// Go best-effort ("same top pick, same ordering", not bit-equal doubles);
/// everything else must match exactly.
fn collect_mismatches(actual: &Value, golden: &Value, path: &str, out: &mut Vec<String>) {
    const FLOAT_TOLERANCE: f64 = 1e-9;
    match (actual, golden) {
        (Value::Number(left), Value::Number(right)) => {
            let (Some(left), Some(right)) = (left.as_f64(), right.as_f64()) else {
                if left != right {
                    out.push(format!("{path}: {left} != {right}"));
                }
                return;
            };
            let scale = right.abs().max(1.0);
            if (left - right).abs() > FLOAT_TOLERANCE * scale {
                out.push(format!("{path}: {left} !~ {right}"));
            }
        }
        (Value::Object(left), Value::Object(right)) => {
            let keys: std::collections::BTreeSet<&String> =
                left.keys().chain(right.keys()).collect();
            for key in keys {
                let joined = format!("{path}/{key}");
                match (left.get(key), right.get(key)) {
                    (Some(l), Some(r)) => collect_mismatches(l, r, &joined, out),
                    (None, Some(_)) => out.push(format!("{joined}: missing in actual")),
                    (Some(_), None) => out.push(format!("{joined}: extra in actual")),
                    (None, None) => unreachable!(),
                }
            }
        }
        (Value::Array(left), Value::Array(right)) => {
            if left.len() != right.len() {
                out.push(format!("{path}: length {} != {}", left.len(), right.len()));
            }
            for (index, (l, r)) in left.iter().zip(right.iter()).enumerate() {
                collect_mismatches(l, r, &format!("{path}[{index}]"), out);
            }
        }
        _ => {
            if actual != golden {
                out.push(format!("{path}: {actual} != {golden}"));
            }
        }
    }
}

fn assert_parity(actual: &Value, golden: &Value, label: &str) {
    let mut mismatches = Vec::new();
    collect_mismatches(actual, golden, "", &mut mismatches);
    assert!(
        mismatches.is_empty(),
        "{label} diverges from the bv golden ({} diffs):\n{}",
        mismatches.len(),
        mismatches
            .iter()
            .take(20)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

fn golden_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/bv_parity")
        .join(name)
}

fn load_golden(name: &str) -> Value {
    let raw = std::fs::read_to_string(golden_path(name))
        .unwrap_or_else(|error| panic!("golden fixture {name} must exist: {error}"));
    serde_json::from_str(&raw).unwrap_or_else(|error| panic!("golden {name} must parse: {error}"))
}

fn workspace_with_fixture() -> BrWorkspace {
    let workspace = BrWorkspace::new();
    let beads_dir = workspace.root.join(".beads");
    std::fs::create_dir_all(&beads_dir).expect("create .beads dir");

    // bv reads inline `dependencies` straight from the JSONL and drops the
    // fixture's dangling short-id references ("a", "b", ...) — the committed
    // goldens therefore describe an EDGELESS graph plus deferred-blocked
    // accounting. Replaying those edges through `br dep add` would normalize
    // the short ids into REAL blocking edges (`fx-b`), producing a different
    // graph than the golden describes. Parity requires reproducing bv's
    // interpretation, so the edges are stripped, not replayed. Persisting
    // dangling refs at all is beads_rust-svtxe.
    let mut stripped_lines: Vec<String> = Vec::new();
    for line in FIXTURE_ISSUES
        .lines()
        .filter(|line| !line.trim().is_empty())
    {
        let mut issue: Value = serde_json::from_str(line).expect("fixture line parses");
        issue
            .as_object_mut()
            .and_then(|map| map.remove("dependencies"));
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
    workspace
}

fn run_robot_json(workspace: &BrWorkspace, args: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_br"))
        .current_dir(&workspace.root)
        .args(args)
        .env("NO_COLOR", "1")
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

fn metric_ids(entries: &Value) -> Vec<String> {
    entries
        .as_array()
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| {
                    entry
                        .get("id")
                        .or_else(|| entry.get("issue_id"))
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn robot_plan_matches_bv_golden_shape_and_track_composition() {
    let workspace = workspace_with_fixture();
    let actual = strip_volatile(&run_robot_json(&workspace, &["plan", "--json"]));
    let golden = strip_volatile(&load_golden("robot-plan.json"));

    assert_parity(
        &actual,
        &golden,
        "`br plan --json` must be structurally identical to the bv golden",
    );

    let actual_tracks = actual
        .pointer("/plan/tracks")
        .and_then(Value::as_array)
        .expect("plan.tracks array");
    let golden_tracks = golden
        .pointer("/plan/tracks")
        .and_then(Value::as_array)
        .expect("golden plan.tracks array");
    assert_eq!(actual_tracks.len(), golden_tracks.len());
    for (actual_track, golden_track) in actual_tracks.iter().zip(golden_tracks.iter()) {
        assert_eq!(
            actual_track.get("track_id"),
            golden_track.get("track_id"),
            "track identity must match"
        );
        let actual_items = metric_ids(actual_track.get("items").expect("track items"));
        let golden_items = metric_ids(golden_track.get("items").expect("track items"));
        assert_eq!(
            actual_items, golden_items,
            "per-track item order defines the parallel execution track"
        );
    }
}

#[test]
fn robot_plan_marks_phase2_metrics_skipped_with_reasons() {
    let workspace = workspace_with_fixture();
    let actual = run_robot_json(&workspace, &["plan", "--json"]);

    // ADR-0003 §3.1: plan keeps Phase-2 metrics off with explicit skip reasons.
    for metric in ["PageRank", "Betweenness", "Eigenvector", "HITS"] {
        let status = actual
            .pointer(&format!("/status/{metric}"))
            .unwrap_or_else(|| panic!("status.{metric} missing from plan envelope"));
        assert_eq!(
            status.get("state").and_then(Value::as_str),
            Some("skipped"),
            "{metric} must be skipped in plan output"
        );
    }
}

#[test]
fn robot_insights_matches_bv_golden_shape_and_metric_ranking() {
    let workspace = workspace_with_fixture();
    let actual = strip_volatile(&run_robot_json(&workspace, &["insights", "--json"]));
    let golden = strip_volatile(&load_golden("robot-insights.json"));

    assert_parity(
        &actual,
        &golden,
        "`br insights --json` must be structurally identical to the bv golden",
    );

    // Rank stability on the flywheel-critical maps (ADR-0003 §3.1 scores note).
    for map in [
        "Bottlenecks",
        "Keystones",
        "Influencers",
        "Hubs",
        "Authorities",
        "Cores",
        "Articulation",
    ] {
        assert_eq!(
            metric_ids(actual.get(map).unwrap_or(&Value::Null)),
            metric_ids(golden.get(map).unwrap_or(&Value::Null)),
            "{map} ordering must be rank-stable against the bv golden"
        );
    }
}
