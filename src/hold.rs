// governed-by: ADR-0005
//! Captain hold lifecycle (beads ADR-0005 §4).
//!
//! Pure core: bind one open corr per hold row, open hold blocks close on
//! every path, clears only on answer/`--verdict` with the resolving corr in
//! the event log, expiry re-surfaces never drops. Storage/CLI are the shell.

use chrono::{DateTime, Utc};

/// A captain hold row. One corr, one row; a second corr is a second row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptainHold {
    pub issue_id: String,
    pub corr: String,
    pub kind: String,
    pub open: bool,
    pub expires_at: Option<DateTime<Utc>>,
    pub resolving_corr: Option<String>,
    /// Set when an expiry was re-surfaced (expiry re-surfaces, never drops).
    pub resurfaced: bool,
}

impl CaptainHold {
    #[must_use]
    pub fn new(issue_id: &str, corr: &str) -> Self {
        Self {
            issue_id: issue_id.to_string(),
            corr: corr.to_string(),
            kind: "captain".to_string(),
            open: true,
            expires_at: None,
            resolving_corr: None,
            resurfaced: false,
        }
    }
}

/// Bind a hold row. Same open corr twice is refused (never overwrite);
/// a different corr is a second row (caller pushes the returned row).
#[must_use = "the returned row must be stored"]
pub fn bind_hold(
    existing: &[CaptainHold],
    issue_id: &str,
    corr: &str,
) -> Result<CaptainHold, String> {
    let duplicate = existing
        .iter()
        .any(|h| h.issue_id == issue_id && h.corr == corr && h.open);
    if duplicate {
        return Err(format!(
            "captain hold already open for {corr}: second corr binds a second row, never overwrite"
        ));
    }
    Ok(CaptainHold::new(issue_id, corr))
}

/// True while ANY captain hold row is open: close refused on every path
/// (verdict, teardown, kill, TTL, force, bypass).
#[must_use]
pub fn close_blocked_by_hold(holds: &[CaptainHold]) -> bool {
    holds.iter().any(|h| h.kind == "captain" && h.open)
}

/// Clear holds whose bound corr resolves via answer/`--verdict`.
/// Returns the resolving corr name recorded per cleared row.
/// A non-matching corr clears nothing.
#[must_use = "cleared corrs drive the event log"]
pub fn resolve_holds(holds: &mut [CaptainHold], resolving_corr: &str) -> Vec<String> {
    let mut resolved = Vec::new();
    for h in holds.iter_mut() {
        if h.open && h.corr == resolving_corr {
            h.open = false;
            h.resolving_corr = Some(resolving_corr.to_string());
            resolved.push(resolving_corr.to_string());
        }
    }
    resolved
}

/// Expiry re-surfaces the hold (event + flag), never drops it:
/// returns true when the hold stays open after expiry.
#[must_use]
pub fn expiry_resurfaces(hold: &mut CaptainHold, now: DateTime<Utc>) -> bool {
    let expired = hold.expires_at.is_some_and(|exp| now >= exp);
    if expired && hold.open {
        hold.resurfaced = true;
    }
    hold.open
}

#[cfg(test)]
mod storage_tests {
    use super::*;
    use crate::model::{AcShape, Blast, Deliverable, Issue, IssueType, Priority, Status};
    use crate::storage::{IssueUpdate, SqliteStorage};

    fn test_issue(id: &str) -> Issue {
        let now = Utc::now();
        Issue {
            revision: 1,
            id: id.to_string(),
            title: format!("hold test {id}"),
            status: Status::Open,
            priority: Priority(1),
            issue_type: IssueType::Task,
            created_at: now,
            updated_at: now,
            defer_until: None,
            content_hash: None,
            description: None,
            design: None,
            acceptance_criteria: None,
            notes: None,
            assignee: None,
            owner: None,
            estimated_minutes: None,
            created_by: None,
            closed_at: None,
            close_reason: None,
            closed_by_session: None,
            due_at: None,
            external_ref: None,
            source_system: None,
            source_repo: None,
            source_repo_path: None,
            agent_context: None,
            deleted_at: None,
            deleted_by: None,
            delete_reason: None,
            original_type: None,
            verify: Some("cargo test captain_hold".to_string()),
            principles: vec![crate::model::PrincipleCitation {
                name: "boundary-discipline".to_string(),
                decision: "test fixture citation".to_string(),
            }],
            wave: None,
            pin: None,
            commit_sha: None,
            close_verdict: None,
            ac_shape: AcShape::Checkable,
            blast: Blast::Normal,
            deliverable: Deliverable::Diff,
            promotes: None,
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
        }
    }

    fn close_update() -> IssueUpdate {
        IssueUpdate {
            status: Some(Status::Closed),
            ..Default::default()
        }
    }

    #[test]
    fn captain_hold_storage_bind_list_resolve_roundtrip() {
        let mut storage = SqliteStorage::open_memory().expect("open memory");
        storage
            .create_issue(&test_issue("holdcase-1"), "tester")
            .expect("create");
        let row = storage
            .bind_captain_hold("holdcase-1", "corr-a", None, "tester")
            .expect("bind");
        assert_eq!(row.corr, "corr-a");
        assert!(
            storage
                .bind_captain_hold("holdcase-1", "corr-a", None, "tester")
                .is_err()
        );
        storage
            .bind_captain_hold("holdcase-1", "corr-b", None, "tester")
            .expect("second row");
        assert_eq!(
            storage.open_captain_holds("holdcase-1").expect("list").len(),
            2
        );
        let resolved = storage
            .resolve_captain_hold("holdcase-1", "corr-a", "tester")
            .expect("resolve");
        assert_eq!(resolved, vec!["corr-a".to_string()]);
        assert_eq!(
            storage.open_captain_holds("holdcase-1").expect("list").len(),
            1
        );
        assert!(
            storage
                .resolve_captain_hold("holdcase-1", "corr-zzz", "tester")
                .expect("noop")
                .is_empty()
        );
    }

    #[test]
    fn captain_hold_storage_close_refused_on_every_path() {
        let mut storage = SqliteStorage::open_memory().expect("open memory");
        storage
            .create_issue(&test_issue("holdcase-2"), "tester")
            .expect("create");
        storage
            .bind_captain_hold("holdcase-2", "corr-a", None, "tester")
            .expect("bind");
        let err = storage
            .update_issues_atomically(&[("holdcase-2".to_string(), close_update())], "tester")
            .expect_err("held bead must refuse close");
        assert!(
            err.to_string().contains("captain-held"),
            "unexpected error: {err}"
        );
        storage
            .resolve_captain_hold("holdcase-2", "corr-a", "tester")
            .expect("resolve");
        storage
            .update_issues_atomically(&[("holdcase-2".to_string(), close_update())], "tester")
            .expect("close after resolve");
    }

    #[test]
    fn captain_hold_storage_expiry_resurfaces_never_drops() {
        let mut storage = SqliteStorage::open_memory().expect("open memory");
        storage
            .create_issue(&test_issue("holdcase-3"), "tester")
            .expect("create");
        let past = (Utc::now() - chrono::Duration::seconds(1)).to_rfc3339();
        storage
            .bind_captain_hold("holdcase-3", "corr-a", Some(&past), "tester")
            .expect("bind");
        // The bind-time sweep runs before the INSERT, so flagging happens
        // on the next sweep; the hold stays open throughout.
        let count = storage
            .resurface_expired_captain_holds("tester")
            .expect("sweep");
        assert_eq!(count, 1);
        let holds = storage.open_captain_holds("holdcase-3").expect("list");
        assert_eq!(holds.len(), 1, "expiry must never drop the hold");
        assert!(holds[0].resurfaced);
        let count = storage
            .resurface_expired_captain_holds("tester")
            .expect("sweep");
        assert_eq!(count, 0);
        let err = storage
            .update_issues_atomically(&[("holdcase-3".to_string(), close_update())], "tester")
            .expect_err("expired hold still blocks close");
        assert!(
            err.to_string().contains("captain-held"),
            "unexpected error: {err}"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn open_hold(issue: &str, corr: &str) -> CaptainHold {
        CaptainHold::new(issue, corr)
    }

    #[test]
    fn captain_hold_second_corr_is_second_row() {
        let existing = vec![open_hold("case-1", "corr-a")];
        let row = bind_hold(&existing, "case-1", "corr-b").expect("second corr must bind");
        assert_eq!(row.corr, "corr-b");
        assert!(row.open);
    }

    #[test]
    fn captain_hold_same_corr_duplicate_refused() {
        let existing = vec![open_hold("case-1", "corr-a")];
        assert!(bind_hold(&existing, "case-1", "corr-a").is_err());
    }

    #[test]
    fn captain_hold_open_blocks_close() {
        assert!(close_blocked_by_hold(&[open_hold("case-1", "corr-a")]));
        let mut h = open_hold("case-1", "corr-a");
        h.open = false;
        assert!(!close_blocked_by_hold(&[h]));
        assert!(!close_blocked_by_hold(&[]));
    }

    #[test]
    fn captain_hold_resolves_on_verdict_corr() {
        let mut holds = vec![open_hold("case-1", "corr-a"), open_hold("case-1", "corr-b")];
        let resolved = resolve_holds(&mut holds, "corr-a");
        assert_eq!(resolved, vec!["corr-a".to_string()]);
        assert!(!holds[0].open);
        assert_eq!(holds[0].resolving_corr.as_deref(), Some("corr-a"));
        assert!(holds[1].open);
        // Non-matching corr clears nothing.
        let resolved = resolve_holds(&mut holds, "corr-zzz");
        assert!(resolved.is_empty());
        assert!(holds[1].open);
    }

    #[test]
    fn captain_hold_expiry_resurfaces_never_drops() {
        let mut h = open_hold("case-1", "corr-a");
        h.expires_at = Some(Utc::now() - Duration::seconds(1));
        assert!(expiry_resurfaces(&mut h, Utc::now()));
        assert!(h.open, "expiry must never drop the hold");
        assert!(h.resurfaced);
    }
}
