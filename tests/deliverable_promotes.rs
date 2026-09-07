//! Beads ADR-0005 §§2–3: typed deliverable + promotes on create.
//!
//! - `deliverable` is `diff|report`, set at creation, immutable after.
//! - `br create --promotes <id>` is an advisory link: visible in show,
//!   never gates readiness.
//!
//! <!-- governed-by: ADR-0005 -->

mod common;

use common::cli::{BrWorkspace, parse_created_id, run_br};
use serde_json::Value;

fn setup() -> BrWorkspace {
    let ws = BrWorkspace::new();
    let init = run_br(&ws, ["init"], "init");
    assert!(init.status.success(), "init failed: {}", init.stderr);
    ws
}

fn show_json(ws: &BrWorkspace, id: &str) -> Value {
    let show = run_br(ws, ["show", id, "--json"], "show");
    assert!(show.status.success(), "show failed: {}", show.stderr);
    serde_json::from_str(&show.stdout).expect("show --json must parse")
}

fn first_issue(v: Value) -> Value {
    if let Value::Array(arr) = v {
        arr.into_iter().next().expect("show returned empty array")
    } else {
        v
    }
}

#[test]
fn deliverable_promotes_default_diff() {
    let ws = setup();
    let create = run_br(&ws, ["create", "plain bead", "-p", "2"], "create");
    assert!(create.status.success(), "create failed: {}", create.stderr);
    let id = parse_created_id(&create.stdout);
    let issue = first_issue(show_json(&ws, &id));
    assert_eq!(
        issue.get("deliverable").and_then(Value::as_str),
        Some("diff"),
        "default deliverable must be diff"
    );
}

#[test]
fn deliverable_promotes_create_report_shows_typed() {
    let ws = setup();
    let create = run_br(
        &ws,
        ["create", "survey", "-p", "2", "--deliverable", "report"],
        "create_report",
    );
    assert!(create.status.success(), "create failed: {}", create.stderr);
    let id = parse_created_id(&create.stdout);
    let issue = first_issue(show_json(&ws, &id));
    assert_eq!(
        issue.get("deliverable").and_then(Value::as_str),
        Some("report"),
        "report bead must show deliverable=report"
    );
}

#[test]
fn deliverable_promotes_invalid_rejected() {
    let ws = setup();
    let create = run_br(
        &ws,
        ["create", "bogus", "-p", "2", "--deliverable", "bogus"],
        "create_bogus",
    );
    assert!(
        !create.status.success(),
        "invalid deliverable must be refused"
    );
}

#[test]
fn deliverable_promotes_retype_refused() {
    let ws = setup();
    let create = run_br(
        &ws,
        ["create", "survey", "-p", "2", "--deliverable", "report"],
        "create_report",
    );
    assert!(create.status.success(), "create failed: {}", create.stderr);
    let id = parse_created_id(&create.stdout);
    let retype = run_br(&ws, ["update", &id, "--deliverable", "diff"], "retype");
    assert!(
        !retype.status.success(),
        "retyping deliverable after creation must be refused"
    );
    let issue = first_issue(show_json(&ws, &id));
    assert_eq!(
        issue.get("deliverable").and_then(Value::as_str),
        Some("report"),
        "refused retype must leave deliverable unchanged"
    );
}

#[test]
fn deliverable_promotes_link_visible_without_ready_gating() {
    let ws = setup();
    let a = run_br(&ws, ["create", "finding", "-p", "2"], "create_a");
    assert!(a.status.success(), "create A failed: {}", a.stderr);
    let id_a = parse_created_id(&a.stdout);
    let b = run_br(
        &ws,
        ["create", "follow-on", "-p", "2", "--promotes", &id_a],
        "create_b",
    );
    assert!(b.status.success(), "create B failed: {}", b.stderr);
    let id_b = parse_created_id(&b.stdout);
    let issue_b = first_issue(show_json(&ws, &id_b));
    assert_eq!(
        issue_b.get("promotes").and_then(Value::as_str),
        Some(id_a.as_str()),
        "promotes link must be visible in show"
    );
    // Advisory, not a dependency edge: no blocks/parent edge is created.
    let deps = issue_b.get("dependencies").and_then(Value::as_array);
    assert!(
        deps.is_none_or(|d| d.is_empty()),
        "promotes must not create a dependency edge: {:?}",
        issue_b.get("dependencies")
    );
    // ...and readiness is unaffected: ready output is identical whether
    // the bead carries the link or not.
    let ready_linked = run_br(&ws, ["ready", "--json"], "ready_linked");
    assert!(ready_linked.status.success(), "ready failed");
    let c = run_br(&ws, ["create", "plain", "-p", "2"], "create_plain");
    assert!(c.status.success(), "create failed: {}", c.stderr);
    let ready_plain = run_br(&ws, ["ready", "--json"], "ready_plain");
    assert!(ready_plain.status.success(), "ready failed");
    assert_eq!(
        ready_linked.stdout, ready_plain.stdout,
        "promotes link must not change the ready set"
    );
}
