use crate::common::cli::{BrWorkspace, run_br};
mod common;

#[test]
fn test_soft_defer_behavior() {
    let workspace = BrWorkspace::new();
    run_br(&workspace, ["init"], "init");

    let create = run_br(&workspace, ["create", "Soft Defer"], "create");
    let id_line = create.stdout.lines().next().unwrap();
    let id = id_line
        .split_whitespace()
        .nth(2)
        .unwrap()
        .trim_end_matches(':');

    let update = run_br(&workspace, ["update", id, "--defer=+1d"], "update");
    assert!(update.status.success());

    let show = run_br(&workspace, ["show", id, "--json"], "show");
    let json: serde_json::Value = serde_json::from_str(&show.stdout).expect("parse json");
    let issue_details = if json.is_array() { &json[0] } else { &json };
    let status = issue_details["status"].as_str().expect("status string");
    assert_eq!(status, "open", "Status should remain open");

    let ready = run_br(&workspace, ["ready"], "ready");
    assert!(
        !ready.stdout.contains(id),
        "Soft deferred issue should be excluded from ready"
    );

    let undefer = run_br(&workspace, ["undefer", id], "undefer");
    println!("Undefer output: {}", undefer.stdout);

    let _show_after = run_br(&workspace, ["show", id], "show_after");

    let show_json = run_br(&workspace, ["show", id, "--json"], "show_json");
    let json: serde_json::Value = serde_json::from_str(&show_json.stdout).unwrap();
    let issue = if json.is_array() { &json[0] } else { &json };

    assert!(
        issue.get("defer_until").is_none() || issue["defer_until"].is_null(),
        "defer_until should be null/missing, got: {:?}",
        issue.get("defer_until")
    );
}

#[test]
fn test_soft_defer_preserves_in_progress_status() {
    let workspace = BrWorkspace::new();
    run_br(&workspace, ["init"], "init");

    let create = run_br(
        &workspace,
        ["create", "Soft Defer In Progress", "--json"],
        "create",
    );
    assert!(create.status.success());
    let created: serde_json::Value = serde_json::from_str(&create.stdout).expect("parse json");
    let id = created["id"].as_str().expect("issue id");

    let update = run_br(
        &workspace,
        ["update", id, "--status", "in_progress", "--defer=+1d"],
        "update_soft_defer_in_progress",
    );
    assert!(update.status.success(), "update failed: {}", update.stderr);

    let undefer = run_br(&workspace, ["undefer", id, "--json"], "undefer");
    assert!(
        undefer.status.success(),
        "undefer failed: {}",
        undefer.stderr
    );

    let show_json = run_br(&workspace, ["show", id, "--json"], "show_json");
    let json: serde_json::Value = serde_json::from_str(&show_json.stdout).expect("parse json");
    let issue = if json.is_array() { &json[0] } else { &json };

    assert_eq!(
        issue["status"].as_str(),
        Some("in_progress"),
        "soft-deferred in-progress issue should stay in progress"
    );
    assert!(
        issue.get("defer_until").is_none() || issue["defer_until"].is_null(),
        "defer_until should be cleared, got: {:?}",
        issue.get("defer_until")
    );
}
