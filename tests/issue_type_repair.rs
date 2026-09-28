//! A stored `issue_type` outside the current enum must not brick the row.
//!
//! The tracked ledger carried five such rows (`debt` three times,
//! `enhancement`, `not_a_real_type`). One was enough to fail every
//! `br sync --flush-only` in the repository, for every lane, because the
//! export validator re-parses each line it writes.
//!
//! The wall was the read path: `parse_issue_type` refused the row, so
//! `br update -t` — the only repair door — could not even load the row it
//! was asked to retype. That is a fixpoint: the invalid value is permanent,
//! and a permanent invalid value means a permanently unflushable ledger.
//!
//! The legacy string is not lost by reading it as the default; it stays
//! recoverable from the ledger's own git history, while the live store is
//! repaired to a value the schema accepts.
//!
//! Fixture ids use an `issue-` prefix, not the tracker's own id shape: an id
//! shaped like a real bead inside a test reads as a citation to the
//! commit-message attribution guard.

use beads::model::{AcShape, Blast, Deliverable, Issue, IssueType, Priority, Status};
use beads::storage::{Connection, IssueUpdate, SqliteStorage};
use chrono::{TimeZone, Utc};
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn seed_issue(storage: &mut SqliteStorage, id: &str) {
    let t = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    let issue = Issue {
        id: id.to_string(),
        title: format!("Legacy-type issue {id}"),
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

/// A workspace holding one issue whose stored type is a legacy spelling.
///
/// The write goes through raw SQL on purpose: no supported API can produce
/// this state any more, which is exactly why rows that already hold it need
/// a way out.
fn seed_with_legacy_stored_type(stored: &str) -> (TempDir, PathBuf) {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("beads.db");
    let mut storage = SqliteStorage::open(&db_path).expect("open storage");
    seed_issue(&mut storage, "issue-legacy-type");
    drop(storage);

    let conn = Connection::open(db_path.to_string_lossy().into_owned()).expect("open raw");
    conn.execute(&format!(
        "UPDATE issues SET issue_type = '{stored}' WHERE id = 'issue-legacy-type'"
    ))
    .expect("store the legacy type");
    let _ = conn.close();

    (dir, db_path)
}

fn stored_type(db_path: &Path, id: &str) -> String {
    let conn = Connection::open(db_path.to_string_lossy().into_owned()).expect("open raw");
    let row = conn
        .query_row(&format!("SELECT issue_type FROM issues WHERE id = '{id}'"))
        .expect("read stored type");
    let value = row
        .get(0)
        .and_then(beads::storage::SqliteValue::as_text)
        .expect("text issue_type")
        .to_string();
    let _ = conn.close();
    value
}

/// The defect: one unknown value made the row unreadable.
#[test]
fn an_unknown_stored_issue_type_still_loads() {
    let (_dir, db_path) = seed_with_legacy_stored_type("debt");
    let storage = SqliteStorage::open(&db_path).expect("open storage");

    let issue = storage
        .get_issue("issue-legacy-type")
        .expect("a legacy stored type must not make the row unreadable")
        .expect("the row is there");
    assert_eq!(
        issue.issue_type,
        IssueType::default(),
        "an unknown stored value reads as the schema default"
    );
}

/// The fixpoint: the repair door could not load the row it was asked to fix.
#[test]
fn an_explicit_retype_supersedes_an_unknown_stored_issue_type() {
    let (_dir, db_path) = seed_with_legacy_stored_type("enhancement");
    let mut storage = SqliteStorage::open(&db_path).expect("open storage");

    storage
        .update_issue(
            "issue-legacy-type",
            &IssueUpdate {
                issue_type: Some(IssueType::Chore),
                ..IssueUpdate::default()
            },
            "repairer",
        )
        .expect("an explicit retype must be allowed to supersede a stale stored value");

    assert_eq!(
        stored_type(&db_path, "issue-legacy-type"),
        "chore",
        "the retype must land in the store, not just in the returned struct"
    );
    assert_eq!(
        storage
            .get_issue("issue-legacy-type")
            .unwrap()
            .unwrap()
            .issue_type,
        IssueType::Chore
    );
}
