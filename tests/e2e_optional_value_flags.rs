//! E2E fence for beads_rust-optional-value-flags-refuse-loud-38jgj.
//!
//! `--wave` and `--defer` on `br update` are optional-value flags (they
//! document "empty clears"). Their space form (`--wave 6`, `--defer <ts>`)
//! reads the following token as an issue id instead of a value, so it must
//! refuse loudly and name the two legal forms:
//!
//! - `--flag=value` sets
//! - `--flag=`     clears
//!
//! The equals forms keep working, and a refused update must not mutate.

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

/// The space form must refuse (exit 2) and name both legal forms.
fn assert_space_form_refused(
    workspace: &BrWorkspace,
    args: &[&str],
    set_form: &str,
    clear_form: &str,
    label: &str,
) {
    let refused = run_br(workspace, args, label);
    assert!(
        !refused.status.success(),
        "{label}: space form must refuse; got stdout: {}",
        refused.stdout
    );
    assert_eq!(
        refused.status.code(),
        Some(2),
        "{label}: usage refusal exits 2: {}",
        refused.stderr
    );
    assert!(
        refused.stderr.contains(set_form),
        "{label}: refusal must name the set form: {}",
        refused.stderr
    );
    assert!(
        refused.stderr.contains(clear_form),
        "{label}: refusal must name the clear form: {}",
        refused.stderr
    );
}

#[test]
fn optional_value_flags_refuse_space_separated() {
    let workspace = BrWorkspace::new();
    init_workspace(&workspace);

    let id = create_issue(
        &workspace,
        &[
            "space form probe",
            "-d",
            "e2e brief",
            "--verify",
            "true",
            "--principle",
            "prove-it-works — e2e brief",
        ],
        "create_probe",
    );
    assert!(!id.is_empty(), "create must yield an id");

    // Seed a wave through the equals form: setting must keep working.
    let set = run_br(&workspace, ["update", &id, "--wave=3"], "wave_equals_set");
    assert!(set.status.success(), "equals set failed: {}", set.stderr);
    assert_eq!(show_json(&workspace, &id)["wave"].as_u64(), Some(3));

    // The space form refuses loudly, naming BOTH legal forms...
    assert_space_form_refused(
        &workspace,
        &["update", &id, "--wave", "6"],
        "--wave=6",
        "--wave=",
        "wave_space",
    );
    // ...and the refused update must not have mutated anything.
    assert_eq!(
        show_json(&workspace, &id)["wave"].as_u64(),
        Some(3),
        "a refused space form must not clear the wave"
    );

    // Same refusal for --defer, and it must not silently skip the defer.
    assert_space_form_refused(
        &workspace,
        &["update", &id, "--defer", "2100-01-01T00:00:00Z"],
        "--defer=2100-01-01T00:00:00Z",
        "--defer=",
        "defer_space",
    );
    assert!(
        show_json(&workspace, &id)["defer_until"].is_null(),
        "the refused form must not defer"
    );

    // Equals forms keep working: set...
    let set_defer = run_br(
        &workspace,
        ["update", &id, "--defer=2100-01-01T00:00:00Z"],
        "defer_equals_set",
    );
    assert!(
        set_defer.status.success(),
        "equals set failed: {}",
        set_defer.stderr
    );
    assert_eq!(
        show_json(&workspace, &id)["defer_until"].as_str(),
        Some("2100-01-01T00:00:00Z")
    );

    // ...and clear.
    let clear_defer = run_br(
        &workspace,
        ["update", &id, "--defer="],
        "defer_equals_clear",
    );
    assert!(
        clear_defer.status.success(),
        "equals clear failed: {}",
        clear_defer.stderr
    );
    assert!(show_json(&workspace, &id)["defer_until"].is_null());

    let clear_wave = run_br(&workspace, ["update", &id, "--wave="], "wave_equals_clear");
    assert!(
        clear_wave.status.success(),
        "equals clear failed: {}",
        clear_wave.stderr
    );
    assert!(show_json(&workspace, &id)["wave"].is_null());
}
