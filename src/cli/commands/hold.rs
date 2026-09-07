// governed-by: ADR-0005
//! `br hold` — captain hold lifecycle (beads ADR-0005 §4).

use crate::cli::commands::{auto_import_storage_ctx_if_stale, resolve_issue_ids};
use crate::cli::{HoldArgs, config};
use crate::error::{BeadsError, Result};
use crate::output::OutputContext;
use crate::util::id::{IdResolver, ResolverConfig};
use serde::Serialize;

/// One hold row for machine-readable output.
#[derive(Debug, Clone, Serialize)]
struct HoldRow {
    issue_id: String,
    corr: String,
    kind: String,
    open: bool,
    expires_at: Option<String>,
    resolving_corr: Option<String>,
    resurfaced: bool,
}

impl From<crate::hold::CaptainHold> for HoldRow {
    fn from(h: crate::hold::CaptainHold) -> Self {
        Self {
            issue_id: h.issue_id,
            corr: h.corr,
            kind: h.kind,
            open: h.open,
            expires_at: h.expires_at.map(|dt| dt.to_rfc3339()),
            resolving_corr: h.resolving_corr,
            resurfaced: h.resurfaced,
        }
    }
}

/// Execute the hold command.
///
/// # Errors
///
/// Returns an error if IDs cannot be resolved or storage writes fail.
pub fn execute(
    args: &HoldArgs,
    json: bool,
    cli: &config::CliOverrides,
    ctx: &OutputContext,
) -> Result<()> {
    if args.kind != "captain" {
        return Err(BeadsError::validation(
            "kind",
            "only --kind captain is supported (ADR-0005 §4)",
        ));
    }
    let beads_dir = config::discover_beads_dir_with_cli(cli)?;
    let mut storage_ctx = config::open_storage_with_cli(&beads_dir, cli)?;
    auto_import_storage_ctx_if_stale(&mut storage_ctx, cli)?;

    let config_layer = storage_ctx.load_config(cli)?;
    let actor = config::resolve_actor(&config_layer);
    let id_config = config::id_config_from_layer(&config_layer);
    let resolver = IdResolver::new(ResolverConfig::with_prefix(id_config.prefix));
    let resolved_ids = resolve_issue_ids(&storage_ctx.storage, &resolver, &args.ids)?;
    if resolved_ids.is_empty() {
        return Err(BeadsError::validation(
            "ids",
            "at least one issue ID is required",
        ));
    }

    if args.list {
        let mut rows = Vec::new();
        for id in &resolved_ids {
            for hold in storage_ctx.storage.open_captain_holds(id)? {
                rows.push(HoldRow::from(hold));
            }
        }
        // Lazy re-surface: expiries re-surface on read, never drop.
        let _ = storage_ctx
            .storage
            .resurface_expired_captain_holds(&actor)?;
        emit_unit(json, ctx, &rows);
        return Ok(());
    }

    if args.sweep {
        let count = storage_ctx
            .storage
            .resurface_expired_captain_holds(&actor)?;
        if json {
            emit_unit(json, ctx, &serde_json::json!({ "resurfaced": count }));
            return Ok(());
        }
        ctx.print_line(&format!(
            "re-surfaced {count} expired hold(s); holds stay open"
        ));
        return Ok(());
    }

    let corr = args.corr.as_deref().map(str::trim).unwrap_or_default();
    if corr.is_empty() {
        return Err(BeadsError::validation("corr", "--corr <id> is required"));
    }
    let mut rows = Vec::new();
    for id in &resolved_ids {
        if args.resolve {
            let resolved = storage_ctx.storage.resolve_captain_hold(id, corr, &actor)?;
            if resolved.is_empty() {
                return Err(BeadsError::validation(
                    "corr",
                    format!("no open captain hold for corr {corr} on {id}: nothing cleared"),
                ));
            }
        } else {
            let hold = storage_ctx.storage.bind_captain_hold(
                id,
                corr,
                args.expires_at.as_deref(),
                &actor,
            )?;
            rows.push(HoldRow::from(hold));
        }
        for hold in storage_ctx.storage.open_captain_holds(id)? {
            if !rows.iter().any(|r: &HoldRow| r.corr == hold.corr) {
                rows.push(HoldRow::from(hold));
            }
        }
    }
    emit_unit(json, ctx, &rows);
    Ok(())
}

fn emit_unit(json: bool, ctx: &OutputContext, value: &impl Serialize) {
    if json || ctx.is_json() {
        ctx.json(value);
    } else if let Ok(rows) = serde_json::to_value(value)
        && let Some(list) = rows.as_array()
    {
        for row in list {
            ctx.print_line(&row.to_string());
        }
    }
}
