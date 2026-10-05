//! `br land` acceptance: the paved close ceremony (beads_rust-br-land-ceremony-de6fu).
//!
//! One call records the verdict gate and closes the bead; the raw verbs stay
//! available. Covered here, against the real binary:
//! - gate+close in one call (gate derived, sha cited, note binds);
//! - sha-token validation refusing a glued `;` (the `sha=<sha>;` UNBOUND trap);
//! - `close --dry-run` naming each missing precondition (sha, gate row,
//!   bindings) and `land --dry-run` naming the plan;
//! - the `--release-leases` failure path stating the close already landed.

// governed-by: ADR-0001

mod common;

use common::cli::{BrWorkspace, parse_created_id, parse_json_value, run_br, run_br_with_env};
use std::fs;
use std::process::Command;

fn setup_workspace_with_issue(title: &str) -> (BrWorkspace, String) {
    let workspace = BrWorkspace::new();
    let init = run_br(&workspace, ["init"], "init");
    assert!(init.status.success(), "init failed: {}", init.stderr);

    let create = run_br(
        &workspace,
        [
            "create",
            title,
            "-p",
            "2",
            "-t",
            "task",
            "-d",
            "land ceremony e2e brief",
            "--verify",
            "cargo test -p beads land_full_ceremony",
            "--principle",
            "sequence-verifiable-units — the ceremony is one call, not six remembered steps",
        ],
        "create_issue",
    );
    assert!(create.status.success(), "create failed: {}", create.stderr);
    let id = parse_created_id(&create.stdout);
    assert!(
        !id.is_empty(),
        "create must print the id: {}",
        create.stdout
    );
    (workspace, id)
}

/// Init a git repo in the workspace with one non-empty commit whose message
/// cites `id`, and return the full 40-hex sha.
fn commit_citing(workspace: &BrWorkspace, id: &str, message: &str) -> String {
    let git = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(&workspace.root)
            .output()
            .expect("git runs");
        assert!(out.status.success(), "git {args:?} failed: {out:?}");
        out
    };
    if !workspace.root.join(".git").exists() {
        git(&["init"]);
        git(&["config", "user.email", "test@example.com"]);
        git(&["config", "user.name", "test"]);
    }
    fs::write(
        workspace
            .root
            .join(format!("work-{}.txt", id.replace(['/', ':'], "_"))),
        "work\n",
    )
    .expect("write work");
    git(&["add", "."]);
    git(&["commit", "-m", message]);
    let sha_out = git(&["rev-parse", "HEAD"]);
    String::from_utf8_lossy(&sha_out.stdout).trim().to_string()
}

fn missing_text(payload: &serde_json::Value) -> String {
    payload["issues"][0]["missing"]
        .as_array()
        .expect("preview payload has a missing list")
        .iter()
        .map(|item| item.as_str().unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n")
}

fn notes_text(payload: &serde_json::Value) -> String {
    payload["issues"][0]["notes"]
        .as_array()
        .expect("preview payload has a notes list")
        .iter()
        .map(|item| item.as_str().unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n")
}

fn step<'a>(payload: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
    payload["steps"]
        .as_array()
        .expect("land payload has steps")
        .iter()
        .find(|step| step["name"] == name)
        .unwrap_or_else(|| panic!("step '{name}' missing from {}", payload["steps"]))
}

#[test]
fn land_full_ceremony_dry_run_names_missing_preconditions() {
    let _log = common::test_log("land_full_ceremony_dry_run_names_missing_preconditions");
    let (workspace, id) = setup_workspace_with_issue("Land ceremony preconditions");
    let sha = commit_citing(&workspace, &id, &format!("feat: land ceremony ({id})"));

    // No sha, no gate row yet: both are named, and the close refuses.
    let dry_no_sha = run_br(
        &workspace,
        ["close", &id, "--dry-run", "--json"],
        "dry_no_sha",
    );
    assert!(dry_no_sha.status.success(), "{}", dry_no_sha.stderr);
    let payload = parse_json_value(&dry_no_sha.stdout);
    assert_eq!(payload["dry_run"], true, "{payload}");
    assert_eq!(payload["would_close"], false, "{payload}");
    let items: Vec<&str> = payload["issues"][0]["missing"]
        .as_array()
        .expect("missing list")
        .iter()
        .map(|item| item.as_str().unwrap_or_default())
        .collect();
    assert!(
        items.iter().any(|item| item.starts_with("--commit-sha")),
        "dry-run must name the missing sha as its own precondition: {items:?}"
    );
    assert!(
        items
            .iter()
            .any(|item| item.contains("no legal PASS gate row")),
        "dry-run must name the missing gate row (bindings): {items:?}"
    );

    // With the citing sha supplied, the citation is satisfied and only the
    // gate row remains missing.
    let dry_with_sha = run_br(
        &workspace,
        ["close", &id, "--dry-run", "--commit-sha", &sha, "--json"],
        "dry_with_sha",
    );
    assert!(dry_with_sha.status.success(), "{}", dry_with_sha.stderr);
    let payload = parse_json_value(&dry_with_sha.stdout);
    let notes = notes_text(&payload);
    assert!(
        notes.contains("sha citation:") && notes.contains(&sha),
        "the cited sha must be reported as satisfied: {notes}"
    );
    let missing_items = payload["issues"][0]["missing"]
        .as_array()
        .expect("missing list");
    assert_eq!(
        missing_items.len(),
        1,
        "only the gate row remains missing once the sha is cited: {payload}"
    );
    let missing = missing_items[0].as_str().unwrap_or_default();
    assert!(
        missing.contains("no legal PASS gate row"),
        "the remaining item must be the gate row: {missing}"
    );
}

#[test]
fn land_full_ceremony_dry_run_plan_writes_nothing() {
    let _log = common::test_log("land_full_ceremony_dry_run_plan_writes_nothing");
    let (workspace, id) = setup_workspace_with_issue("Land ceremony dry plan");
    let sha = commit_citing(&workspace, &id, &format!("feat: land ceremony ({id})"));

    // land --dry-run names the plan: derived gate, note, and the steps.
    let land_dry = run_br(
        &workspace,
        ["land", &id, "--commit-sha", &sha, "--dry-run", "--json"],
        "land_dry",
    );
    assert!(land_dry.status.success(), "{}", land_dry.stderr);
    let payload = parse_json_value(&land_dry.stdout);
    assert_eq!(payload["dry_run"], true, "{payload}");
    assert_eq!(
        payload["gate"], "command-verified",
        "the only legal gate for a P2 loop-runnable bead must be derived: {payload}"
    );
    assert!(
        payload["note"].as_str().unwrap_or_default().contains(&sha),
        "the note must carry the sha token: {payload}"
    );
    for name in [
        "gate row",
        "close",
        "release leases",
        "sync",
        "captain mail",
    ] {
        step(&payload, name);
    }

    // Dry-runs wrote nothing: no gate row, bead still open.
    let gates = run_br(
        &workspace,
        ["gate", "list", &id, "--json"],
        "gate_list_after_dry",
    );
    let payload = parse_json_value(&gates.stdout);
    assert!(
        payload["history"].as_array().expect("history").is_empty(),
        "a dry-run must not record gate rows: {payload}"
    );
    let show = run_br(&workspace, ["show", &id, "--json"], "show_after_dry");
    let payload = parse_json_value(&show.stdout);
    assert_eq!(
        payload["status"], "open",
        "a dry-run must not close the bead: {payload}"
    );
}

#[test]
fn land_full_ceremony_refuses_glued_sha() {
    let _log = common::test_log("land_full_ceremony_refuses_glued_sha");
    let (workspace, id) = setup_workspace_with_issue("Land ceremony glued sha");
    let sha = commit_citing(&workspace, &id, &format!("feat: land ceremony ({id})"));

    // ---- sha-token validation refuses a glued `;` -------------------------
    let glued = format!("{sha};");
    let refused = run_br(
        &workspace,
        ["land", &id, "--commit-sha", &glued, "--json"],
        "land_glued_sha",
    );
    assert!(
        !refused.status.success(),
        "a glued sha must be refused: {}",
        refused.stdout
    );
    let transcript = format!("{}{}", refused.stdout, refused.stderr);
    assert!(
        transcript.contains("glued") && transcript.contains("UNBOUND"),
        "the refusal must name the glue trap: {transcript}"
    );
    let show = run_br(&workspace, ["show", &id, "--json"], "show_still_open");
    let payload = parse_json_value(&show.stdout);
    assert_eq!(
        payload["status"], "open",
        "the bead must stay open: {payload}"
    );
}

#[test]
fn land_full_ceremony_live_close_binds_sha_and_receipt() {
    let _log = common::test_log("land_full_ceremony_live_close_binds_sha_and_receipt");
    let (workspace, id) = setup_workspace_with_issue("Land ceremony live close");
    let sha = commit_citing(&workspace, &id, &format!("feat: land ceremony ({id})"));

    // ---- gate + close in one call ----------------------------------------
    let landed = run_br(
        &workspace,
        [
            "land",
            &id,
            "--commit-sha",
            &sha,
            "--receipt",
            "spec.txt",
            "--reason",
            "ceremony landed",
            "--json",
        ],
        "land_live",
    );
    assert!(
        landed.status.success(),
        "land must succeed: {} {}",
        landed.stdout,
        landed.stderr
    );
    let payload = parse_json_value(&landed.stdout);
    assert_eq!(payload["closed"], true, "{payload}");
    assert_eq!(payload["gate"], "command-verified", "{payload}");
    let note = payload["note"].as_str().unwrap_or_default();
    assert!(
        note.contains(&format!("sha={sha}")) && note.contains("receipt=spec.txt"),
        "the gate note must bind sha + receipt: {note}"
    );
    assert_eq!(step(&payload, "gate row")["status"], "done", "{payload}");
    assert_eq!(step(&payload, "close")["status"], "done", "{payload}");
    assert_eq!(
        step(&payload, "release leases")["status"],
        "next",
        "{payload}"
    );
    assert_eq!(step(&payload, "sync")["status"], "next", "{payload}");

    // The close is real, and the recorded verdict row carries the binding.
    let show = run_br(&workspace, ["show", &id, "--json"], "show_closed");
    let payload = parse_json_value(&show.stdout);
    assert_eq!(payload["status"], "closed", "{payload}");

    let gates = run_br(&workspace, ["gate", "list", &id, "--json"], "gate_list");
    assert!(gates.status.success(), "{}", gates.stderr);
    let payload = parse_json_value(&gates.stdout);
    let history = payload["history"].as_array().expect("gate history array");
    assert_eq!(history.len(), 1, "exactly one verdict row: {payload}");
    assert_eq!(history[0]["gate"], "command-verified", "{payload}");
    assert_eq!(history[0]["passed"], true, "{payload}");
    let note = history[0]["note"].as_str().unwrap_or_default();
    assert!(
        note.contains(&format!("sha={sha}")) && note.contains("receipt=spec.txt"),
        "the persisted row must carry the binding: {note}"
    );

    // A second dry-run now reports the bead already closed (no duplicate work).
    let dry_closed = run_br(
        &workspace,
        ["close", &id, "--dry-run", "--commit-sha", &sha, "--json"],
        "dry_closed",
    );
    assert!(dry_closed.status.success(), "{}", dry_closed.stderr);
    let payload = parse_json_value(&dry_closed.stdout);
    assert!(
        missing_text(&payload).contains("already closed"),
        "a closed bead must preview as already closed: {payload}"
    );
}

#[test]
fn land_full_ceremony_release_leases_flag() {
    let _log = common::test_log("land_full_ceremony_release_leases_flag");

    // ---- failure: the close landed, and land says so + names the retry ---
    let (workspace, id) = setup_workspace_with_issue("Land release-lease failure");
    let sha = commit_citing(&workspace, &id, &format!("feat: release failure ({id})"));
    let attempted = run_br_with_env(
        &workspace,
        [
            "land",
            &id,
            "--commit-sha",
            &sha,
            "--release-leases",
            "--project",
            "testproj",
            "--json",
        ],
        [("BR_TORON_BIN", "/bin/false"), ("TORON_AGENT", "TestPin")],
        "land_release_failure",
    );
    assert!(
        !attempted.status.success(),
        "a failed lease release must be non-zero: {}",
        attempted.stdout
    );
    let transcript = format!("{}{}", attempted.stdout, attempted.stderr);
    assert!(
        transcript.contains("close LANDED") && transcript.contains("retry"),
        "the failure must state the close landed and name the retry: {transcript}"
    );
    let show = run_br(&workspace, ["show", &id, "--json"], "show_landed");
    let payload = parse_json_value(&show.stdout);
    assert_eq!(
        payload["status"], "closed",
        "the close itself must have landed: {payload}"
    );

    // ---- success: a working toron is invoked with the ceremony's args ----
    let (workspace, id) = setup_workspace_with_issue("Land release-lease success");
    let sha = commit_citing(&workspace, &id, &format!("feat: release success ({id})"));
    let script = workspace.root.join("fake-toron.sh");
    let invocation = workspace.root.join("toron-invocation.txt");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\necho \"$@\" > '{}'\nexit 0\n",
            invocation.display()
        ),
    )
    .expect("write fake toron");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).expect("chmod");
    }
    let landed = run_br_with_env(
        &workspace,
        [
            "land",
            &id,
            "--commit-sha",
            &sha,
            "--release-leases",
            "--project",
            "testproj",
            "--json",
        ],
        [
            ("BR_TORON_BIN", script.to_str().unwrap()),
            ("TORON_AGENT", "TestPin"),
        ],
        "land_release_success",
    );
    assert!(
        landed.status.success(),
        "land with --release-leases must succeed when toron does: {} {}",
        landed.stdout,
        landed.stderr
    );
    let payload = parse_json_value(&landed.stdout);
    assert_eq!(
        step(&payload, "release leases")["status"],
        "done",
        "{payload}"
    );
    let called = fs::read_to_string(&invocation).expect("fake toron was invoked");
    assert!(
        called.contains(&format!("reserve release-by-reason {id}"))
            && called.contains("--project testproj")
            && called.contains("--as TestPin"),
        "the release must pass the bead, project, and pin: {called}"
    );
}
