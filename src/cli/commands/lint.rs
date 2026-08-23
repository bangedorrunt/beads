//! Lint command implementation.
//!
//! ADR-0001 §5.2: one brief schema. Lint checks the typed work-ledger fields —
//! non-empty `verify`, and non-empty well-formed `principles` for P≤2 — and
//! no longer requires markdown template headings.

use super::{
    acquire_routed_workspace_write_lock, auto_import_storage_ctx_if_stale,
    cli_for_routed_workspace, resolve_issue_id,
};
use crate::cli::LintArgs;
use crate::config;
use crate::error::{BeadsError, Result};
use crate::format::sanitize_terminal_inline;
use crate::model::{Issue, IssueType, Status};
use crate::output::OutputContext;
use crate::storage::{ListFilters, SqliteStorage};
use crate::util::id::{IdResolver, ResolverConfig};
use rich_rust::prelude::*;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Serialize)]
struct LintResult {
    id: String,
    title: String,
    #[serde(rename = "type")]
    issue_type: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    missing: Vec<String>,
    warnings: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    suggestions: Vec<LintSuggestion>,
}

#[derive(Debug, Serialize)]
struct LintSuggestion {
    section: String,
    hint: String,
}

#[derive(Debug, Serialize)]
struct LintOutput {
    total: usize,
    issues: usize,
    results: Vec<LintResult>,
}

#[derive(Debug)]
struct LintSummary {
    checked: usize,
    warnings: usize,
    results: Vec<LintResult>,
}

impl LintSummary {
    const fn exit_code(&self, structured: bool) -> i32 {
        if structured || self.warnings == 0 {
            0
        } else {
            1
        }
    }
}

/// Execute the lint command.
///
/// # Errors
///
/// Returns an error if database access fails or filters are invalid.
pub fn execute(
    args: &LintArgs,
    _json: bool,
    cli: &config::CliOverrides,
    ctx: &OutputContext,
) -> Result<()> {
    let beads_dir = config::discover_beads_dir_with_cli(cli)?;

    let issues = if args.ids.is_empty() {
        let storage_ctx = config::open_storage_with_cli(&beads_dir, cli)?;
        lint_issues_with_storage(args, &storage_ctx.storage)?
    } else {
        resolve_issues(&beads_dir, args, cli)?
    };

    render_lint_output(lint_issues(&issues), ctx);
    Ok(())
}

/// Execute the all-issues lint scan using storage already opened by the caller.
///
/// Returns `Ok(false)` when explicit issue IDs require the normal routed path.
///
/// # Errors
///
/// Returns an error if database access fails or filters are invalid.
pub fn execute_with_storage_ctx(
    args: &LintArgs,
    ctx: &OutputContext,
    storage_ctx: &config::OpenStorageResult,
) -> Result<bool> {
    if !args.ids.is_empty() {
        return Ok(false);
    }

    let issues = lint_issues_with_storage(args, &storage_ctx.storage)?;
    render_lint_output(lint_issues(&issues), ctx);
    Ok(true)
}

fn lint_issues_with_storage(args: &LintArgs, storage: &SqliteStorage) -> Result<Vec<Issue>> {
    let filters = build_filters(args)?;
    storage.list_lint_issues_for_command_output(&filters)
}

fn render_lint_output(summary: LintSummary, ctx: &OutputContext) {
    if ctx.is_toon() {
        let output = LintOutput {
            total: summary.warnings,
            issues: summary.results.len(),
            results: summary.results,
        };
        ctx.toon(&output);
        return;
    }

    if ctx.is_json() {
        let output = LintOutput {
            total: summary.warnings,
            issues: summary.results.len(),
            results: summary.results,
        };
        ctx.json_pretty(&output);
        return;
    }

    if ctx.is_quiet() {
        if summary.results.is_empty() {
            return;
        }
        std::process::exit(summary.exit_code(false));
    }

    if ctx.is_rich() {
        render_lint_rich(&summary, ctx);
    } else {
        if summary.results.is_empty() {
            println!(
                "✓ No template warnings found ({} issues checked)",
                summary.checked
            );
            return;
        }

        println!(
            "Template warnings ({} issues, {} warnings):\n",
            summary.results.len(),
            summary.warnings
        );
        for result in &summary.results {
            println!(
                "{} [{}]: {}",
                result.id,
                result.issue_type,
                sanitize_terminal_inline(&result.title)
            );
            for suggestion in &result.suggestions {
                println!("  ⚠ Missing: {} - {}", suggestion.section, suggestion.hint);
            }
            println!();
        }
    }

    std::process::exit(summary.exit_code(false));
}

fn render_lint_rich(summary: &LintSummary, ctx: &OutputContext) {
    let theme = ctx.theme();
    let mut content = Text::new("");

    content.append_styled("Template Lint\n", theme.emphasis.clone());
    content.append("\n");

    content.append_styled("Checked: ", theme.dimmed.clone());
    content.append_styled(&summary.checked.to_string(), theme.emphasis.clone());
    content.append_styled("    Warnings: ", theme.dimmed.clone());
    if summary.warnings == 0 {
        content.append_styled("0", theme.success.clone());
    } else {
        content.append_styled(&summary.warnings.to_string(), theme.warning.clone());
    }
    content.append("\n\n");

    if summary.results.is_empty() {
        content.append_styled(
            &format!(
                "✓ No template warnings found ({} issues checked)",
                summary.checked
            ),
            theme.success.clone(),
        );
    } else {
        let mut by_type: BTreeMap<&str, Vec<&LintResult>> = BTreeMap::new();
        for result in &summary.results {
            by_type
                .entry(result.issue_type.as_str())
                .or_default()
                .push(result);
        }

        for (issue_type, results) in by_type {
            content.append_styled(
                &format!(
                    "{} ({})\n",
                    sanitize_terminal_inline(issue_type),
                    results.len()
                ),
                theme.section.clone(),
            );
            for result in results {
                content.append_styled("- ", theme.warning.clone());
                content.append_styled(&result.id, theme.issue_id.clone());
                content.append(" ");
                content.append_styled(
                    &format!("[{}] ", sanitize_terminal_inline(&result.issue_type)),
                    issue_type_style(theme, &result.issue_type),
                );
                content.append_styled(
                    sanitize_terminal_inline(&result.title).as_ref(),
                    theme.issue_title.clone(),
                );
                content.append("\n");

                for suggestion in &result.suggestions {
                    content.append_styled("    missing: ", theme.dimmed.clone());
                    content.append_styled(&suggestion.section, theme.warning.clone());
                    content.append_styled(" - ", theme.dimmed.clone());
                    content.append_styled(&suggestion.hint, theme.dimmed.clone());
                    content.append("\n");
                }
            }
            content.append("\n");
        }

        content.append_styled(
            "Tip: Add the missing sections to issue descriptions to clear warnings.\n",
            theme.dimmed.clone(),
        );
    }

    let panel = Panel::from_rich_text(&content, ctx.width())
        .title(Text::styled("Lint Results", theme.panel_title.clone()))
        .box_style(theme.box_style)
        .border_style(theme.panel_border.clone());

    ctx.render(&panel);
}

fn issue_type_style(theme: &crate::output::Theme, issue_type: &str) -> Style {
    match issue_type {
        "task" => theme.type_task.clone(),
        "bug" => theme.type_bug.clone(),
        "feature" => theme.type_feature.clone(),
        "epic" => theme.type_epic.clone(),
        "chore" => theme.type_chore.clone(),
        "docs" => theme.type_docs.clone(),
        "question" => theme.type_question.clone(),
        _ => theme.dimmed.clone(),
    }
}

fn build_filters(args: &LintArgs) -> Result<ListFilters> {
    let mut filters = ListFilters {
        include_templates: false,
        ..ListFilters::default()
    };

    if let Some(ref type_str) = args.type_ {
        let issue_type: IssueType = type_str.parse()?;
        // bd conformance: CLI rejects custom/unknown types
        if !issue_type.is_standard() {
            return Err(BeadsError::InvalidType {
                issue_type: type_str.clone(),
            });
        }
        filters.types = Some(vec![issue_type]);
    }

    let status_filter = args.status.as_deref().unwrap_or("open").trim();
    if !status_filter.is_empty() && !status_filter.eq_ignore_ascii_case("all") {
        let status: Status = status_filter.parse()?;
        if status.is_terminal() {
            filters.include_closed = true;
        }
        if status == Status::Deferred {
            filters.include_deferred = true;
        }
        filters.statuses = Some(vec![status]);
    } else if status_filter.eq_ignore_ascii_case("all") {
        filters.include_closed = true;
        filters.include_deferred = true;
    }

    Ok(filters)
}

fn resolve_issues(
    beads_dir: &Path,
    args: &LintArgs,
    cli: &config::CliOverrides,
) -> Result<Vec<Issue>> {
    let routed_batches = config::routing::group_issue_inputs_by_route(&args.ids, beads_dir)?;
    let mut issues_by_input = std::collections::HashMap::new();

    for batch in routed_batches {
        let mut batch_cli = routed_cli_for_batch(cli, batch.is_external);
        let routed_write_lock = acquire_routed_workspace_write_lock(
            &batch.beads_dir,
            batch.is_external,
            batch_cli.lock_timeout,
        )?;
        routed_write_lock.mark_cli_write_lock_held(&mut batch_cli);
        let mut storage_ctx = config::open_storage_with_cli(&batch.beads_dir, &batch_cli)?;
        auto_import_storage_ctx_if_stale(&mut storage_ctx, &batch_cli)?;
        let config_layer = storage_ctx.load_config(&batch_cli)?;
        let id_config = config::id_config_from_layer(&config_layer);
        let resolver = IdResolver::new(ResolverConfig::with_prefix(id_config.prefix));

        let mut resolved_ids = Vec::with_capacity(batch.issue_inputs.len());
        for id_input in &batch.issue_inputs {
            resolved_ids.push(resolve_issue_id(&storage_ctx.storage, &resolver, id_input)?);
        }

        let issues = fetch_issues_in_resolved_order(&storage_ctx.storage, &resolved_ids)?;
        for (input, issue) in batch.issue_inputs.into_iter().zip(issues) {
            issues_by_input.insert(input, issue);
        }
    }

    args.ids
        .iter()
        .map(|input| {
            issues_by_input
                .get(input)
                .cloned()
                .ok_or_else(|| BeadsError::IssueNotFound { id: input.clone() })
        })
        .collect()
}

fn fetch_issues_in_resolved_order(
    storage: &SqliteStorage,
    resolved_ids: &[String],
) -> Result<Vec<Issue>> {
    let issues_by_id = storage
        .get_issues_by_ids(resolved_ids)?
        .into_iter()
        .map(|issue| (issue.id.clone(), issue))
        .collect::<std::collections::HashMap<_, _>>();

    resolved_ids
        .iter()
        .map(|id| {
            issues_by_id
                .get(id)
                .cloned()
                .ok_or_else(|| BeadsError::IssueNotFound { id: id.clone() })
        })
        .collect()
}

fn routed_cli_for_batch(cli: &config::CliOverrides, is_external: bool) -> config::CliOverrides {
    cli_for_routed_workspace(cli, is_external)
}

fn lint_issues(issues: &[Issue]) -> LintSummary {
    let mut warnings = 0;
    let mut results = Vec::new();

    for issue in issues {
        if let Some(result) = lint_issue(issue) {
            warnings += result.warnings;
            results.push(result);
        }
    }

    LintSummary {
        checked: issues.len(),
        warnings,
        results,
    }
}

/// Kebab-case principle name: lowercase alphanumeric segments joined by
/// single hyphens (`^[a-z0-9]+(-[a-z0-9]+)*$`).
fn is_kebab_case(name: &str) -> bool {
    !name.is_empty()
        && name.split('-').all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}

fn lint_issue(issue: &Issue) -> Option<LintResult> {
    let mut problems: Vec<LintProblem> = Vec::new();

    if issue
        .verify
        .as_deref()
        .is_none_or(|verify| verify.trim().is_empty())
    {
        problems.push(LintProblem {
            section: "verify".to_string(),
            hint: "Set a VERIFY command that proves this bead done (br update --verify)"
                .to_string(),
        });
    }

    if issue.priority <= crate::model::Priority::MEDIUM && issue.principles.is_empty() {
        problems.push(LintProblem {
            section: "principles".to_string(),
            hint: "Cite at least one engineering principle with the decision it changed (br update --principle 'name — decision')".to_string(),
        });
    }

    for citation in &issue.principles {
        if !is_kebab_case(&citation.name) {
            problems.push(LintProblem {
                section: format!("principles:{}", citation.name),
                hint: "Principle names are kebab-case (lowercase alphanumeric, single hyphens)"
                    .to_string(),
            });
        }
        if citation.decision.trim().is_empty() {
            problems.push(LintProblem {
                section: format!("principles:{}", citation.name),
                hint: "Each principle cites the concrete decision it changed; empty decisions are skipped".to_string(),
            });
        }
    }

    if problems.is_empty() {
        return None;
    }

    let missing = problems.iter().map(|p| p.section.clone()).collect();
    let suggestions = problems
        .iter()
        .map(|p| LintSuggestion {
            section: p.section.clone(),
            hint: p.hint.clone(),
        })
        .collect();

    Some(LintResult {
        id: issue.id.clone(),
        title: issue.title.clone(),
        issue_type: issue.issue_type.as_str().to_string(),
        warnings: problems.len(),
        missing,
        suggestions,
    })
}

#[derive(Debug, Clone)]
struct LintProblem {
    section: String,
    hint: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use tempfile::TempDir;

    fn make_issue(issue_type: IssueType, description: Option<&str>) -> Issue {
        Issue {
            verify: None,
            principles: Vec::new(),
            wave: None,
            pin: None,
            commit_sha: None,
            close_verdict: None,
            ac_shape: crate::model::AcShape::Checkable,
            blast: crate::model::Blast::Normal,
            id: "bd-123".to_string(),
            content_hash: None,
            title: "Sample".to_string(),
            description: description.map(str::to_string),
            design: None,
            acceptance_criteria: None,
            notes: None,
            status: Status::Open,
            priority: crate::model::Priority::MEDIUM,
            issue_type,
            assignee: None,
            owner: None,
            estimated_minutes: None,
            created_at: Utc::now(),
            created_by: None,
            updated_at: Utc::now(),
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

    #[test]
    fn lint_does_not_require_acceptance_criteria_heading() {
        // ADR-0001 §5.2: heading sections are dead; a bug whose description
        // carries no "## Acceptance Criteria" / "## Steps to Reproduce" text
        // lints clean as long as the typed fields satisfy the brief schema.
        let mut issue = make_issue(IssueType::Bug, Some("Bug report with no headings"));
        issue.priority = crate::model::Priority::HIGH;
        issue.verify = Some("cargo test --offline lint".to_string());
        issue.principles = vec![crate::model::PrincipleCitation {
            name: "prove-it-works".to_string(),
            decision: "lint contract carried by unit tests, not heading grep".to_string(),
        }];
        assert!(lint_issue(&issue).is_none());
    }

    #[test]
    fn lint_requires_non_empty_verify() {
        let mut missing = make_issue(IssueType::Task, None);
        missing.priority = crate::model::Priority::LOW;
        let result = lint_issue(&missing).expect("lint result");
        assert_eq!(result.warnings, 1);
        assert!(result.missing.contains(&"verify".to_string()));

        let mut blank = make_issue(IssueType::Task, None);
        blank.priority = crate::model::Priority::LOW;
        blank.verify = Some("   ".to_string());
        let result = lint_issue(&blank).expect("lint result");
        assert_eq!(result.warnings, 1);
        assert!(result.missing.contains(&"verify".to_string()));
    }

    #[test]
    fn lint_requires_principles_for_priority_at_or_below_two() {
        let p2 = make_issue(IssueType::Task, None);
        let mut p2 = p2;
        p2.verify = Some("cargo test".to_string());
        let result = lint_issue(&p2).expect("lint result");
        assert_eq!(result.warnings, 1);
        assert!(result.missing.contains(&"principles".to_string()));
    }

    #[test]
    fn lint_skips_principles_requirement_above_priority_two() {
        let mut p3 = make_issue(IssueType::Task, None);
        p3.verify = Some("cargo test".to_string());
        p3.priority = crate::model::Priority::LOW;
        assert!(lint_issue(&p3).is_none());
    }

    #[test]
    fn lint_flags_non_kebab_case_principle_names() {
        let mut issue = make_issue(IssueType::Task, None);
        issue.verify = Some("cargo test".to_string());
        issue.principles = vec![crate::model::PrincipleCitation {
            name: "Fix Root Causes".to_string(),
            decision: "chose storage-layer fix over CLI shim".to_string(),
        }];
        let result = lint_issue(&issue).expect("lint result");
        assert_eq!(result.warnings, 1);
        assert!(
            result
                .missing
                .iter()
                .any(|m| m.starts_with("principles:") && m.contains("Fix Root Causes"))
        );
        assert!(
            result
                .suggestions
                .iter()
                .any(|s| s.hint.contains("kebab-case"))
        );
    }

    #[test]
    fn lint_flags_empty_principle_decisions() {
        let mut issue = make_issue(IssueType::Task, None);
        issue.verify = Some("cargo test".to_string());
        issue.principles = vec![
            crate::model::PrincipleCitation {
                name: "fix-root-causes".to_string(),
                decision: String::new(),
            },
            crate::model::PrincipleCitation {
                name: "prove-it-works".to_string(),
                decision: "   ".to_string(),
            },
        ];
        let result = lint_issue(&issue).expect("lint result");
        assert_eq!(result.warnings, 2);
    }

    #[test]
    fn lint_accepts_well_formed_brief() {
        let mut issue = make_issue(IssueType::Bug, Some("## VERIFY\ncargo test"));
        issue.priority = crate::model::Priority::CRITICAL;
        issue.verify = Some("timeout 900 cargo test --offline lint".to_string());
        issue.principles = vec![crate::model::PrincipleCitation {
            name: "subtract-before-you-add".to_string(),
            decision: "dual lint templates die; one brief schema".to_string(),
        }];
        assert!(lint_issue(&issue).is_none());
    }

    #[test]
    fn lint_issues_with_storage_matches_full_hydration_results() {
        let mut storage = SqliteStorage::open_memory().unwrap();
        let now = Utc.with_ymd_and_hms(2026, 5, 3, 12, 0, 0).unwrap();

        let mut missing = make_issue(IssueType::Task, Some("Needs a real section"));
        missing.id = "bd-lint-missing".to_string();
        missing.title = "Lint missing section".to_string();
        missing.created_at = now;
        missing.updated_at = now;
        missing.design = Some("unused design".repeat(512));
        missing.acceptance_criteria = Some("unused criteria".repeat(512));
        missing.notes = Some("unused notes".repeat(512));
        missing.owner = Some("owner".to_string());
        missing.sender = Some("cli".to_string());

        let mut complete = make_issue(
            IssueType::Task,
            Some("## Acceptance Criteria\n- Already present"),
        );
        complete.id = "bd-lint-complete".to_string();
        complete.title = "Lint complete section".to_string();
        complete.created_at = now;
        complete.updated_at = now;

        storage.create_issue(&missing, "tester").unwrap();
        storage.create_issue(&complete, "tester").unwrap();

        let args = LintArgs::default();
        let filters = build_filters(&args).unwrap();
        let full_summary = lint_issues(&storage.list_issues(&filters).unwrap());
        let projected_raw = lint_issues_with_storage(&args, &storage).unwrap();
        let projected_issue = projected_raw
            .iter()
            .find(|issue| issue.id == "bd-lint-missing")
            .unwrap();
        assert!(projected_issue.design.is_none());
        assert!(projected_issue.acceptance_criteria.is_none());
        assert!(projected_issue.notes.is_none());
        assert!(projected_issue.owner.is_none());
        assert!(projected_issue.sender.is_none());

        let projected_summary = lint_issues(&projected_raw);
        assert_eq!(projected_summary.checked, full_summary.checked);
        assert_eq!(projected_summary.warnings, full_summary.warnings);
        assert_eq!(
            serde_json::to_value(projected_summary.results).unwrap(),
            serde_json::to_value(full_summary.results).unwrap()
        );
    }

    #[test]
    fn fetch_issues_in_resolved_order_preserves_duplicate_ids() {
        let temp = TempDir::new().expect("tempdir");
        let db_path = temp.path().join("beads.db");
        let mut storage = SqliteStorage::open(&db_path).expect("storage");
        storage
            .create_issue(&make_issue(IssueType::Bug, Some("Bug report")), "tester")
            .expect("create issue");

        let duplicate_ids = vec!["bd-123".to_string(), "bd-123".to_string()];
        let issues =
            fetch_issues_in_resolved_order(&storage, &duplicate_ids).expect("duplicate lookup");

        assert_eq!(issues.len(), 2);
        assert_eq!(issues[0].id, "bd-123");
        assert_eq!(issues[1].id, "bd-123");
    }

    #[test]
    fn test_exit_code_behavior() {
        let issue = make_issue(IssueType::Task, Some("No criteria"));
        let summary = lint_issues(&[issue]);
        assert_eq!(summary.exit_code(true), 0);
        assert_eq!(summary.exit_code(false), 1);
    }
}
