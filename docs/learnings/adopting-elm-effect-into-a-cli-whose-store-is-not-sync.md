# Adopting elm-effect into a CLI whose store is `!Sync`

The elm-effect branch for beads says the fold is `update(model, msg) ->
(model, commands)` and that it reads no clock, no environment, and no
database. That is easy to write and it hides two questions the API surface
does not answer for you: how a command reaches the world, and what an
unanswered command means. Both cost a test run to find.

## 1. An `Effect` cannot hold a `rusqlite` store

`elm_effect::Effect::sync` takes a `Send` closure and
`ManagedRuntime::run` takes `Send` bounds, because a command window is run
concurrently. beads' store is `rusqlite` over
`RefCell<rusqlite::inner_connection::InnerConnection>`, so
`&SqliteStorage: Send` only if `SqliteStorage: Sync`, which it is not:

```
error[E0277]: `RefCell<rusqlite::inner_connection::InnerConnection>` cannot be shared between threads safely
   --> src/elm/world.rs
```

There is no `unsafe` way out (`#![deny(unsafe_code)]`), and wrapping the
store in a lock the fold never needed is a cost the design has not earned.
`Loop` is also unusable for the same reason: it is the caller of
`run_commands`, so it needs `Interpret::interpret` to build effects that
hold the world, and the trait gives it no way to.

So the bridge in `src/elm/world.rs` runs the commands itself, one at a
time, on the caller's thread. That is the effect-oriented skill's own
answer, not a shortcut around it: "the process entry point, or one bridge
into a framework handler, is what runs them". The `Program` fold, the
`Cmd` values, and the `Failure` classification all stay framework-native;
only the run edge is a plain function, because that is the only edge a
`!Sync` store can have.

**Upgrade path.** A `Sync` handle on the store (a connection pool, or an
`Mutex`-guarded handle) moves the interpreter to `Effect::sync` plus a
`Layer::from_value` and nothing above `CloseFold` changes. The port trait
carries no `Send`/`Sync` bound today on purpose, so adding one is a
one-line edit rather than a refactor.

## 2. `unwrap_or_default()` on a missing decision is a fail-open

The first draft of `CloseFold::request` ended with
`self.decision(issue_id).cloned().unwrap_or_default()`. `CloseDecision`
has an empty `violations` vector, and `is_legal()` is
`violations.is_empty()`, so a command that never answered came back
**legal**. A store that refused to read verdicts, or an interpreter that
never ran, authorized every close.

The test that caught it was a port scripted to return `Err`. It asserted
`!decision.is_legal()` and failed, which is the only reason this is not a
shipped fail-open in a fail-closed ledger.

The fix is that a decision which was never produced is a refusal:

```rust
None => CloseDecision::unanswered(issue_id),
```

with `gate: "close_decision_unanswered"`. Silence is not authorization.
Any fold that returns a decision type whose `Default` is permissive needs
the same guard, because `unwrap_or_default` reads as an answer.

## 3. Reuse the existing evaluator instead of restating the rule

ADR-0001 §5.3 already has one implementation, inline in
`run_close_core`. The temptation is to write the band selection again in
the fold, which gives the ledger two copies of the rule that can drift.
`src/elm/mod.rs` instead calls
`close_policy::legal_close_input_for_issue_pub`,
`crate::verify::legal_close`, and
`close_policy::evaluate_require_legal_close`, in the same order the CLI
block called them.

That is why the CLI's refusal text is byte-identical after the change:
`sh scripts/e2e-elm-fold-close.sh` closes a real bead through the fold and
asserts both refusals and the success path, plus that an issues-only
import leaves the verdict table empty.
