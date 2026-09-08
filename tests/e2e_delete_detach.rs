//! E2E tests for `br delete --detach`.
//!
//! --detach drops every dependency edge touching the listed issues (both
//! directions) then tombstones them. Dependents outside the explicit list
//! are orphaned, never deleted — the safe alternative to --cascade when the
//! doomed set chains into unrelated work.

mod common;

use common::cli::{BrWorkspace, extract_json_payload, run_br};
use serde_json::Value;

fn create_id(workspace: &BrWorkspace, title: &str, label: &str) -> String {
    let create = run_br(workspace, ["create", title, "--json"], label);
    assert!(
        create.status.success(),
        "create failed: {}",
        create.stderr
    );
    let issue: Value =
        serde_json::from_str(&extract_json_payload(&create.stdout)).expect("create json");
    issue["id"].as_str().expect("issue id").to_string()
}

fn show_status(workspace: &BrWorkspace, id: &str, label: &str) -> Value {
    let show = run_br(workspace, ["show", id, "--json"], label);
    assert!(show.status.success(), "show failed: {}", show.stderr);
    serde_json::from_str(&extract_json_payload(&show.stdout)).expect("show json")
}

/// Chain A <- B <- C (B depends on A, C depends on B). Deleting B with
/// --detach must tombstone only B, orphan A and C alive, and drop both
/// edges. Each removed edge is printed.
#[test]
fn e2e_delete_detach_drops_edges_and_spares_unrelated_work() {
    let _log = common::test_log("e2e_delete_detach_drops_edges_and_spares_unrelated_work");
    let workspace = BrWorkspace::new();

    let init = run_br(&workspace, ["init"], "init");
    assert!(init.status.success(), "init failed: {}", init.stderr);

    let a = create_id(&workspace, "doomed A", "create_a");
    let c = create_id(&workspace, "unrelated C", "create_c");
    let mid = create_id(&workspace, "middle B", "create_mid");

    let dep1 = run_br(&workspace, ["dep", "add", &mid, &a], "dep_mid_a");
    assert!(dep1.status.success(), "dep add failed: {}", dep1.stderr);
    let dep2 = run_br(&workspace, ["dep", "add", &c, &mid], "dep_c_mid");
    assert!(dep2.status.success(), "dep add failed: {}", dep2.stderr);

    // Plain delete still refuses while dependents exist.
    let blocked = run_br(&workspace, ["delete", &mid], "delete_blocked");
    assert!(
        blocked.stdout.contains("Use --detach"),
        "preview should advertise --detach: {}",
        blocked.stdout
    );

    let delete = run_br(&workspace, ["delete", &mid, "--detach"], "delete_detach");
    assert!(
        delete.status.success(),
        "detach failed: {}",
        delete.stderr
    );
    assert!(
        delete.stdout.contains("Detached 2 edge(s)"),
        "must report both detached edges: {}",
        delete.stdout
    );
    assert!(
        delete.stdout.contains(&format!("{mid} -> {a}"))
            && delete.stdout.contains(&format!("{c} -> {mid}")),
        "must print each removed edge: {}",
        delete.stdout
    );

    assert_eq!(show_status(&workspace, &mid, "show_mid")["status"], "tombstone");
    assert_eq!(show_status(&workspace, &a, "show_a")["status"], "open");
    let survivor_c = show_status(&workspace, &c, "show_c");
    assert_eq!(survivor_c["status"], "open");
    assert!(
        survivor_c
            .get("dependencies")
            .is_none_or(|d| d.as_array().is_some_and(|d| d.is_empty())),
        "survivor's edge to the deleted issue must be gone: {}",
        survivor_c.get("dependencies").cloned().unwrap_or(Value::Null)
    );
}

/// --detach composes with --dry-run: nothing mutates.
#[test]
fn e2e_delete_detach_dry_run_changes_nothing() {
    let _log = common::test_log("e2e_delete_detach_dry_run_changes_nothing");
    let workspace = BrWorkspace::new();

    let init = run_br(&workspace, ["init"], "init");
    assert!(init.status.success(), "init failed: {}", init.stderr);

    let a = create_id(&workspace, "keeper", "create_a");
    let b = create_id(&workspace, "dependent", "create_b");
    let dep = run_br(&workspace, ["dep", "add", &b, &a], "dep_add");
    assert!(dep.status.success(), "dep add failed: {}", dep.stderr);

    let dry = run_br(
        &workspace,
        ["delete", &a, "--detach", "--dry-run"],
        "detach_dry_run",
    );
    assert!(dry.status.success(), "dry-run failed: {}", dry.stderr);
    assert!(
        dry.stdout.contains("Would detach 1 edge(s)"),
        "dry-run should preview the edge: {}",
        dry.stdout
    );

    assert_eq!(show_status(&workspace, &a, "show_a")["status"], "open");
    assert_eq!(
        show_status(&workspace, &b, "show_b")["dependencies"]
            .as_array()
            .map(|d| d.len()),
        Some(1),
        "dry-run must not remove the edge"
    );}

/// --detach conflicts with --cascade and --force at the clap layer.
#[test]
fn e2e_delete_detach_conflicts_with_cascade_and_force() {
    let _log = common::test_log("e2e_delete_detach_conflicts_with_cascade_and_force");
    let workspace = BrWorkspace::new();

    let init = run_br(&workspace, ["init"], "init");
    assert!(init.status.success(), "init failed: {}", init.stderr);
    let a = create_id(&workspace, "whatever", "create_a");

    for flag in ["--cascade", "--force"] {
        let run = run_br(
            &workspace,
            ["delete", &a, "--detach", flag],
            "detach_conflict",
        );
        assert!(
            !run.status.success(),
            "--detach with {flag} should be rejected"
        );
    }
}
