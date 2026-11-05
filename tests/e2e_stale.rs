//! E2E tests for the `stale` command.

mod common;

use common::cli::{BrWorkspace, extract_json_payload, parse_created_id, run_br};
use serde_json::Value;
use std::fs;

#[test]
fn e2e_stale_basic() {
    common::init_test_logging();
    let workspace = BrWorkspace::new();
    run_br(&workspace, ["init"], "init");

    run_br(&workspace, ["create", "Issue 1"], "create1");
    run_br(&workspace, ["create", "Issue 2"], "create2");
    run_br(&workspace, ["create", "Issue 3"], "create3");

    let stale0 = run_br(&workspace, ["stale", "--days", "0"], "stale0");
    assert!(stale0.status.success());
    let count = stale0.stdout.lines().filter(|l| l.contains(". [")).count();
    assert_eq!(count, 3, "All issues should be stale with days=0");

    let stale1 = run_br(&workspace, ["stale", "--days", "1"], "stale1");
    assert!(stale1.status.success());
    let count1 = stale1.stdout.lines().filter(|l| l.contains(". [")).count();
    assert_eq!(
        count1, 0,
        "No issues should be stale with days=1 (freshly created)"
    );
}

#[test]
fn e2e_stale_json_output() {
    common::init_test_logging();
    let workspace = BrWorkspace::new();
    run_br(&workspace, ["init"], "init");
    run_br(&workspace, ["create", "Issue JSON"], "create");

    let stale = run_br(&workspace, ["stale", "--days", "0", "--json"], "stale_json");
    assert!(stale.status.success());

    let payload = extract_json_payload(&stale.stdout);
    let json: Vec<Value> = serde_json::from_str(&payload).expect("valid json");
    assert_eq!(json.len(), 1);
    assert_eq!(json[0]["title"], "Issue JSON");
    assert!(json[0].get("updated_at").is_some());
}

#[test]
fn e2e_stale_with_status_filter() {
    common::init_test_logging();
    let workspace = BrWorkspace::new();
    run_br(&workspace, ["init"], "init");

    run_br(&workspace, ["create", "Open Issue"], "create1");

    let create2 = run_br(&workspace, ["create", "InProgress Issue"], "create2");
    let id2 = parse_created_id(&create2.stdout);
    run_br(
        &workspace,
        ["update", &id2, "--status", "in_progress"],
        "update2",
    );

    let stale_open = run_br(
        &workspace,
        ["stale", "--days", "0", "--status", "open"],
        "stale_open",
    );
    let count_open = stale_open
        .stdout
        .lines()
        .filter(|l| l.contains("Open Issue"))
        .count();
    assert_eq!(count_open, 1);
    assert!(!stale_open.stdout.contains("InProgress Issue"));
}

#[test]
fn e2e_stale_with_deferred_status_filter() {
    common::init_test_logging();
    let workspace = BrWorkspace::new();
    run_br(&workspace, ["init"], "init");

    let open = run_br(&workspace, ["create", "Open Issue"], "create_open");
    assert!(open.status.success(), "create open failed: {}", open.stderr);

    let deferred = run_br(&workspace, ["create", "Deferred Issue"], "create_deferred");
    assert!(
        deferred.status.success(),
        "create deferred failed: {}",
        deferred.stderr
    );
    let deferred_id = parse_created_id(&deferred.stdout);
    let defer = run_br(
        &workspace,
        [
            "update",
            &deferred_id,
            "--status",
            "deferred",
            "--defer=2100-01-01T00:00:00Z",
        ],
        "defer_issue",
    );
    assert!(defer.status.success(), "defer failed: {}", defer.stderr);

    let stale = run_br(
        &workspace,
        ["stale", "--days", "0", "--status", "deferred", "--json"],
        "stale_deferred",
    );
    assert!(
        stale.status.success(),
        "stale deferred failed: {}",
        stale.stderr
    );

    let payload = extract_json_payload(&stale.stdout);
    let json: Vec<Value> = serde_json::from_str(&payload).expect("valid json");
    assert_eq!(json.len(), 1);
    assert_eq!(json[0]["title"], "Deferred Issue");
    assert_eq!(json[0]["status"], "deferred");
}

#[test]
fn e2e_stale_default_includes_custom_nonterminal_statuses() {
    common::init_test_logging();
    let workspace = BrWorkspace::new();
    run_br(&workspace, ["init"], "init");

    fs::write(
        workspace.root.join(".beads/policy.yaml"),
        "workflow:\n  strict: false\n  statuses: [open, in_progress, blocked, closed, deferred]\n",
    )
    .expect("write workflow policy");

    let create = run_br(&workspace, ["create", "Blocked Issue"], "create_blocked");
    assert!(create.status.success(), "create failed: {}", create.stderr);
    let issue_id = parse_created_id(&create.stdout);

    let update = run_br(
        &workspace,
        ["update", &issue_id, "--status", "blocked"],
        "set_blocked_status",
    );
    assert!(update.status.success(), "update failed: {}", update.stderr);

    let stale = run_br(
        &workspace,
        ["stale", "--days", "0", "--json"],
        "stale_blocked",
    );
    assert!(stale.status.success(), "stale failed: {}", stale.stderr);

    let payload = extract_json_payload(&stale.stdout);
    let json: Vec<Value> = serde_json::from_str(&payload).expect("valid json");
    assert!(
        json.iter()
            .any(|issue| issue["id"] == issue_id && issue["status"] == "blocked"),
        "nonterminal non-ready status should be included in default stale output"
    );
}
