//! E2E fence for `beads_rust-assignee-pin-normalized-rd8ch`.
//!
//! In a project with a spawn registry (`.flywheel/agent-names.json`),
//! `br update --assignee` folds a registered herdr pane name to its mail
//! identity pin, refuses an unregistered herdr-shaped name with the fix
//! named, and stores pins (and non-herdr free strings) verbatim. `br show`
//! renders both the stored value and how it resolves.

mod common;

use common::cli::{BrWorkspace, extract_json_payload, parse_created_id, run_br};

const REGISTRY: &str = r#"[
  {
    "pane_id": "w1:p1",
    "name": "flywheel-demo-oc",
    "mail_identity": "AmberFox",
    "skills_injected": false
  },
  {
    "pane_id": "w1:p2",
    "name": "flywheel-demo-qa",
    "mail_identity": "CalmLantern",
    "skills_injected": false
  }
]"#;

fn init_workspace_with_registry() -> BrWorkspace {
    let workspace = BrWorkspace::new();
    let init = run_br(&workspace, ["init"], "init");
    assert!(init.status.success(), "init failed: {}", init.stderr);
    let flywheel_dir = workspace.root.join(".flywheel");
    std::fs::create_dir_all(&flywheel_dir).expect("create .flywheel");
    std::fs::write(flywheel_dir.join("agent-names.json"), REGISTRY).expect("write registry");
    workspace
}

fn seed_issue(workspace: &BrWorkspace, title: &str, label: &str) -> String {
    let output = run_br(workspace, ["create", title], label);
    assert!(output.status.success(), "create failed: {}", output.stderr);
    parse_created_id(&output.stdout)
}

fn stored_assignee(workspace: &BrWorkspace, id: &str, label: &str) -> Option<String> {
    let output = run_br(workspace, ["show", id, "--json"], label);
    assert!(output.status.success(), "show failed: {}", output.stderr);
    let payload: serde_json::Value =
        serde_json::from_str(&extract_json_payload(&output.stdout)).expect("show json");
    payload["assignee"].as_str().map(str::to_string)
}

#[test]
fn assignee_pin_normalized_herdr_name_folds_to_pin() {
    let workspace = init_workspace_with_registry();
    let id = seed_issue(&workspace, "Fold target", "create_fold_target");

    let update = run_br(
        &workspace,
        ["update", &id, "--assignee", "flywheel-demo-oc"],
        "update_with_herdr_name",
    );
    assert!(update.status.success(), "update failed: {}", update.stderr);
    assert_eq!(
        stored_assignee(&workspace, &id, "show_after_fold").as_deref(),
        Some("AmberFox"),
        "a registered herdr name must store its pin"
    );
}

#[test]
fn assignee_pin_normalized_unknown_herdr_name_refused_with_fix() {
    let workspace = init_workspace_with_registry();
    let id = seed_issue(&workspace, "Refuse target", "create_refuse_target");

    let update = run_br(
        &workspace,
        ["update", &id, "--assignee", "flywheel-ghost-oc"],
        "update_unknown_herdr",
    );
    assert!(
        !update.status.success(),
        "an unregistered herdr-shaped name must be refused: {}",
        update.stdout
    );
    let transcript = format!("{}{}", update.stdout, update.stderr);
    assert!(
        transcript.contains("agent-names.json") && transcript.contains("pin"),
        "the refusal must name the fix: {transcript}"
    );
    assert_eq!(
        stored_assignee(&workspace, &id, "show_after_refusal"),
        None,
        "a refused assignee must not land"
    );

    // The registry is optional: without one, the same input passes through.
    let plain = BrWorkspace::new();
    let init = run_br(&plain, ["init"], "init_plain");
    assert!(init.status.success(), "init failed: {}", init.stderr);
    let plain_id = seed_issue(&plain, "Plain target", "create_plain_target");
    let update = run_br(
        &plain,
        ["update", &plain_id, "--assignee", "flywheel-ghost-oc"],
        "update_plain",
    );
    assert!(
        update.status.success(),
        "without a registry the assignee passes through: {}",
        update.stderr
    );
    assert_eq!(
        stored_assignee(&plain, &plain_id, "show_plain").as_deref(),
        Some("flywheel-ghost-oc")
    );
}

#[test]
fn assignee_pin_normalized_pin_stored_verbatim() {
    let workspace = init_workspace_with_registry();
    let id = seed_issue(&workspace, "Verbatim target", "create_verbatim_target");

    // A registered pin stores exactly as passed.
    let update = run_br(
        &workspace,
        ["update", &id, "--assignee", "CalmLantern"],
        "update_registered_pin",
    );
    assert!(update.status.success(), "update failed: {}", update.stderr);
    assert_eq!(
        stored_assignee(&workspace, &id, "show_after_pin").as_deref(),
        Some("CalmLantern")
    );

    // A free string that is not herdr-shaped (a human name) stores verbatim
    // too: the strict dialect exists for herdr pane names, not people.
    let update = run_br(
        &workspace,
        ["update", &id, "--assignee", "alice"],
        "update_free_string",
    );
    assert!(update.status.success(), "update failed: {}", update.stderr);
    assert_eq!(
        stored_assignee(&workspace, &id, "show_after_free").as_deref(),
        Some("alice")
    );

    // `--assignee` wins over `--claim`'s actor default, so the claim recipe
    // stores the pin rather than the pane name.
    let claimed = seed_issue(&workspace, "Claim target", "create_claim_target");
    let update = run_br(
        &workspace,
        ["update", &claimed, "--claim", "--assignee", "AmberFox"],
        "update_claim_with_pin",
    );
    assert!(update.status.success(), "update failed: {}", update.stderr);
    assert_eq!(
        stored_assignee(&workspace, &claimed, "show_after_claim").as_deref(),
        Some("AmberFox"),
        "an explicit --assignee must win over --claim's actor default"
    );
}

#[test]
fn assignee_pin_normalized_show_describes_resolution() {
    let workspace = init_workspace_with_registry();
    let id = seed_issue(&workspace, "Describe target", "create_describe_target");

    let update = run_br(
        &workspace,
        ["update", &id, "--assignee", "flywheel-demo-qa"],
        "update_for_describe",
    );
    assert!(update.status.success(), "update failed: {}", update.stderr);

    let shown = run_br(&workspace, ["show", &id], "show_describe_text");
    assert!(shown.status.success(), "show failed: {}", shown.stderr);
    assert!(
        shown
            .stdout
            .contains("Assignee: CalmLantern (herdr flywheel-demo-qa, pane w1:p2)"),
        "show must render the pin and its pane: {}",
        shown.stdout
    );

    // A stored free string has no resolution and stays plain.
    let update = run_br(
        &workspace,
        ["update", &id, "--assignee", "alice"],
        "update_describe_free",
    );
    assert!(update.status.success(), "update failed: {}", update.stderr);
    let shown = run_br(&workspace, ["show", &id], "show_describe_plain");
    assert!(shown.status.success(), "show failed: {}", shown.stderr);
    assert!(
        shown.stdout.contains("Assignee: alice\n"),
        "a free string must render plainly: {}",
        shown.stdout
    );
}
