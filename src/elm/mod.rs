//! The elm-effect core of beads (ADR-0002 sibling port, beads branch).
//!
//! beads is a long-lived ledger, so its core is a fold over messages:
//! [`Program::update`] takes a model plus a message and returns the next
//! model plus a list of commands. The fold is pure — it reads no clock, no
//! environment, and no database. The elm-effect fold lints police exactly
//! that (`SystemTime`, `std::env`, `rand::thread_rng` inside `update`, and
//! a `HashMap` inside a `*Model`), and the rules hold here for the same
//! reason they hold in the sibling: a fold that reads the world cannot be
//! replayed, so the same event log can never be guaranteed to fold twice
//! into the same status.
//!
//! Two streams live in the model and never cross:
//!
//! - Issue rows ([`BeadsModel::issues`]), written by [`BeadsMsg::Event`].
//! - Gate verdicts ([`BeadsModel::gates`]), written only by
//!   [`BeadsMsg::GatesLoaded`].
//!
//! Folding an event log therefore cannot mint a verdict, and importing
//! issues cannot rebuild the verdict table. A projection may join the two
//! streams for display; nothing in here writes one stream from the other,
//! and no import path replays issues to rebuild verdicts.
//!
//! The close decision reuses the existing pure evaluators
//! ([`crate::close_policy::evaluate_require_legal_close`] and
//! [`crate::verify::legal_close`]) instead of restating them, so this fold
//! and `run_close_core` cannot drift apart on the same rule.
//!
//! [`crate::elm::world`] is the one place the world is touched: it turns
//! each command into an effect, provides the ledger port at that edge, and
//! feeds every outcome back into the fold as a message.

mod world;

use std::collections::BTreeMap;

use elm_effect_elm::{Boot, Cmd, Durability, Failure, Program, Update};
use serde::{Deserialize, Serialize};

pub use world::{CloseFold, LedgerPort, SqliteLedger};

use crate::close_policy::{self, GateResult, PolicyViolation};
use crate::model::{Event, EventType, Status};

/// Issue facts the fold carries.
///
/// The store owns the whole [`crate::model::Issue`]; the fold keeps only
/// what a close decision reads. Folding an event log can then never widen
/// into a store dependency, and a snapshot of facts is small enough that a
/// test can name the whole input.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueFacts {
    /// Current status. Only events and an interpreter confirmation move it.
    pub status: Status,
    /// Priority as the raw `i32` the legal-close table keys on.
    pub priority: i32,
    /// The typed VERIFY fence. Empty means not loop-runnable, which is a
    /// band of its own in the legal-close table.
    pub verify: String,
}

/// An in-flight close request.
///
/// The fold's own bookkeeping: it holds what the interpreter was asked for
/// and, once the verdict rows arrive, the decision they produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingClose {
    /// The facts the request arrived with.
    pub facts: IssueFacts,
    /// The decision, once [`BeadsMsg::GatesLoaded`] answered the request.
    pub decision: Option<CloseDecision>,
}

/// The fold's close decision.
///
/// Both fields come from the existing pure evaluators, in the order
/// `run_close_core` produced them, so the CLI's error text cannot drift
/// from the ledger's.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CloseDecision {
    /// The gate name that legally authorizes the write, when one does.
    pub verdict: Option<String>,
    /// Every policy violation found. Non-empty means the close is refused.
    pub violations: Vec<PolicyViolation>,
}

impl CloseDecision {
    /// Whether the fold authorizes the status write.
    #[must_use]
    pub fn is_legal(&self) -> bool {
        self.violations.is_empty()
    }

    /// The refusal recorded when a close request the fold could not answer.
    ///
    /// A decision that was never produced is not a pass: the verdict read
    /// failed, or it never came back. Either way the ledger stays closed,
    /// which is the only reading of ADR-0001 §5.3 that survives an
    /// interpreter that stops answering.
    #[must_use]
    pub(crate) fn unanswered(issue_id: &str) -> Self {
        Self {
            verdict: None,
            violations: vec![PolicyViolation {
                gate: "close_decision_unanswered".to_string(),
                message: format!(
                    "no close decision was reached for {issue_id}: the gate verdict read did \
                     not answer, so nothing authorizes the write"
                ),
                detail: Some(serde_json::json!({ "issue_id": issue_id })),
            }],
        }
    }
}

/// Ledger state.
///
/// `BTreeMap` throughout. Iteration order is part of the fold's contract:
/// two folds of the same event log must visit the same issues in the same
/// order, so hash order is not acceptable here.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BeadsModel {
    /// Issue facts by id. Written by [`BeadsMsg::Event`].
    pub issues: BTreeMap<String, IssueFacts>,
    /// Gate verdict rows by id. Written only by [`BeadsMsg::GatesLoaded`].
    pub gates: BTreeMap<String, Vec<GateResult>>,
    /// One in-flight close request per issue.
    pub pending: BTreeMap<String, PendingClose>,
    /// Failures, defects, and interrupts the interpreter reported, in the
    /// order they arrived. Kept apart from [`CloseDecision::violations`] so
    /// a defect is never laundered into a business error.
    pub failures: Vec<Failure<CoreError>>,
}

/// Everything the fold hears.
///
/// The world's inputs ride inside these messages: an event carries its own
/// timestamp, and the store's rows arrive as a reply. Nothing in the fold
/// reaches out for them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum BeadsMsg {
    /// A domain event folded into the model.
    Event(Event),
    /// A close request for one issue.
    CloseRequested {
        /// The issue being closed.
        issue_id: String,
        /// The facts the request arrived with.
        facts: IssueFacts,
    },
    /// The interpreter's reply: the verdict rows bound to this issue.
    GatesLoaded {
        /// The issue the rows belong to.
        issue_id: String,
        /// The rows, as the store scoped them.
        rows: Vec<GateResult>,
    },
    /// The interpreter's confirmation that a status write committed. The
    /// model's status changes only on this message, never on the request.
    Persisted {
        /// The issue whose status write committed.
        issue_id: String,
    },
    /// A command outcome that was not success.
    Failed(Failure<CoreError>),
}

/// The fold's output.
///
/// A list of commands is work that has NOT happened yet: the interpreter
/// runs each one and feeds every outcome back as a message. Dropping the
/// list drops the work.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BeadsCmd {
    /// Read the gate verdict rows bound to one issue's transition.
    LoadGates {
        /// The issue to read rows for.
        issue_id: String,
        /// The transition's source status.
        from: String,
        /// The transition's target status.
        to: String,
    },
}

impl Cmd for BeadsCmd {
    /// SQLite is the ledger's system of record, so reading verdicts leaves
    /// no trace of its own: the durable intent is the row itself, which
    /// makes this an `Ephemeral` read.
    ///
    /// A `Persist` command earns `Durable` when the close write itself
    /// routes through the fold — see `references/migrate-beads.md` item 5.
    fn durability(&self) -> Durability {
        Durability::Ephemeral
    }
}

/// The interpreter's typed failure.
///
/// Typed failures are domain data the fold handles by value. A defect or
/// an interruption is not one of these and never becomes one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
pub enum CoreError {
    /// The store refused the verdict read for an issue.
    #[error("gate verdict read failed for {issue_id}: {message}")]
    GateRead {
        /// The issue whose rows were requested.
        issue_id: String,
        /// The store's error, rendered once at the boundary.
        message: String,
    },
}

/// The beads core program: a pure fold over the ledger's event stream.
pub struct BeadsCore;

impl Program for BeadsCore {
    type Model = BeadsModel;
    type Msg = BeadsMsg;
    type Cmd = BeadsCmd;
    type Err = CoreError;

    fn init(_boot: &Boot) -> Update<BeadsModel, BeadsCmd> {
        Update::quiet(BeadsModel::default())
    }

    fn update(model: &BeadsModel, msg: &BeadsMsg) -> Update<BeadsModel, BeadsCmd> {
        let mut next = model.clone();
        match msg {
            BeadsMsg::Event(event) => {
                fold_event(&mut next, event);
                Update::quiet(next)
            }
            BeadsMsg::CloseRequested { issue_id, facts } => {
                next.issues.insert(issue_id.clone(), facts.clone());
                next.pending.insert(
                    issue_id.clone(),
                    PendingClose {
                        facts: facts.clone(),
                        decision: None,
                    },
                );
                Update::with_commands(
                    next,
                    vec![BeadsCmd::LoadGates {
                        issue_id: issue_id.clone(),
                        from: facts.status.as_str().to_string(),
                        to: Status::Closed.as_str().to_string(),
                    }],
                )
            }
            BeadsMsg::GatesLoaded { issue_id, rows } => {
                // The verdict stream is its own log. This fold fills it from
                // the interpreter's reply and from nothing else, so an event
                // log can never mint a verdict.
                next.gates.insert(issue_id.clone(), rows.clone());
                let decision = next
                    .pending
                    .get(issue_id)
                    .map(|pending| close_decision(issue_id, &pending.facts, rows));
                let Some(decision) = decision else {
                    return Update::quiet(next);
                };
                if let Some(pending) = next.pending.get_mut(issue_id) {
                    pending.decision = Some(decision);
                }
                Update::quiet(next)
            }
            BeadsMsg::Persisted { issue_id } => {
                // The request never moves a status. Only the interpreter's
                // confirmation does, and only when the decision was legal.
                let legal = next.pending.get(issue_id).is_some_and(|pending| {
                    pending
                        .decision
                        .as_ref()
                        .is_some_and(CloseDecision::is_legal)
                });
                next.pending.remove(issue_id);
                if !legal {
                    return Update::quiet(next);
                }
                if let Some(facts) = next.issues.get_mut(issue_id) {
                    facts.status = Status::Closed;
                }
                Update::quiet(next)
            }
            BeadsMsg::Failed(failure) => {
                // The three cause kinds stay apart: a typed failure is
                // recorded as one, and a defect or interrupt is never
                // relabelled as a policy violation.
                next.failures.push(failure.clone());
                Update::quiet(next)
            }
        }
    }

    fn failure(failure: &Failure<CoreError>) -> BeadsMsg {
        BeadsMsg::Failed(failure.clone())
    }
}

/// Fold one domain event into the model.
///
/// Pure: the event carries its own timestamp, so no clock is read. An
/// event whose type does not name a status leaves the status alone; the
/// fold never invents a transition it was not told about.
fn fold_event(model: &mut BeadsModel, event: &Event) {
    let Some(status) = status_after(event) else {
        return;
    };
    let facts = model.issues.entry(event.issue_id.clone()).or_default();
    facts.status = status;
}

/// The status an event leaves an issue in, when the event names one.
fn status_after(event: &Event) -> Option<Status> {
    match event.event_type {
        EventType::StatusChanged => event.new_value.as_deref().and_then(|v| v.parse().ok()),
        EventType::Closed => Some(Status::Closed),
        EventType::Reopened | EventType::Restored => Some(Status::Open),
        EventType::Deleted | EventType::Compacted => Some(Status::Tombstone),
        _ => None,
    }
}

/// Decide whether a close with these verdict rows is legal.
///
/// A re-implementation of `run_close_core`'s fail-closed block would be a
/// second copy of ADR-0001 §5.3, so this calls the same evaluators that
/// block does. Order and wording are preserved so the CLI's refusal text
/// is byte-identical whichever door produced it.
fn close_decision(issue_id: &str, facts: &IssueFacts, rows: &[GateResult]) -> CloseDecision {
    let from = facts.status.as_str();
    let verify = &facts.verify;
    let input = close_policy::legal_close_input_for_issue_pub(facts.priority, verify);
    let verdict = rows
        .iter()
        .filter(|result| result.passed)
        .find_map(|result| {
            let kind = crate::verify::VerdictKind::from_gate_name(&result.gate)?;
            crate::verify::legal_close(kind, &input).then(|| kind.gate_name().to_string())
        });
    let violations =
        close_policy::evaluate_require_legal_close(issue_id, from, facts.priority, verify, rows);
    CloseDecision {
        verdict,
        violations,
    }
}

#[cfg(test)]
mod tests;
