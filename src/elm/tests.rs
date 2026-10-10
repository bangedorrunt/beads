//! Fold tests. Two of these are the migrate-beads `Done` checklist; the
//! rest hold the fold to its own contract.
//!
//! Every test here runs the fold and a scripted port. No test touches
//! SQLite, a clock, the environment, or a socket.

use std::sync::{Arc, Mutex};

use elm_effect_elm::Program;

use super::{
    BeadsCmd, BeadsCore, BeadsModel, BeadsMsg, CloseFold, CoreError, Failure, IssueFacts,
    LedgerPort, Status, Update,
};
use crate::close_policy::GateResult;
use crate::model::{Event, EventType};

/// Fold every message in order, returning the final model and the command
/// list the fold produced along the way.
fn fold_all(msgs: &[BeadsMsg]) -> (BeadsModel, Vec<BeadsCmd>) {
    let mut model = BeadsModel::default();
    let mut commands = Vec::new();
    for msg in msgs {
        let Update {
            model: next,
            commands: step,
        } = BeadsCore::update(&model, msg);
        commands.extend(step);
        model = next;
    }
    (model, commands)
}

/// One fold step, named here so the tests never reach into the trait's
/// associated function twice per line.
fn update(model: &BeadsModel, msg: &BeadsMsg) -> (BeadsModel, Vec<BeadsCmd>) {
    let Update {
        model: next,
        commands,
    } = BeadsCore::update(model, msg);
    (next, commands)
}

fn close_requested(id: &str) -> BeadsMsg {
    BeadsMsg::CloseRequested {
        issue_id: id.into(),
        facts: facts(Status::Open, 2, "cargo test -p beads"),
    }
}

fn gates_loaded(id: &str, rows: &[GateResult]) -> BeadsMsg {
    BeadsMsg::GatesLoaded {
        issue_id: id.into(),
        rows: rows.to_vec(),
    }
}

fn facts(status: Status, priority: i32, verify: &str) -> IssueFacts {
    IssueFacts {
        status,
        priority,
        verify: verify.into(),
    }
}

fn gate(name: &str, passed: bool) -> GateResult {
    GateResult {
        gate: name.into(),
        provider: "ci".into(),
        passed,
        note: None,
    }
}

fn event(event_type: EventType, new_value: Option<&str>) -> Event {
    Event {
        id: 1,
        issue_id: "beads_rust-a".into(),
        event_type,
        actor: "IvoryRidge".into(),
        old_value: None,
        new_value: new_value.map(str::to_string),
        comment: None,
        created_at: "2026-10-10T00:00:00Z".parse().expect("a static timestamp"),
        agent_name: None,
        harness: None,
        model: None,
    }
}

/// An event log long enough to be real: a creation, a claim, a reopen, and
/// a second claim. Every event carries its own timestamp, so the fold never
/// reads a clock to place it.
fn event_log() -> Vec<BeadsMsg> {
    vec![
        BeadsMsg::Event(event(EventType::StatusChanged, Some("open"))),
        BeadsMsg::Event(event(EventType::StatusChanged, Some("in_progress"))),
        BeadsMsg::Event(event(EventType::StatusChanged, Some("blocked"))),
        BeadsMsg::Event(event(EventType::StatusChanged, Some("in_progress"))),
        BeadsMsg::Event(event(EventType::Commented, None)),
        BeadsMsg::Event(event(EventType::StatusChanged, Some("not-a-status"))),
    ]
}

/// A port that hands back a script of replies and records what it ran.
struct RecordingPort {
    replies: Mutex<Vec<Result<BeadsMsg, CoreError>>>,
    commands: Mutex<Vec<BeadsCmd>>,
}

impl RecordingPort {
    fn new(replies: Vec<Result<BeadsMsg, CoreError>>) -> Arc<Self> {
        Arc::new(Self {
            replies: Mutex::new(replies),
            commands: Mutex::new(Vec::new()),
        })
    }

    fn commands(&self) -> Vec<BeadsCmd> {
        self.commands
            .lock()
            .expect("the test lock is not poisoned")
            .clone()
    }
}

impl LedgerPort for RecordingPort {
    fn run(&self, cmd: &BeadsCmd) -> Result<BeadsMsg, CoreError> {
        self.commands
            .lock()
            .expect("the test lock is not poisoned")
            .push(cmd.clone());
        self.replies
            .lock()
            .expect("the test lock is not poisoned")
            .pop()
            .unwrap_or_else(|| {
                Err(CoreError::GateRead {
                    issue_id: "beads_rust-a".into(),
                    message: "the script is empty".into(),
                })
            })
    }
}

/// The migrate-beads checklist, item 1: the same event log folds twice into
/// the same status and the same command list.
///
/// A test that only ran once would pass even if `update` read a clock or
/// the environment, because a single run still looks right. Folding the
/// same log twice and comparing both outputs is what makes the claim.
#[test]
fn the_same_event_log_folds_twice_into_the_same_state_and_commands() {
    let log = event_log();
    let (first_model, first_commands) = fold_all(&log);
    let (second_model, second_commands) = fold_all(&log);

    assert_eq!(
        first_model, second_model,
        "the same log produced two models"
    );
    assert_eq!(
        first_commands, second_commands,
        "the same log produced two command lists"
    );
    // Guard the comparison itself: the log must actually have moved a
    // status, or "same twice" is trivially true of an empty fold.
    assert_eq!(
        first_model.issues["beads_rust-a"].status,
        Status::InProgress
    );
    assert!(
        first_commands.is_empty(),
        "a status change is a fold, not a command"
    );
}

/// A close request asks for verdict rows. It does not write anything and
/// it does not change the model's status.
#[test]
fn a_close_request_only_asks_for_verdict_rows() {
    let (model, commands) = update(&BeadsModel::default(), &close_requested("beads_rust-a"));

    assert_eq!(
        commands,
        vec![BeadsCmd::LoadGates {
            issue_id: "beads_rust-a".into(),
            from: "open".into(),
            to: "closed".into(),
        }]
    );
    assert_eq!(model.issues["beads_rust-a"].status, Status::Open);
    assert!(model.pending["beads_rust-a"].decision.is_none());
}

/// The migrate-beads checklist, item 2: a close with no PASS row never
/// becomes closed.
///
/// This is the fail-closed invariant behind ADR-0001 §5.3, and the reason
/// the fold exists at all. The refusal must also name the legal gate, or
/// the operator has nothing to run to fix it.
#[test]
fn a_close_with_no_pass_row_is_refused_and_the_status_stays_open() {
    let (model, _) = update(&BeadsModel::default(), &close_requested("beads_rust-a"));
    let (model, commands) = update(&model, &gates_loaded("beads_rust-a", &[]));

    let decision = model.pending["beads_rust-a"]
        .decision
        .clone()
        .expect("a decision was reached");
    assert!(
        !decision.is_legal(),
        "no rows at all is a refusal, not a default pass"
    );
    assert_eq!(decision.verdict, None);
    assert_eq!(decision.violations.len(), 1);
    assert_eq!(decision.violations[0].gate, "gate_require_legal_close");
    assert!(
        decision.violations[0].message.contains("command-verified"),
        "the refusal must name the legal gate: {}",
        decision.violations[0].message
    );

    // A FAIL row refuses too, and names itself.
    let (model, _) = update(&BeadsModel::default(), &close_requested("beads_rust-a"));
    let (model, _) = update(
        &model,
        &gates_loaded("beads_rust-a", &[gate("command-verified", false)]),
    );
    let failed = model.pending["beads_rust-a"]
        .decision
        .clone()
        .expect("a decision was reached");
    assert_eq!(failed.violations[0].gate, "gate_command-verified");

    // No command was emitted for either refusal: the fold proposed no write.
    assert!(commands.is_empty());
    // And the request never moved a status.
    assert_eq!(model.issues["beads_rust-a"].status, Status::Open);
}

/// The other half of the fail-closed invariant: a legal PASS row does
/// authorize the write, and the model's status moves only when the
/// interpreter confirms the write landed.
#[test]
fn a_legal_pass_row_authorizes_the_write_and_only_the_confirmation_moves_the_status() {
    let (mut model, _) = update(&BeadsModel::default(), &close_requested("beads_rust-a"));
    let (next, _) = update(
        &model,
        &gates_loaded("beads_rust-a", &[gate("command-verified", true)]),
    );
    model = next;

    let decision = model.pending["beads_rust-a"]
        .decision
        .clone()
        .expect("a decision was reached");
    assert!(decision.is_legal());
    assert_eq!(decision.verdict, Some("command-verified".to_string()));

    // Still open: a request plus a legal verdict is a proposal, not a write.
    assert_eq!(model.issues["beads_rust-a"].status, Status::Open);

    let (model, _) = update(
        &model,
        &BeadsMsg::Persisted {
            issue_id: "beads_rust-a".into(),
        },
    );
    assert_eq!(model.issues["beads_rust-a"].status, Status::Closed);
    assert!(model.pending.is_empty());
}

/// The migrate-beads checklist, item 3: importing issues without
/// `gates.jsonl` does not grow the verdict table.
///
/// The two streams are separate, so folding the issue event log must leave
/// the verdict table exactly as it found it.
#[test]
fn importing_issues_without_a_gate_sidecar_leaves_the_verdict_table_empty() {
    let (model, _) = fold_all(&event_log());

    assert!(
        model.gates.is_empty(),
        "an issue event minted a verdict row"
    );
    assert!(model.pending.is_empty());
    assert!(model.failures.is_empty());
}

/// The end-to-end close, through the interpreter.
///
/// Substitutes the world (a scripted port) rather than a store, which is
/// the only way a store that behaves badly can be tested without breaking
/// one.
#[test]
fn the_interpreter_drives_the_close_and_the_fold_sees_the_reply() {
    let store = RecordingPort::new(vec![Ok(gates_loaded(
        "beads_rust-a",
        &[gate("command-verified", true)],
    ))]);

    let mut fold = CloseFold::new(Arc::clone(&store) as Arc<dyn LedgerPort>);
    let decision = fold.request(
        "beads_rust-a",
        facts(Status::Open, 2, "cargo test -p beads"),
    );

    assert!(decision.is_legal());
    assert_eq!(
        store.commands(),
        vec![BeadsCmd::LoadGates {
            issue_id: "beads_rust-a".into(),
            from: "open".into(),
            to: "closed".into(),
        }]
    );

    fold.persisted("beads_rust-a");
}

/// A store that fails surfaces as a typed failure, never as a policy
/// violation: a broken read is not evidence that a close is legal, and it
/// is not evidence that one is illegal either. The request goes unanswered,
/// and an unanswered request refuses.
#[test]
fn a_failing_store_refuses_rather_than_passing() {
    let store = RecordingPort::new(vec![Err(CoreError::GateRead {
        issue_id: "beads_rust-a".into(),
        message: "database is locked".into(),
    })]);

    let mut fold = CloseFold::new(Arc::clone(&store) as Arc<dyn LedgerPort>);
    let decision = fold.request(
        "beads_rust-a",
        facts(Status::Open, 2, "cargo test -p beads"),
    );

    assert!(
        !decision.is_legal(),
        "an unanswered close must not read as a pass"
    );
    assert_eq!(decision.violations[0].gate, "close_decision_unanswered");
    assert!(
        fold.decision("beads_rust-a").is_none(),
        "no verdict rows ever arrived"
    );
}

/// A fold that never hears a reply records no decision: the request is a
/// description of work, and dropping the work drops the decision. The
/// bridge still refuses, because silence is not authorization.
#[test]
fn a_request_without_a_reply_leaves_no_decision() {
    let store = RecordingPort::new(vec![]);

    let mut fold = CloseFold::new(Arc::clone(&store) as Arc<dyn LedgerPort>);
    let decision = fold.request(
        "beads_rust-a",
        facts(Status::Open, 2, "cargo test -p beads"),
    );

    assert!(!decision.is_legal());
    assert_eq!(decision.violations[0].gate, "close_decision_unanswered");
    assert!(fold.decision("beads_rust-a").is_none());
    assert_eq!(
        store.commands().len(),
        1,
        "the command still ran; only the reply is missing"
    );
}

/// A typed failure reaches the fold as a failure, in its own channel.
#[test]
fn a_typed_failure_keeps_its_kind_on_the_way_into_the_fold() {
    let (model, _) = update(
        &BeadsModel::default(),
        &BeadsMsg::Failed(Failure::Fail(CoreError::GateRead {
            issue_id: "beads_rust-a".into(),
            message: "locked".into(),
        })),
    );

    assert_eq!(model.failures.len(), 1);
    assert!(matches!(model.failures[0], Failure::Fail(_)));
    assert!(
        model.pending.is_empty(),
        "no close was requested, so nothing is pending"
    );
}

/// The fold lints the sibling enforces, applied to this crate's fold.
#[test]
fn the_fold_reads_no_ambient_world_and_stores_no_hash_map() {
    let src = include_str!("mod.rs");
    for token in ["SystemTime", "std::env", "rand::thread_rng"] {
        let update_body = fn_body(src, "update").expect("an update function exists");
        assert!(
            !update_body.contains(token),
            "update reaches for {token}: the fold must take it as a message"
        );
    }
    let model_body = struct_body(src, "BeadsModel").expect("a BeadsModel struct exists");
    assert!(
        !model_body.contains("HashMap"),
        "BeadsModel stores a HashMap; use BTreeMap"
    );
}

/// Extract the brace-balanced body of the first `fn <name>(` in `src`.
fn fn_body<'a>(src: &'a str, name: &str) -> Option<&'a str> {
    let sig = format!("fn {name}(");
    let start = src.find(&sig)?;
    let open = src[start..].find('{')? + start;
    let mut depth = 0usize;
    for (i, c) in src[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&src[open + 1..open + i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Extract the brace-balanced body of the first `struct <name>` in `src`.
fn struct_body<'a>(src: &'a str, name: &str) -> Option<&'a str> {
    let sig = format!("struct {name}");
    let start = src.find(&sig)?;
    let open = src[start..].find('{')? + start;
    let mut depth = 0usize;
    for (i, c) in src[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&src[open + 1..open + i]);
                }
            }
            _ => {}
        }
    }
    None
}
