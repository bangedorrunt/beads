//! E2E tests for the ADR-0001 §5.2 typed work-ledger flags on `br create` /
//! `br update`, plus the one-brief-schema lint contract end to end:
//!
//! - `--verify`, `--principle` (repeatable), `--wave`, `--pin`,
//!   `--commit-sha`, `--blast`, `--ac`
//! - `ac_shape` defaults: checkable when `verify` is present; judgment only
//!   when `verify` is absent AND `--ac judgment` is explicit
//! - `br lint` drops heading sections; wants non-empty `verify` and P≤2
//!   non-empty principles
//!
//! Flags write typed fields, never description markdown.

mod common;

use common::cli::{BrWorkspace, extract_json_payload, run_br};

fn init_workspace(workspace: &BrWorkspace) {
    let init = run_br(workspace, ["init"], "init");
    assert!(init.status.success(), "init failed: {}", init.stderr);
}

fn create_issue(workspace: &BrWorkspace, args: &[&str], label: &str) -> String {
    let mut full: Vec<&str> = vec!["create"];
    full.extend_from_slice(args);
    let out = run_br(workspace, &full, label);
    assert!(out.status.success(), "{label} failed: {}", out.stderr);
    let line = out.stdout.lines().next().unwrap_or("");
    let normalized = line.strip_prefix("✓ ").unwrap_or(line);
    normalized
        .strip_prefix("Created ")
        .and_then(|rest| rest.split(':').next())
        .unwrap_or("")
        .trim()
        .to_string()
}

fn show_json(workspace: &BrWorkspace, id: &str) -> serde_json::Value {
    let out = run_br(workspace, ["show", id, "--json"], "show_json");
    assert!(out.status.success(), "show failed: {}", out.stderr);
    let parsed: serde_json::Value =
        serde_json::from_str(&extract_json_payload(&out.stdout)).expect("valid show JSON");
    // `br show --json` emits a details array; the target is its only entry.
    match parsed {
        serde_json::Value::Array(items) => items
            .into_iter()
            .find(|i| i["id"].as_str() == Some(id))
            .unwrap_or(serde_json::Value::Null),
        obj @ serde_json::Value::Object(_) => obj,
        other => other,
    }
}

#[test]
fn e2e_create_typed_flags_round_trip() {
    let workspace = BrWorkspace::new();
    init_workspace(&workspace);

    let id = create_issue(
        &workspace,
        &[
            "Typed flag bead",
            "--verify",
            "timeout 900 cargo test --offline lint",
            "--wave",
            "5",
            "--pin",
            "CalmLantern",
            "--commit-sha",
            "abc1234",
            "--blast",
            "normal",
        ],
        "create_typed",
    );

    let issue = show_json(&workspace, &id);
    assert_eq!(
        issue["verify"].as_str(),
        Some("timeout 900 cargo test --offline lint")
    );
    assert_eq!(issue["wave"].as_u64(), Some(5));
    assert_eq!(issue["pin"].as_str(), Some("CalmLantern"));
    assert_eq!(issue["commit_sha"].as_str(), Some("abc1234"));
    assert_eq!(issue["blast"].as_str(), Some("normal"));
    // ac_shape default: checkable when verify is present.
    assert_eq!(issue["ac_shape"].as_str(), Some("checkable"));
}

#[test]
fn e2e_create_ac_judgment_requires_absent_verify() {
    let workspace = BrWorkspace::new();
    init_workspace(&workspace);

    let id = create_issue(
        &workspace,
        &["Judgment bead", "--ac", "judgment"],
        "create_judgment",
    );

    let issue = show_json(&workspace, &id);
    assert_eq!(issue["verify"].as_str(), None);
    assert_eq!(issue["ac_shape"].as_str(), Some("judgment"));
}

#[test]
fn e2e_create_verify_wins_over_ac_flag_conflict_is_rejected() {
    let workspace = BrWorkspace::new();
    init_workspace(&workspace);

    let out = run_br(
        &workspace,
        &[
            "create",
            "Conflicting bead",
            "--verify",
            "cargo build",
            "--ac",
            "judgment",
        ],
        "create_conflict",
    );
    assert!(
        !out.status.success(),
        "--ac judgment with --verify must be rejected, got success"
    );
}

#[test]
fn e2e_update_principle_appends_citation() {
    let workspace = BrWorkspace::new();
    init_workspace(&workspace);

    let id = create_issue(&workspace, &["Citation bead"], "create_plain");

    let first = run_br(
        &workspace,
        &[
            "update",
            &id,
            "--principle",
            "subtract-before-you-add — dual lint templates die; one brief schema",
        ],
        "update_principle_1",
    );
    assert!(first.status.success(), "update 1 failed: {}", first.stderr);

    let second = run_br(
        &workspace,
        &[
            "update",
            &id,
            "--principle",
            "prove-it-works — close is a gate row plus a SHA",
        ],
        "update_principle_2",
    );
    assert!(
        second.status.success(),
        "update 2 failed: {}",
        second.stderr
    );

    let issue = show_json(&workspace, &id);
    let principles = issue["principles"].as_array().expect("principles array");
    assert_eq!(principles.len(), 2, "appended citations: {principles:?}");
    assert_eq!(
        principles[0]["name"].as_str(),
        Some("subtract-before-you-add")
    );
    assert_eq!(
        principles[0]["decision"].as_str(),
        Some("dual lint templates die; one brief schema")
    );
    assert_eq!(principles[1]["name"].as_str(), Some("prove-it-works"));
    assert_eq!(
        principles[1]["decision"].as_str(),
        Some("close is a gate row plus a SHA")
    );
}

#[test]
fn e2e_update_principle_rejects_malformed_citation() {
    let workspace = BrWorkspace::new();
    init_workspace(&workspace);

    let id = create_issue(&workspace, &["Malformed bead"], "create_malformed");

    let out = run_br(
        &workspace,
        &["update", &id, "--principle", "NoSeparatorHere"],
        "update_principle_bad",
    );
    assert!(
        !out.status.success(),
        "citation without 'name — decision' separator must be rejected"
    );
}

#[test]
fn e2e_lint_does_not_require_acceptance_criteria_heading() {
    let workspace = BrWorkspace::new();
    init_workspace(&workspace);

    let id = create_issue(
        &workspace,
        &[
            "Heading-free bug",
            "--type",
            "bug",
            "--priority",
            "3",
            "--verify",
            "cargo test --offline lint",
        ],
        "create_heading_free",
    );

    // P3 + non-empty verify: no warnings despite zero markdown headings.
    let lint = run_br(&workspace, ["lint", "--json"], "lint_clean");
    assert!(lint.status.success(), "lint failed: {}", lint.stderr);
    let payload: serde_json::Value =
        serde_json::from_str(&extract_json_payload(&lint.stdout)).expect("lint JSON");
    let results = payload["results"].as_array().expect("results array");
    assert!(
        !results
            .iter()
            .any(|r| r["id"].as_str() == Some(id.as_str())),
        "heading-free bead must not warn: {results:?}"
    );

    // Same bead missing verify warns on `verify`, never on a heading section.
    let bare = create_issue(
        &workspace,
        &["Bare bug", "--type", "bug", "--priority", "3"],
        "create_bare",
    );
    let lint = run_br(&workspace, ["lint", "--json"], "lint_bare");
    let payload: serde_json::Value =
        serde_json::from_str(&extract_json_payload(&lint.stdout)).expect("lint JSON");
    let results = payload["results"].as_array().expect("results array");
    let entry = results
        .iter()
        .find(|r| r["id"].as_str() == Some(bare.as_str()))
        .expect("bare bead warns");
    let missing: Vec<&str> = entry["missing"]
        .as_array()
        .expect("missing array")
        .iter()
        .filter_map(|m| m.as_str())
        .collect();
    assert!(
        missing.iter().all(|m| !m.contains("#")),
        "no markdown headings in lint output: {missing:?}"
    );
    assert!(
        missing.contains(&"verify"),
        "expected verify warning, got: {missing:?}"
    );
}
