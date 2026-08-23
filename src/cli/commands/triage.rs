// governed-by: ADR-0003
//! `br triage` and `br next` — ADR-0003 §3.2 robot commands.
//!
//! Placeholder implementations that compile and emit valid JSON envelopes.
//! Full scoring/quick_ref/fail-closed-claim logic lands via gu7ts.3.

use crate::cli::{NextArgs, TriageArgs};
use crate::config::CliOverrides;
use crate::error::Result;
use crate::output::OutputContext;
use serde_json::json;

/// Execute `br triage`.
///
/// # Errors
///
/// Returns an error if the storage layer fails.
pub fn execute_triage(
    args: &TriageArgs,
    _overrides: &CliOverrides,
    ctx: &OutputContext,
) -> Result<()> {
    let body = json!({
        "status": "ok",
        "data": { "quick_ref": {}, "recommendations": [] },
        "usage_hints": ["br next", "br plan"],
    });
    if args.robot {
        println!("{}", serde_json::to_string_pretty(&body)?);
    }
    Ok(())
}

/// Execute `br next`.
///
/// # Errors
///
/// Returns an error if the storage layer fails.
pub fn execute_next(args: &NextArgs, _overrides: &CliOverrides, ctx: &OutputContext) -> Result<()> {
    let body = json!({
        "status": "ok",
        "data": { "recommendations": [] },
        "usage_hints": ["br triage"],
    });
    if args.robot {
        println!("{}", serde_json::to_string_pretty(&body)?);
    }
    Ok(())
}
