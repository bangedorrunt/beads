//! E2E fence for `beads_rust-short-ids-and-budget-warning-6udpw`.
//!
//! Derived short aliases — a hyphen-boundary slice of the canonical id plus
//! the abbreviated-prefix child handle — resolve through the ordinary verbs,
//! and `br create` warns before a generated id inflates the standard flywheel
//! dispatch brief past its budget.

mod common;

use common::cli::{BrWorkspace, extract_json_payload, parse_created_id, run_br};
use serde_json::Value;

fn init_with_prefix(workspace: &BrWorkspace, prefix: &str) {
    let init = run_br(workspace, ["init", "--prefix", prefix], "init");
    assert!(init.status.success(), "init failed: {}", init.stderr);
}

fn seed_issue(workspace: &BrWorkspace, title: &str, extra: &[&str], label: &str) -> String {
    let mut args = vec!["create", title];
    args.extend_from_slice(extra);
    let output = run_br(workspace, args, label);
    assert!(output.status.success(), "create failed: {}", output.stderr);
    parse_created_id(&output.stdout)
}

fn show_json(workspace: &BrWorkspace, id: &str, label: &str) -> Value {
    let output = run_br(workspace, ["show", id, "--json"], label);
    assert!(output.status.success(), "show failed: {}", output.stderr);
    serde_json::from_str(&extract_json_payload(&output.stdout)).expect("show json")
}

#[test]
fn short_alias_resolves_through_show_update_and_dep() {
    let workspace = BrWorkspace::new();
    init_with_prefix(&workspace, "elm-effect-rs");

    let slugged = seed_issue(
        &workspace,
        "Alias target",
        &["--slug", "land-ceremony"],
        "create_alias_target",
    );
    assert!(
        slugged.starts_with("elm-effect-rs-land-ceremony-"),
        "unexpected slugged id: {slugged}"
    );
    let plain = seed_issue(&workspace, "Alias dependent", &[], "create_alias_dependent");

    // Slice alias: drop the project prefix, keep the slug and hash segments.
    let slice = slugged
        .strip_prefix("elm-effect-rs-")
        .expect("slugged id carries the configured prefix")
        .to_string();
    let shown = show_json(&workspace, &slice, "show_via_slice_alias");
    assert_eq!(shown["id"].as_str(), Some(slugged.as_str()));

    // Hash alias: the final segment alone.
    let hash = plain.rsplit('-').next().expect("hash segment");
    let shown = show_json(&workspace, hash, "show_via_hash_alias");
    assert_eq!(shown["id"].as_str(), Some(plain.as_str()));

    // Update through the slice alias.
    let update = run_br(
        &workspace,
        ["update", &slice, "--priority", "1"],
        "update_via_slice_alias",
    );
    assert!(update.status.success(), "update failed: {}", update.stderr);
    let shown = show_json(&workspace, &slugged, "show_after_alias_update");
    assert_eq!(shown["priority"].as_u64(), Some(1));

    // Dependency through aliases on both ends.
    let dep = run_br(
        &workspace,
        ["dep", "add", hash, &slice],
        "dep_add_via_aliases",
    );
    assert!(dep.status.success(), "dep add failed: {}", dep.stderr);
    let listed = run_br(
        &workspace,
        ["dep", "list", &plain, "--json"],
        "dep_list_after_alias_add",
    );
    assert!(
        listed.status.success(),
        "dep list failed: {}",
        listed.stderr
    );
    let listed: Vec<Value> =
        serde_json::from_str(&extract_json_payload(&listed.stdout)).expect("dep list json");
    assert!(
        listed
            .iter()
            .any(|item| item["issue_id"] == plain && item["depends_on_id"] == slugged),
        "dependency added via aliases not listed: {listed:?}"
    );
}

#[test]
fn short_alias_resolves_child_and_close_via_slice_alias() {
    let workspace = BrWorkspace::new();
    init_with_prefix(&workspace, "elm-effect-rs");

    let slugged = seed_issue(
        &workspace,
        "Alias target",
        &["--slug", "land-ceremony"],
        "create_alias_target",
    );

    // Child alias: hash plus child suffix.
    let child = seed_issue(
        &workspace,
        "Alias child",
        &["--parent", &slugged],
        "create_alias_child",
    );
    assert_eq!(child, format!("{slugged}.1"));
    let child_alias = {
        let hash = slugged.rsplit('-').next().expect("hash segment");
        format!("{hash}.1")
    };
    let shown = show_json(&workspace, &child_alias, "show_via_child_alias");
    assert_eq!(shown["id"].as_str(), Some(child.as_str()));

    // Gate + close through a slice alias, on an issue with no open children.
    let closable = seed_issue(
        &workspace,
        "Alias closable",
        &["--slug", "close-ceremony"],
        "create_alias_closable",
    );
    let closable_slice = closable
        .strip_prefix("elm-effect-rs-")
        .expect("slugged id carries the configured prefix")
        .to_string();
    let gate = run_br(
        &workspace,
        [
            "gate",
            "report",
            &closable_slice,
            "--gate",
            "unit-test-verified",
            "--provider",
            "AliasTester",
            "--status",
            "pass",
            "--to",
            "closed",
        ],
        "gate_via_slice_alias",
    );
    assert!(gate.status.success(), "gate failed: {}", gate.stderr);
    let close = run_br(
        &workspace,
        ["close", &closable_slice, "--commit-sha", "abc1234"],
        "close_via_slice_alias",
    );
    assert!(close.status.success(), "close failed: {}", close.stderr);
    let shown = show_json(&workspace, &closable, "show_after_alias_close");
    assert_eq!(shown["status"].as_str(), Some("closed"));
}

#[test]
fn short_alias_resolves_abbreviated_prefix_handle() {
    let workspace = BrWorkspace::new();
    init_with_prefix(&workspace, "elm-effect-rs");

    let parent = seed_issue(
        &workspace,
        "Abbreviated parent",
        &["--slug", "epic-elm-effect-port"],
        "create_abbrev_parent",
    );
    let child = seed_issue(
        &workspace,
        "Abbreviated child",
        &["--parent", &parent],
        "create_abbrev_child",
    );
    assert_eq!(child, format!("{parent}.1"));

    // `eer` is `abbreviate_prefix("elm-effect-rs")` — the initials of the
    // configured prefix, per that helper's own contract (`My_Project-Name`
    // abbreviates to `mpn`).
    let shown = show_json(&workspace, "eer-1", "show_via_abbreviated_handle");
    assert_eq!(shown["id"].as_str(), Some(child.as_str()));

    // The handle only resolves real children; a missing one stays not-found.
    let missing = run_br(
        &workspace,
        ["show", "eer-99"],
        "show_missing_abbreviated_handle",
    );
    assert!(
        !missing.status.success(),
        "eer-99 should not resolve: {}",
        missing.stdout
    );
}

#[test]
fn short_alias_resolves_create_warns_on_dispatch_brief_budget_and_config_prefix() {
    let workspace = BrWorkspace::new();
    init_with_prefix(&workspace, "elm-effect-rust-dispatch-budget");

    let created = run_br(
        &workspace,
        ["create", "Long prefix bead"],
        "create_long_prefix",
    );
    assert!(
        created.status.success(),
        "create failed: {}",
        created.stderr
    );
    let id = parse_created_id(&created.stdout);
    assert!(
        id.len() >= 24,
        "fixture id should cross the warning threshold: {id}"
    );
    assert!(
        created.stderr.contains("dispatch brief"),
        "warning missing: {}",
        created.stderr
    );
    assert!(
        created.stderr.contains("config set id.prefix"),
        "warning should name the fix: {}",
        created.stderr
    );

    // The JSON envelope carries the same warning instead of only stderr.
    let json_created = run_br(
        &workspace,
        ["create", "Long prefix json bead", "--json"],
        "create_long_prefix_json",
    );
    assert!(
        json_created.status.success(),
        "json create failed: {}",
        json_created.stderr
    );
    let payload: Value =
        serde_json::from_str(&extract_json_payload(&json_created.stdout)).expect("create json");
    assert!(
        payload["budget_warning"]
            .as_str()
            .is_some_and(|warning| warning.contains("dispatch brief")),
        "json create should carry budget_warning: {}",
        json_created.stdout
    );

    // A short prefix stays quiet.
    let short = BrWorkspace::new();
    init_with_prefix(&short, "bd");
    let quiet = run_br(
        &short,
        ["create", "Short prefix bead"],
        "create_short_prefix",
    );
    assert!(quiet.status.success(), "create failed: {}", quiet.stderr);
    assert!(
        !quiet.stderr.contains("dispatch brief"),
        "short prefix should not warn: {}",
        quiet.stderr
    );

    // `br config set id.prefix` is the documented knob, and it takes effect.
    let set = run_br(
        &workspace,
        ["config", "set", "id.prefix", "zz"],
        "config_set_id_prefix",
    );
    assert!(set.status.success(), "config set failed: {}", set.stderr);
    let after = run_br(
        &workspace,
        ["create", "After prefix change"],
        "create_after_prefix_change",
    );
    assert!(after.status.success(), "create failed: {}", after.stderr);
    let new_id = parse_created_id(&after.stdout);
    assert!(
        new_id.starts_with("zz-"),
        "config set id.prefix did not take effect: {new_id}"
    );
    assert!(
        !after.stderr.contains("dispatch brief"),
        "short prefix should not warn: {}",
        after.stderr
    );
}
