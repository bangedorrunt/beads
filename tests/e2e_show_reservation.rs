// governed-by: ADR-0001
//! Wave 3 / Layer 3: `br show` displays the toron reservation holder/paths
//! READ-ONLY (beads_rust-wave3-pin-reservation-jxsyt part 2).
//!
//! Toron remains the grantor — br only renders what an offline snapshot (the
//! same JSON/JSONL shape `br coordination status --reservations` consumes)
//! reports. No lease-granting, no daemon contact, no mutation: a missing or
//! unreadable snapshot simply omits the block.

use std::fs;

use serde_json::{Value, json};

mod common;

use common::cli::{BrWorkspace, extract_json_payload, run_br};

fn seed_one_issue(workspace: &BrWorkspace) -> String {
    let init = run_br(workspace, ["init"], "init");
    assert!(init.status.success(), "init failed: {}", init.stderr);
    let create = run_br(
        workspace,
        ["create", "reservation display fixture", "--json"],
        "create",
    );
    assert!(create.status.success(), "create failed: {}", create.stderr);
    let payload: Value =
        serde_json::from_str(&extract_json_payload(&create.stdout)).expect("created issue json");
    // Pure-success create prints a bare object; batch shapes print arrays.
    if let Some(id) = payload.get("id").and_then(Value::as_str) {
        return id.to_string();
    }
    payload[0]["id"].as_str().expect("issue id").to_string()
}

fn write_reservation_snapshot(workspace: &BrWorkspace, body: &Value) -> String {
    let path = workspace.root.join("reservations.json");
    fs::write(&path, body.to_string()).expect("write reservation snapshot");
    path.to_string_lossy().into_owned()
}

#[test]
fn show_reservation_block_reports_holder_and_paths_from_snapshot() {
    let workspace = BrWorkspace::new();
    let id = seed_one_issue(&workspace);

    // The bead's own reason/thread carries the issue id so the shared matcher
    // attaches this lease to this issue.
    let snapshot = json!({
        "reservations": [
            {
                "holder": "GreenStream",
                "path_pattern": "src/cli/**",
                "exclusive": true,
                "reason": format!("{id} scope"),
                "expires_ts": "2099-01-01T00:00:00Z",
                "released_ts": null,
                "thread_id": id
            },
            {
                "holder": "OtherAgent",
                "path_pattern": "docs/**",
                "exclusive": false,
                "reason": "unrelated lease",
                "expires_ts": "2099-01-01T00:00:00Z",
                "released_ts": null,
                "thread_id": "other-bead"
            }
        ]
    });
    let snapshot_path = write_reservation_snapshot(&workspace, &snapshot);

    let result = run_br(
        &workspace,
        ["show", &id, "--json", "--reservations", &snapshot_path],
        "show_with_reservations",
    );
    assert!(result.status.success(), "show failed: {}", result.stderr);
    let payload: Value =
        serde_json::from_str(&extract_json_payload(&result.stdout)).expect("show json");
    assert_eq!(payload.as_array().map(Vec::len), Some(1));

    let reservation = &payload[0]["reservation"];
    let reservation = reservation
        .as_object()
        .expect("reservation block present in --json output");

    assert_eq!(
        reservation.get("state").and_then(Value::as_str),
        Some("active"),
        "matched live lease reports active"
    );
    assert_eq!(
        reservation.get("holder").and_then(Value::as_str),
        Some("GreenStream")
    );
    let paths = reservation
        .get("paths")
        .and_then(Value::as_array)
        .expect("paths array");
    assert_eq!(paths.len(), 1, "only the matching lease is listed");
    assert_eq!(
        paths[0],
        json!({"path_pattern": "src/cli/**", "exclusive": true})
    );

    // Read-only contract: no grant/mutation surface is exposed.
    assert!(
        reservation.get("release_command").is_none() && reservation.get("acquire").is_none(),
        "display must not carry lease-mutation affordances"
    );
}

#[test]
fn show_without_reservation_flag_omits_the_block() {
    let workspace = BrWorkspace::new();
    let id = seed_one_issue(&workspace);
    let result = run_br(&workspace, ["show", &id, "--json"], "show_plain");
    assert!(result.status.success());
    let payload: Value =
        serde_json::from_str(&extract_json_payload(&result.stdout)).expect("show json");
    assert!(
        payload[0].get("reservation").is_none(),
        "no snapshot supplied → no reservation key"
    );
}

#[test]
fn show_reservation_missing_or_expired_states_are_explicit() {
    let workspace = BrWorkspace::new();
    let id = seed_one_issue(&workspace);

    // Snapshot with no lease matching this issue.
    let empty = write_reservation_snapshot(
        &workspace,
        &json!({
            "reservations": [
                {
                    "holder": "OtherAgent",
                    "path_pattern": "docs/**",
                    "exclusive": false,
                    "reason": "unrelated",
                    "expires_ts": "2099-01-01T00:00:00Z",
                    "released_ts": null,
                    "thread_id": "other-bead"
                }
            ]
        }),
    );
    let result = run_br(
        &workspace,
        ["show", &id, "--json", "--reservations", &empty],
        "show_no_match",
    );
    assert!(result.status.success());
    let payload: Value =
        serde_json::from_str(&extract_json_payload(&result.stdout)).expect("show json");
    let state = payload[0]["reservation"]["state"].as_str().expect("state");
    assert_eq!(
        state, "no_reservation",
        "explicit state, never silent absence"
    );

    // Expired lease on this issue.
    let expired = write_reservation_snapshot(
        &workspace,
        &json!({
            "reservations": [
                {
                    "holder": "GhostHolder",
                    "path_pattern": "src/**",
                    "exclusive": true,
                    "reason": format!("{id} abandoned"),
                    "expires_ts": "2020-01-01T00:00:00Z",
                    "released_ts": "2020-01-02T00:00:00Z",
                    "thread_id": id
                }
            ]
        }),
    );
    let result = run_br(
        &workspace,
        ["show", &id, "--json", "--reservations", &expired],
        "show_expired",
    );
    assert!(result.status.success());
    let payload: Value =
        serde_json::from_str(&extract_json_payload(&result.stdout)).expect("show json");
    let reservation = &payload[0]["reservation"];
    assert_eq!(
        reservation["state"].as_str(),
        Some("expired"),
        "expired lease names its holder for reclaim decisions"
    );
    assert_eq!(reservation["holder"].as_str(), Some("GhostHolder"));
}
