//! TUI effects: what a key returns, and what the world answers.
//!
//! The key handlers are pure. A key that changes the ledger returns a
//! [`TuiCmd`] instead of writing; [`interpret`] is the only place that opens
//! storage, runs the same close/delete/update cores as the CLI, and reloads
//! the list. The model changes only when the reply comes back as a
//! [`TuiMsg`], so a refused or failed write cannot leave the TUI claiming it
//! happened.

use std::path::Path;

use chrono::{DateTime, Utc};

use crate::cli::commands::{close, delete, update};
use crate::model::Issue;

use super::load_issues_from;

/// Work the fold asked for but has not done.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TuiCmd {
    /// Close one issue. `commit_sha` must cite the bead.
    Close {
        issue_id: String,
        commit_sha: String,
        reason: String,
        expected_revision: Option<u64>,
    },
    /// Tombstone one issue. Dependents refuse the delete.
    Delete {
        issue_id: String,
        reason: String,
        expected_revision: Option<u64>,
    },
    /// Rename one issue.
    EditTitle {
        issue_id: String,
        title: String,
        expected_revision: Option<u64>,
    },
    /// Re-read the workspace.
    Reload,
}

/// Where the cursor lands after a reload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReloadSelection {
    /// Keep the same bead selected (plain refresh, rename).
    KeepById,
    /// Keep the same row index (close/delete move the row away).
    KeepIndex,
}

/// The interpreter's reply. The model changes only here.
#[derive(Debug, Clone)]
pub enum TuiMsg {
    /// The workspace was re-read after an action (or a refresh).
    Reloaded {
        issues: Vec<Issue>,
        now: DateTime<Utc>,
        status: String,
        selection: ReloadSelection,
    },
    /// The action refused or failed; the ledger is unchanged.
    Failed { message: String },
}

/// Run one command at the edge and return the message the fold hears.
#[must_use]
pub fn interpret(beads_dir: Option<&Path>, cmd: &TuiCmd) -> TuiMsg {
    let Some(dir) = beads_dir else {
        return TuiMsg::Failed {
            message: "no beads workspace found".to_string(),
        };
    };
    let now = Utc::now();
    match cmd {
        TuiCmd::Close {
            issue_id,
            commit_sha,
            reason,
            expected_revision,
        } => match close::close_from_tui(dir, issue_id, commit_sha, reason, *expected_revision) {
            Ok(closed) => reload(
                dir,
                format!("closed {}", closed.id),
                now,
                ReloadSelection::KeepIndex,
            ),
            Err(error) => TuiMsg::Failed {
                message: error.to_string(),
            },
        },
        TuiCmd::Delete {
            issue_id,
            reason,
            expected_revision,
        } => match delete::delete_from_tui(dir, issue_id, reason, *expected_revision) {
            Ok(_) => reload(
                dir,
                format!("deleted {issue_id}"),
                now,
                ReloadSelection::KeepIndex,
            ),
            Err(error) => TuiMsg::Failed {
                message: error.to_string(),
            },
        },
        TuiCmd::EditTitle {
            issue_id,
            title,
            expected_revision,
        } => match update::update_title_from_tui(dir, issue_id, title, *expected_revision) {
            Ok(id) => reload(dir, format!("updated {id}"), now, ReloadSelection::KeepById),
            Err(error) => TuiMsg::Failed {
                message: error.to_string(),
            },
        },
        TuiCmd::Reload => reload(dir, String::new(), now, ReloadSelection::KeepById),
    }
}

fn reload(dir: &Path, status: String, now: DateTime<Utc>, selection: ReloadSelection) -> TuiMsg {
    match load_issues_from(dir) {
        Ok(issues) => {
            let status = if status.is_empty() {
                format!("reloaded {} issues", issues.len())
            } else {
                status
            };
            TuiMsg::Reloaded {
                issues,
                now,
                status,
                selection,
            }
        }
        Err(error) => TuiMsg::Failed {
            message: format!("reload failed: {error}"),
        },
    }
}
