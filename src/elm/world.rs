//! The edge: the one place the fold's commands meet the world.
//!
//! [`crate::elm::BeadsCore`] is pure, so every command in
//! [`crate::elm::BeadsCmd`] is a description of work that has not happened.
//! This module runs those descriptions and feeds each outcome back into the
//! fold as a message.
//!
//! # Why this runs the commands directly instead of through an `Effect`
//!
//! `elm_effect::Effect::sync` takes a `Send` closure and
//! `ManagedRuntime::run` takes `Send` bounds, and a command window is run
//! concurrently. beads' store is `rusqlite` over a `RefCell<InnerConnection>`,
//! so `&SqliteStorage` is `Send` only if the store is `Sync`, which it is
//! not. There is therefore no sound way to hand the store to an effect, and
//! no sound way to race two commands against it.
//!
//! So the bridge runs one command at a time on the caller's thread. That is
//! the law's own answer, not a shortcut around it: "the process entry point,
//! or one bridge into a framework handler, is what runs them", and a bridge
//! that cannot race a `!Sync` store must not pretend to. When the store
//! gains a `Sync` handle (a connection pool, an `Mutex`-guarded handle), the
//! interpreter moves to `Effect::` + a `Layer` and nothing above this
//! function changes.
//!
//! What the edge does keep from the framework is the part that carries
//! meaning: commands are values that run nowhere until the bridge runs
//! them, a command's outcome keeps its kind, and a typed failure, a defect,
//! and an interruption each reach the fold as a distinct [`Failure`].

use std::sync::Arc;

use elm_effect_elm::{Failure, Program};

use super::{BeadsCmd, BeadsCore, BeadsMsg, CloseDecision, CoreError, IssueFacts};

/// The world as the fold sees it: one method that runs one command and
/// returns the message the fold hears.
///
/// A port is a verb-shaped boundary, not a store handle. Tests implement
/// this trait to script scenarios — a store that refuses, a store that
/// returns rows for an unknown issue — with no database in the test.
///
/// The trait deliberately carries no `Send`/`Sync` bound. The store behind
/// [`SqliteLedger`] is `rusqlite` over a `RefCell`, so a bound of either
/// kind would be a claim this crate cannot make. Requiring them here would
/// force the port to wrap the store in a lock the fold never needed; the
/// bound gets added when the store gains a `Sync` handle, and until then
/// the bridge stays on the caller's thread.
pub trait LedgerPort {
    /// Run one command. A typed failure is returned, never panicked.
    fn run(&self, cmd: &BeadsCmd) -> Result<BeadsMsg, CoreError>;
}

/// The ledger port backed by the store.
pub struct SqliteLedger<'a> {
    storage: &'a crate::storage::SqliteStorage,
}

impl<'a> SqliteLedger<'a> {
    /// Wrap the store.
    pub fn new(storage: &'a crate::storage::SqliteStorage) -> Self {
        Self { storage }
    }

    /// The port as a service value.
    // shortcut: not `Send`/`Sync` because the `rusqlite` store behind it is
    // `RefCell`-backed. Upgrade path: give the store a `Sync` handle
    // (connection pool or `Mutex` guard) and the bound drops in.
    #[allow(clippy::arc_with_non_send_sync)]
    pub fn service(storage: &'a crate::storage::SqliteStorage) -> Arc<dyn LedgerPort + 'a> {
        Arc::new(Self::new(storage))
    }
}

impl LedgerPort for SqliteLedger<'_> {
    fn run(&self, cmd: &BeadsCmd) -> Result<BeadsMsg, CoreError> {
        match cmd {
            BeadsCmd::LoadGates { issue_id, from, to } => {
                let rows = self
                    .storage
                    .get_scoped_gate_results(issue_id, from, to)
                    .map_err(|error| CoreError::GateRead {
                        issue_id: issue_id.clone(),
                        message: error.to_string(),
                    })?;
                Ok(BeadsMsg::GatesLoaded {
                    issue_id: issue_id.clone(),
                    rows,
                })
            }
        }
    }
}

/// The edge bridge for one close.
///
/// Owns the fold's model and the ledger port, runs each command, and feeds
/// every outcome back. `Program::update` is never called from anywhere that
/// owns a socket or a clock, and no fold call happens inside a command.
pub struct CloseFold<'a> {
    model: super::BeadsModel,
    port: Arc<dyn LedgerPort + 'a>,
}

impl<'a> CloseFold<'a> {
    /// Boot the fold against a ledger port.
    #[must_use]
    pub fn new(port: Arc<dyn LedgerPort + 'a>) -> Self {
        Self {
            model: super::BeadsModel::default(),
            port,
        }
    }

    /// Boot the fold against the store.
    // `Self<'a>` is not a legal spelling, so the name is spelled out.
    #[allow(clippy::use_self)]
    #[must_use]
    pub fn over_storage(storage: &'a crate::storage::SqliteStorage) -> CloseFold<'a> {
        Self::new(SqliteLedger::service(storage))
    }

    /// Fold a close request and run the commands it produced.
    ///
    /// Returns the decision the fold reached. The caller writes the status
    /// change itself; when that write commits it reports back with
    /// [`CloseFold::persisted`], so the fold's model stops being a proposal.
    ///
    /// A request the fold could not answer — the interpreter failed, or it
    /// never replied — comes back as a refusal, never as a pass. Silence is
    /// not authorization.
    pub fn request(&mut self, issue_id: &str, facts: IssueFacts) -> CloseDecision {
        let update = BeadsCore::update(
            &self.model,
            &BeadsMsg::CloseRequested {
                issue_id: issue_id.to_string(),
                facts,
            },
        );
        self.apply(update);
        match self.decision(issue_id) {
            Some(decision) => decision.clone(),
            None => CloseDecision::unanswered(issue_id),
        }
    }

    /// Report that a status write committed.
    ///
    /// The model's status moves to closed only here, and only when the
    /// decision was legal. A refusal never gets a confirmation, so a
    /// refused close cannot leave the model claiming otherwise.
    pub fn persisted(&mut self, issue_id: &str) {
        let update = BeadsCore::update(
            &self.model,
            &BeadsMsg::Persisted {
                issue_id: issue_id.to_string(),
            },
        );
        self.apply(update);
    }

    /// The last decision the fold recorded for an issue.
    #[must_use]
    pub fn decision(&self, issue_id: &str) -> Option<&CloseDecision> {
        self.model
            .pending
            .get(issue_id)
            .and_then(|pending| pending.decision.as_ref())
    }

    /// Adopt one fold step, running any commands it produced before the
    /// next message is folded.
    fn apply(&mut self, update: elm_effect_elm::Update<super::BeadsModel, BeadsCmd>) {
        self.model = update.model;
        let mut queue: std::collections::VecDeque<BeadsCmd> = update.commands.into();
        while let Some(cmd) = queue.pop_front() {
            let reply = self.run_one(&cmd);
            let next = BeadsCore::update(&self.model, &reply);
            self.model = next.model;
            // A reply can itself carry commands; the bridge keeps folding
            // until nothing is left, in command order, one at a time.
            queue.extend(next.commands);
        }
    }

    /// Run one command and classify its outcome.
    ///
    /// Success keeps the interpreter's message. A typed failure arrives by
    /// value; a panic during the command is a defect, not a business error,
    /// and is classified as one instead of being unwrapped onto the
    /// caller's thread.
    fn run_one(&self, cmd: &BeadsCmd) -> BeadsMsg {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.port.run(cmd))) {
            Ok(Ok(msg)) => msg,
            Ok(Err(error)) => BeadsCore::failure(&Failure::Fail(error)),
            Err(payload) => BeadsCore::failure(&Failure::Defect(render_panic(payload))),
        }
    }
}

/// Render a panic payload escaped from a command as a defect.
fn render_panic(payload: Box<dyn std::any::Any + Send>) -> elm_effect::Defect {
    let message = match payload.downcast::<&str>() {
        Ok(text) => (*text).to_string(),
        Err(payload) => match payload.downcast::<String>() {
            Ok(text) => *text,
            Err(_payload) => "command panicked".to_string(),
        },
    };
    elm_effect::Defect::new(message)
}
