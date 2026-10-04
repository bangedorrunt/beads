//! `br land`: the paved close ceremony.
//!
//! One call validates the `--commit-sha` token, records the verdict gate
//! (through [`gate::record_verdict`], so the gate row is byte-for-byte the
//! one `br gate report` would write, previews included), then closes the bead
//! (through [`close::execute_land_close`], so every close-policy gate and the
//! sha-citation check run exactly as `br close` runs them), then names the
//! remaining operator steps in order — or performs the automatable ones
//! behind `--release-leases` / `--sync`.
//!
//! Land is a composition, never a second policy authority: the day the raw
//! verbs change, land changes with them.

use crate::cli::commands::{close, gate, sync};
use crate::cli::{GateReportArgs, GateStatus, LandArgs, SyncArgs};
use crate::close_policy;
use crate::config::{self, CliOverrides};
use crate::error::{BeadsError, Result};
use crate::format::sanitize_terminal_inline;
use crate::output::OutputContext;
use crate::util::id::{IdResolver, ResolverConfig};
use serde::Serialize;

/// Git object names are exactly 40 hex chars.
const SHA_LEN: usize = 40;

/// Validate the `--commit-sha` token: it must be the BARE full 40-hex sha.
///
/// Why refuse anything else up front: the ledger's legal-close check matches
/// the `sha=<token>` note by exact token. A sha glued to punctuation
/// (`sha=<sha>;`) or to prose reads UNBOUND forever — the row cannot be
/// re-bound after the close (a closed bead has no legal gate transition), so
/// the cheapest place to catch it is here.
fn validate_sha_token(raw: &str) -> Result<String> {
    let sha = raw.trim();
    if sha.is_empty() {
        return Err(BeadsError::validation(
            "commit-sha",
            "`--commit-sha` is empty; pass the bare full 40-hex sha of the commit whose \
             message cites the bead",
        ));
    }
    // `sha.get(SHA_LEN..)` returns `Some("")` on a string of EXACTLY 40 bytes,
    // so the length test must come first (a 40-char sha reported "glue").
    if sha.len() > SHA_LEN
        && let Some(extra) = sha.get(SHA_LEN..)
    {
        return Err(BeadsError::validation(
            "commit-sha",
            format!(
                "`--commit-sha` has `{extra}` glued after the {SHA_LEN}-hex sha ({} chars \
                 total); a `sha=<sha>{extra_head}` token reads UNBOUND in the ledger's \
                 exact-token check — pass the bare sha",
                sha.len(),
                extra_head = extra.chars().next().map(String::from).unwrap_or_default(),
            ),
        ));
    }
    if sha.len() < SHA_LEN {
        return Err(BeadsError::validation(
            "commit-sha",
            format!(
                "`--commit-sha {sha}` is {} chars; the close gates need the full \
                 {SHA_LEN}-hex sha (an abbreviation cannot be verified and does not bind)",
                sha.len()
            ),
        ));
    }
    if !sha.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(BeadsError::validation(
            "commit-sha",
            format!("`--commit-sha {sha}` is not hex; pass the bare full {SHA_LEN}-hex sha"),
        ));
    }
    Ok(sha.to_ascii_lowercase())
}

/// The gate note carrying the revision binding (and optional receipt).
fn build_gate_note(sha: &str, receipt: Option<&str>) -> String {
    match receipt.map(str::trim).filter(|receipt| !receipt.is_empty()) {
        Some(receipt) => format!("sha={sha} receipt={receipt}"),
        None => format!("sha={sha}"),
    }
}

/// Pick the verdict gate: the explicit one, or the bead's single legal close
/// gate. A bead admitting several refuses and names them (never a guess).
///
/// An explicit non-legal verdict is refused up front: `br gate report` warns
/// and records it anyway (external systems may report proactively), but land
/// IS the paved ceremony — recording a row that can never authorize the close
/// it is about to attempt is a guaranteed dead end, so it fails closed here
/// with the same legal-name list the close would demand.
fn resolve_gate_name(explicit: Option<&str>, issue: &crate::model::Issue) -> Result<String> {
    let input = close_policy::legal_close_input_for_issue_pub(
        issue.priority.0,
        issue.verify.as_deref().unwrap_or(""),
    );
    let legal = close_policy::legal_close_gate_names(&input);
    if let Some(gate) = explicit {
        let gate = gate.trim();
        if gate.is_empty() {
            return Err(BeadsError::validation("gate", "--gate must not be empty"));
        }
        if !legal.contains(&gate) {
            return Err(BeadsError::validation(
                "gate",
                format!(
                    "--gate '{gate}' would be recorded but never authorize this close \
                     (P{} band); legal close gates for this bead: {}",
                    issue.priority.0,
                    legal.join(", ")
                ),
            ));
        }
        return Ok(gate.to_string());
    }
    match legal.as_slice() {
        [only] => Ok((*only).to_string()),
        _ => Err(BeadsError::validation(
            "gate",
            format!(
                "--gate <kind> is required; legal close gates for this bead: {}",
                legal.join(", ")
            ),
        )),
    }
}

/// One step of the remaining ceremony.
#[derive(Debug, Serialize)]
struct LandStep {
    name: String,
    /// `done` (land performed it) or `next` (operator step, command provided).
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    command: Option<String>,
}

impl LandStep {
    fn done(name: &str, command: String) -> Self {
        Self {
            name: name.to_string(),
            status: "done".to_string(),
            command: Some(command),
        }
    }

    fn next(name: &str, command: String) -> Self {
        Self {
            name: name.to_string(),
            status: "next".to_string(),
            command: Some(command),
        }
    }
}

/// Machine payload for a completed land.
#[derive(Debug, Serialize)]
struct LandOutput {
    issue_id: String,
    gate: String,
    provider: String,
    sha: String,
    note: String,
    closed: bool,
    steps: Vec<LandStep>,
}

/// Machine payload for `br land --dry-run`.
#[derive(Debug, Serialize)]
struct LandDryRun {
    dry_run: bool,
    issue_id: String,
    gate: String,
    provider: String,
    sha: String,
    note: String,
    close: close::CloseDryRunReport,
    steps: Vec<LandStep>,
}

/// Execute `br land`.
///
/// # Errors
///
/// Returns an error when the sha token is malformed, the bead resolves
/// nothing, the gate row cannot be recorded, or the close refuses. A failure
/// AFTER a successful close says so and names the retry (never a silent
/// half-landed ceremony).
pub fn execute(args: &LandArgs, cli: &CliOverrides, ctx: &OutputContext) -> Result<()> {
    let use_json = args.robot || ctx.is_json() || ctx.is_toon();
    let sha = validate_sha_token(&args.commit_sha)?;
    let beads_dir = config::discover_beads_dir_with_cli(cli)?;

    let storage_ctx = config::open_storage_with_cli(&beads_dir, cli)?;
    let config_layer = storage_ctx.load_config(cli)?;
    let actor = config::resolve_actor(&config_layer);
    let id_config = config::id_config_from_layer(&config_layer);
    let resolver = IdResolver::new(ResolverConfig::with_prefix(id_config.prefix));
    let issue_id = super::resolve_issue_id(&storage_ctx.storage, &resolver, &args.id)?;
    let issue =
        storage_ctx
            .storage
            .get_issue(&issue_id)?
            .ok_or_else(|| BeadsError::IssueNotFound {
                id: issue_id.clone(),
            })?;

    let gate_name = resolve_gate_name(args.gate.as_deref(), &issue)?;
    let provider = args
        .provider
        .as_deref()
        .map(str::trim)
        .filter(|provider| !provider.is_empty())
        .map(str::to_string)
        .unwrap_or(actor);
    let note = build_gate_note(&sha, args.receipt.as_deref());

    let close_args = close::CloseArgs {
        ids: vec![issue_id.clone()],
        reason: args.reason.clone(),
        agent_name: args.agent_name.clone(),
        harness: args.harness.clone(),
        model: args.model.clone(),
        commit_sha: Some(sha.clone()),
        repo: args.repo.clone(),
        ..close::CloseArgs::default()
    };

    if args.dry_run {
        let report = close::preview_close_report(&close_args, cli, &beads_dir, &storage_ctx)?;
        let plan = LandPlan {
            issue_id: &issue_id,
            gate: &gate_name,
            provider: &provider,
            sha: &sha,
            note: &note,
        };
        emit_dry_run(args, ctx, use_json, &plan, report);
        return Ok(());
    }

    // 1. Gate row, through the exact gate-report write path.
    let recorded = gate::record_verdict(
        &GateReportArgs {
            id: issue_id.clone(),
            gate: gate_name.clone(),
            provider: provider.clone(),
            status: GateStatus::Pass,
            to: Some("closed".to_string()),
            note: Some(note.clone()),
            robot: false,
        },
        cli,
        &beads_dir,
    )?;

    // 2. Close, through the exact close core (policy gates included).
    let outcome = close::execute_land_close(&close_args, cli, ctx, &beads_dir)?;
    if outcome.closed.is_empty() {
        let reasons = outcome
            .skipped
            .iter()
            .map(|skipped| format!("{}: {}", skipped.id, skipped.reason))
            .collect::<Vec<_>>()
            .join("; ");
        return Err(BeadsError::validation(
            "close",
            format!(
                "land: gate '{}' was recorded (row {}) but the close did not land ({reasons})",
                recorded.gate, recorded.id
            ),
        ));
    }

    // 3. Automatable tail steps, then the operator's remaining ones, in order.
    let mut steps = vec![
        LandStep::done(
            "gate row",
            format!(
                "br gate report {issue_id} --gate {gate_name} --provider {provider} \
                 --status pass --to closed --note '{note}'"
            ),
        ),
        LandStep::done("close", format!("br close {issue_id} --commit-sha {sha}")),
    ];
    steps.extend(live_tail_steps(&issue_id, args, cli)?);

    let output = LandOutput {
        issue_id: issue_id.clone(),
        gate: gate_name.clone(),
        provider: provider.clone(),
        sha: sha.clone(),
        note: note.clone(),
        closed: true,
        steps,
    };
    render_live(use_json, ctx, &output);
    Ok(())
}

/// The identity of one land plan, for dry-run rendering.
struct LandPlan<'a> {
    issue_id: &'a str,
    gate: &'a str,
    provider: &'a str,
    sha: &'a str,
    note: &'a str,
}

/// Build and render the dry-run payload: the plan's steps plus the nested
/// close preview (so "would refuse" shows exactly what the live close would
/// name).
fn emit_dry_run(
    args: &LandArgs,
    ctx: &OutputContext,
    use_json: bool,
    plan: &LandPlan<'_>,
    report: close::CloseDryRunReport,
) {
    let mut steps = vec![
        LandStep::next(
            "gate row",
            format!(
                "br gate report {} --gate {} --provider {} --status pass --to closed \
                 --note '{}'",
                plan.issue_id, plan.gate, plan.provider, plan.note
            ),
        ),
        LandStep::next(
            "close",
            format!("br close {} --commit-sha {}", plan.issue_id, plan.sha),
        ),
    ];
    if !report.would_close {
        // As of NOW the close would refuse (the gate row this land would
        // write is not recorded yet, or something else is missing).
        steps[1].status = "blocked".to_string();
    }
    let project = args.project.as_deref().unwrap_or("<TORON_PROJECT>");
    steps.push(if args.release_leases {
        LandStep::next(
            "release leases",
            format!(
                "would run: toron reserve release-by-reason {} --project {project} --as <pin>",
                plan.issue_id
            ),
        )
    } else {
        LandStep::next(
            "release leases",
            format!(
                "toron reserve release-by-reason {} --project <slug> --as <pin>",
                plan.issue_id
            ),
        )
    });
    steps.push(if args.sync {
        LandStep::next("sync", "would run: br sync --flush-only".to_string())
    } else {
        LandStep::next("sync", "br sync --flush-only".to_string())
    });
    steps.extend(remaining_steps(plan.issue_id));
    let output = LandDryRun {
        dry_run: true,
        issue_id: plan.issue_id.to_string(),
        gate: plan.gate.to_string(),
        provider: plan.provider.to_string(),
        sha: plan.sha.to_string(),
        note: plan.note.to_string(),
        close: report,
        steps,
    };
    render_dry_run(use_json, ctx, &output);
}

/// The automatable tail of a live land: `--release-leases`, `--sync`, then
/// the operator's remaining steps. A failure here states that the close
/// already landed and names the retry — never a silent half-ceremony.
fn live_tail_steps(issue_id: &str, args: &LandArgs, cli: &CliOverrides) -> Result<Vec<LandStep>> {
    let mut steps = Vec::new();
    if args.release_leases {
        match release_leases(issue_id, args.project.as_deref()) {
            Ok(command) => steps.push(LandStep::done("release leases", command)),
            Err(error) => {
                return Err(BeadsError::validation(
                    "release-leases",
                    format!(
                        "land: closed {issue_id}, but --release-leases failed: {error}; the \
                         close LANDED — retry: toron reserve release-by-reason {issue_id} \
                         --project <slug> --as <pin>"
                    ),
                ));
            }
        }
    } else {
        steps.push(LandStep::next(
            "release leases",
            format!("toron reserve release-by-reason {issue_id} --project <slug> --as <pin>"),
        ));
    }
    if args.sync {
        // Quiet context: land renders one document; sync failures still
        // propagate loudly below.
        let quiet = OutputContext::from_flags(false, true, false);
        match sync::execute(
            &SyncArgs {
                flush_only: true,
                ..SyncArgs::default()
            },
            false,
            cli,
            &quiet,
            false,
        ) {
            Ok(()) => steps.push(LandStep::done("sync", "br sync --flush-only".to_string())),
            Err(error) => {
                return Err(BeadsError::validation(
                    "sync",
                    format!(
                        "land: closed {issue_id}, but --sync failed: {error}; the close \
                         LANDED — retry: br sync --flush-only"
                    ),
                ));
            }
        }
    } else {
        steps.push(LandStep::next("sync", "br sync --flush-only".to_string()));
    }
    steps.extend(remaining_steps(issue_id));
    Ok(steps)
}

/// The steps no flag automates here: the bookkeeping commit and the captain
/// mail. Printed with the exact command shapes the ceremony uses.
fn remaining_steps(issue_id: &str) -> Vec<LandStep> {
    vec![
        LandStep::next(
            "tracker bookkeeping",
            "git add .beads && git commit -m 'chore(beads): publish <id> close' \
             (no bead id in the bookkeeping message)"
                .to_string(),
        ),
        LandStep::next(
            "captain mail",
            format!(
                "toron mail send --project <slug> --as <pin> --to captain --thread \
                 {issue_id} --subject '[{issue_id}] done' --body '…'"
            ),
        ),
    ]
}

/// Release the bead's toron leases. Needs the project slug and the acting
/// pin; both come from the ceremony's env ladder, never a guess.
fn release_leases(bead_id: &str, project_arg: Option<&str>) -> Result<String> {
    let project = project_arg
        .map(str::trim)
        .filter(|project| !project.is_empty())
        .map(str::to_string)
        .or_else(|| {
            std::env::var("TORON_PROJECT")
                .ok()
                .map(|project| project.trim().to_string())
                .filter(|project| !project.is_empty())
        })
        .ok_or_else(|| {
            BeadsError::validation(
                "project",
                "--release-leases needs the toron project slug: pass --project <slug> or set \
                 TORON_PROJECT",
            )
        })?;
    let pin = ["TORON_AGENT", "AGENT_NAME", "FLYWHEEL_MAIL_AS"]
        .iter()
        .filter_map(|key| std::env::var(key).ok())
        .map(|pin| pin.trim().to_string())
        .find(|pin| !pin.is_empty())
        .ok_or_else(|| {
            BeadsError::validation(
                "as",
                "--release-leases needs the acting pin: set TORON_AGENT / AGENT_NAME / \
                 FLYWHEEL_MAIL_AS",
            )
        })?;
    let bin = std::env::var("BR_TORON_BIN").unwrap_or_else(|_| "toron".to_string());
    let output = std::process::Command::new(&bin)
        .args([
            "reserve",
            "release-by-reason",
            bead_id,
            "--project",
            &project,
            "--as",
            &pin,
        ])
        .output()
        .map_err(|error| {
            BeadsError::validation("release-leases", format!("cannot run {bin}: {error}"))
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(BeadsError::validation(
            "release-leases",
            format!("{bin} exited {}: {stderr}", output.status),
        ));
    }
    Ok(format!(
        "{bin} reserve release-by-reason {bead_id} --project {project} --as {pin}"
    ))
}

fn render_live(use_json: bool, ctx: &OutputContext, output: &LandOutput) {
    if ctx.is_toon() {
        ctx.toon(output);
    } else if use_json {
        ctx.json_pretty(output);
    } else {
        ctx.success(&format!(
            "landed {}: gate '{}' recorded (provider {}), closed with sha={}",
            sanitize_terminal_inline(&output.issue_id),
            sanitize_terminal_inline(&output.gate),
            sanitize_terminal_inline(&output.provider),
            sanitize_terminal_inline(&output.sha),
        ));
        for step in &output.steps {
            let marker = if step.status == "done" {
                "done"
            } else {
                "next"
            };
            ctx.print_line(&format!(
                "  [{marker}] {}: {}",
                step.name,
                step.command.as_deref().unwrap_or("")
            ));
        }
    }
}

fn render_dry_run(use_json: bool, ctx: &OutputContext, output: &LandDryRun) {
    if ctx.is_toon() {
        ctx.toon(output);
    } else if use_json {
        ctx.json_pretty(output);
    } else {
        ctx.print_line(&format!(
            "land --dry-run {}: gate '{}' (provider {}) would be recorded, then close",
            sanitize_terminal_inline(&output.issue_id),
            sanitize_terminal_inline(&output.gate),
            sanitize_terminal_inline(&output.provider),
        ));
        ctx.print_line(&format!(
            "  note: {}",
            sanitize_terminal_inline(&output.note)
        ));
        for issue in &output.close.issues {
            let verdict = if issue.would_close {
                "would close"
            } else {
                "would refuse"
            };
            ctx.print_line(&format!(
                "  close {}: {verdict}",
                sanitize_terminal_inline(&issue.id)
            ));
            for item in &issue.missing {
                ctx.print_line(&format!("    missing: {}", sanitize_terminal_inline(item)));
            }
        }
        for step in &output.steps {
            ctx.print_line(&format!(
                "  [{}] {}: {}",
                step.status,
                step.name,
                step.command.as_deref().unwrap_or("")
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Issue, IssueType, Priority, Status};

    fn issue_with(priority: Priority, verify: Option<&str>) -> Issue {
        let now = chrono::Utc::now();
        Issue {
            id: "bd-1".to_string(),
            title: "t".to_string(),
            status: Status::Open,
            priority,
            issue_type: IssueType::Task,
            created_at: now,
            updated_at: now,
            verify: verify.map(str::to_string),
            ..Issue::default()
        }
    }

    const SHA: &str = "d1d02dd39a725fb256d49e77a5fcc284e7a200b9";

    #[test]
    fn sha_token_refuses_glued_semicolon() {
        let glued = format!("{SHA};");
        let error = validate_sha_token(&glued).expect_err("a glued `;` must be refused");
        let message = error.to_string();
        assert!(
            message.contains("glued") && message.contains("UNBOUND"),
            "the refusal must name the trap: {message}"
        );
        // The bare sha passes and normalizes case.
        assert_eq!(validate_sha_token(SHA).unwrap(), SHA);
        assert_eq!(validate_sha_token(&SHA.to_uppercase()).unwrap(), SHA);
        // A short sha and non-hex are refused too.
        assert!(validate_sha_token("d1d02dd3").is_err());
        assert!(validate_sha_token("zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz").is_err());
    }

    #[test]
    fn gate_note_carries_sha_and_receipt() {
        assert_eq!(build_gate_note(SHA, None), format!("sha={SHA}"));
        assert_eq!(
            build_gate_note(SHA, Some(".flywheel/receipts/x.txt")),
            format!("sha={SHA} receipt=.flywheel/receipts/x.txt")
        );
        assert_eq!(
            build_gate_note(SHA, Some("   ")),
            format!("sha={SHA}"),
            "a blank receipt must not add a dangling token"
        );
    }

    #[test]
    fn gate_derives_only_from_a_single_legal_name() {
        // P2 + loop-runnable verify: exactly command-verified is legal.
        let runnable = issue_with(
            Priority::MEDIUM,
            Some("cargo test -p beads land_full_ceremony"),
        );
        assert_eq!(
            resolve_gate_name(None, &runnable).unwrap(),
            "command-verified"
        );
        // Explicit wins, trimmed.
        assert_eq!(
            resolve_gate_name(Some(" command-verified "), &runnable).unwrap(),
            "command-verified"
        );
        assert!(resolve_gate_name(Some("  "), &runnable).is_err());
        // An explicit ILLEGAL verdict is refused up front (land would record a
        // row that can never authorize its own close).
        let illegal = resolve_gate_name(Some("unit-test-verified"), &runnable)
            .expect_err("an illegal verdict must be refused");
        assert!(
            illegal.to_string().contains("never authorize"),
            "the refusal must say why: {illegal}"
        );
        // Empty verify (triage band): several legal names, so refuse and name them.
        let triage = issue_with(Priority::MEDIUM, None);
        let error = resolve_gate_name(None, &triage).expect_err("ambiguous band must refuse");
        let message = error.to_string();
        assert!(
            message.contains("legal close gates for this bead"),
            "the refusal must name the options: {message}"
        );
    }
}
