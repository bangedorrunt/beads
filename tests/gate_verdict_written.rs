//! ADR-0001 §5.4: a recorded close verdict must be provable.
//!
//! `br doctor` reports `gate.verdict_orphans` for a closed issue whose
//! `close_verdict` has no PASS row. That state is reachable today (a flywheel
//! ledger bead was closed with a verdict and zero rows, and a second one the
//! same day) and it is only detectable *after* the fact, by hand. These tests
//! pin the invariant at the one place the verdict is written: the storage
//! update chokepoint.
//!
//! Fixture ids use an `issue-` prefix, not the tracker's own id shape: an id
//! shaped like a real bead inside a test reads as a citation to the
//! commit-message attribution guard.

// governed-by: ADR-0001

use beads::model::{AcShape, Blast, Deliverable, Issue, IssueType, Priority, Status};
use beads::storage::{IssueUpdate, SqliteStorage};
use chrono::{TimeZone, Utc};

const VERDICT: &str = "unit-test-verified";

fn seed_issue(storage: &mut SqliteStorage, id: &str) {
    let t = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    let issue = Issue {
        id: id.to_string(),
        title: format!("Verdict-proof issue {id}"),
        status: Status::Open,
        priority: Priority(2),
        issue_type: IssueType::Task,
        created_at: t,
        updated_at: t,
        assignee: None,
        content_hash: None,
        description: None,
        design: None,
        acceptance_criteria: None,
        notes: None,
        owner: None,
        estimated_minutes: None,
        created_by: None,
        closed_at: None,
        close_reason: None,
        closed_by_session: None,
        due_at: None,
        defer_until: None,
        external_ref: None,
        source_system: None,
        source_repo: None,
        source_repo_path: None,
        agent_context: None,
        deleted_at: None,
        deleted_by: None,
        delete_reason: None,
        original_type: None,
        verify: Some("true".to_string()),
        principles: Vec::new(),
        wave: None,
        pin: None,
        commit_sha: None,
        close_verdict: None,
        ac_shape: AcShape::Checkable,
        blast: Blast::Normal,
        deliverable: Deliverable::Diff,
        promotes: None,
        revision: 1,
        compaction_level: None,
        compacted_at: None,
        compacted_at_commit: None,
        original_size: None,
        sender: None,
        ephemeral: false,
        pinned: false,
        is_template: false,
        labels: vec![],
        dependencies: vec![],
        comments: vec![],
    };
    storage.create_issue(&issue, "seed").unwrap();
}

/// Record a scoped gate row exactly as `br gate report --to closed` does.
fn record_pass_row(storage: &SqliteStorage, id: &str, gate: &str) {
    let revision = storage.status_revision(id).unwrap();
    storage
        .record_scoped_gate_result(
            id, "open", revision, "closed", gate, "verifier", true, None, "verifier",
        )
        .unwrap();
}

fn close_with_verdict(
    storage: &mut SqliteStorage,
    id: &str,
    verdict: &str,
) -> beads::error::Result<Issue> {
    storage.update_issue(
        id,
        &IssueUpdate {
            status: Some(Status::Closed),
            closed_at: Some(Some(Utc::now())),
            close_reason: Some(Some("done".to_string())),
            commit_sha: Some(Some("deadbeef".to_string())),
            close_verdict: Some(Some(verdict.to_string())),
            ..IssueUpdate::default()
        },
        "closer",
    )
}

/// The defect: the verification column is writable without its row.
#[test]
fn gate_verdict_written_refuses_a_close_verdict_with_no_proving_row() {
    let mut storage = SqliteStorage::open_memory().unwrap();
    seed_issue(&mut storage, "issue-orphan");

    let err = close_with_verdict(&mut storage, "issue-orphan", VERDICT).unwrap_err();
    let message = err.to_string();
    assert!(
        message.contains("proving gate row"),
        "the refusal must name what is missing, got: {message}"
    );

    // Refuse means roll back: no verdict, no closed status, no row.
    let issue = storage.get_issue("issue-orphan").unwrap().unwrap();
    assert_eq!(issue.status, Status::Open, "a refused close must not land");
    assert!(issue.close_verdict.is_none());
    assert!(issue.closed_at.is_none());
    assert!(
        storage
            .get_gate_result_history("issue-orphan")
            .unwrap()
            .is_empty(),
        "a refused close must not invent a row"
    );
}

/// A row for a *different* gate does not prove the verdict that was recorded.
#[test]
fn gate_verdict_written_is_refused_when_only_another_gates_row_exists() {
    let mut storage = SqliteStorage::open_memory().unwrap();
    seed_issue(&mut storage, "issue-wrong-gate");
    record_pass_row(&storage, "issue-wrong-gate", "ci-green");

    let err = close_with_verdict(&mut storage, "issue-wrong-gate", VERDICT).unwrap_err();
    assert!(err.to_string().contains("proving gate row"), "got: {err}");
    assert_eq!(
        storage
            .get_issue("issue-wrong-gate")
            .unwrap()
            .unwrap()
            .status,
        Status::Open
    );
}

/// The honest path still works, and the row it was proved by stays put.
#[test]
fn gate_verdict_written_accepts_a_close_verdict_with_its_pass_row() {
    let mut storage = SqliteStorage::open_memory().unwrap();
    seed_issue(&mut storage, "issue-proved");
    record_pass_row(&storage, "issue-proved", VERDICT);

    let closed = close_with_verdict(&mut storage, "issue-proved", VERDICT).unwrap();
    assert_eq!(closed.status, Status::Closed);
    assert_eq!(closed.close_verdict.as_deref(), Some(VERDICT));

    let rows = storage.get_gate_result_history("issue-proved").unwrap();
    assert_eq!(rows.len(), 1, "the proving row must exist exactly once");
    assert!(rows[0].passed);
    assert_eq!(rows[0].gate, VERDICT);
    assert_eq!(rows[0].to_status, "closed");
}

/// A later, unrelated row -- the shape a second agent's commit binding takes --
/// cannot displace the row that proved this close.
#[test]
fn gate_verdict_written_row_survives_a_later_unrelated_proof() {
    let mut storage = SqliteStorage::open_memory().unwrap();
    seed_issue(&mut storage, "issue-first");
    seed_issue(&mut storage, "issue-second");
    record_pass_row(&storage, "issue-first", VERDICT);
    close_with_verdict(&mut storage, "issue-first", VERDICT).unwrap();

    record_pass_row(&storage, "issue-second", VERDICT);
    close_with_verdict(&mut storage, "issue-second", VERDICT).unwrap();

    let first = storage.get_gate_result_history("issue-first").unwrap();
    assert_eq!(first.len(), 1);
    assert!(first[0].passed);
    assert_eq!(
        storage.get_issue("issue-first").unwrap().unwrap().status,
        Status::Closed
    );
}
