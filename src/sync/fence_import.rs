//! ADR-0001 §5.2 one-shot fence import (`br sync --import-only` +
//! `br doctor --repair`, beads_rust-fence-import-ou8g).
//!
//! Legacy beads carry their brief as markdown fences inside `description`
//! (`## VERIFY` heading + one fenced one-line command; `## PRINCIPLES`
//! heading + `name — decision` lines). This module migrates those fences
//! into the typed schema-v18 fields EXACTLY once: a field is only written
//! while it is still empty, so later fence edits are ignored by
//! construction — the typed field is the single source of truth. The
//! description itself is never modified (lossless).
//!
//! This is the ONLY place that parses brief markdown off a description;
//! ready/close/lint consume the typed fields exclusively (ADR-0001 §8.3).

// governed-by: ADR-0001

use crate::error::Result;
use crate::model::{Issue, PrincipleCitation};
use crate::storage::SqliteStorage;
use serde::Serialize;

/// Summary of one [`sync_fences_into_typed_fields`] pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FenceImportOutcome {
    /// Issues whose typed fields were stamped this pass.
    pub issues_updated: usize,
}

/// What a description's fences yield.
#[derive(Debug, Default, PartialEq, Eq)]
struct ParsedFences {
    verify_command: Option<String>,
    citations: Vec<PrincipleCitation>,
}

/// Parse the `## VERIFY` and `## PRINCIPLES` fences out of a description,
/// using flywheel's `parse_verify_fence` shape rules: the VERIFY section is
/// legal only when its fenced block holds exactly one non-empty line (the
/// command); PRINCIPLES lines must match `^([a-z0-9-]+) — (.+)$`.
#[must_use]
pub fn parse_description_fences(description: &str) -> (Option<String>, Vec<PrincipleCitation>) {
    let parsed = parse_fences_impl(description);
    (parsed.verify_command, parsed.citations)
}

fn parse_fences_impl(description: &str) -> ParsedFences {
    let mut out = ParsedFences::default();
    let mut lines = description.lines().peekable();

    while let Some(line) = lines.next() {
        match line.trim() {
            "## VERIFY" => {
                out.verify_command = extract_single_fenced_line(&mut lines);
            }
            "## PRINCIPLES" => {
                for line in lines.by_ref() {
                    let trimmed = line.trim();
                    if trimmed.starts_with("## ") {
                        break;
                    }
                    if let Some(citation) = parse_principle_line(trimmed) {
                        out.citations.push(citation);
                    }
                }
            }
            _ => {}
        }
        if out.verify_command.is_some() && !out.citations.is_empty() {
            // Both found; nothing below can contribute.
            break;
        }
    }
    out
}

/// Consume lines up to and including the closing fence of the NEXT fenced
/// block. Returns the single non-empty interior line, or `None` when the
/// block is absent, unterminated, or does not hold exactly one command line.
fn extract_single_fenced_line(
    lines: &mut std::iter::Peekable<std::str::Lines<'_>>,
) -> Option<String> {
    for line in lines.by_ref() {
        if line.trim_start().starts_with("```") {
            let mut interior: Vec<&str> = Vec::new();
            for inner in lines.by_ref() {
                if inner.trim_start().starts_with("```") {
                    let commands: Vec<&str> = interior
                        .into_iter()
                        .filter(|l| !l.trim().is_empty())
                        .collect();
                    return match commands.len() {
                        1 => Some(commands[0].trim().to_string()),
                        _ => None,
                    };
                }
                interior.push(inner);
            }
            // Unterminated fence: not a legal VERIFY fence.
            return None;
        }
    }
    None
}

/// `^([a-z0-9-]+) — (.+)$` — canonical kebab name, em-dash separator, then
/// the decision. Anything else on a PRINCIPLES line is prose, not a citation.
fn parse_principle_line(line: &str) -> Option<PrincipleCitation> {
    let (name, decision) = line.split_once(" — ")?;
    let name = name.trim();
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return None;
    }
    let decision = decision.trim();
    if decision.is_empty() {
        return None;
    }
    Some(PrincipleCitation {
        name: name.to_string(),
        decision: decision.to_string(),
    })
}

/// Stamp one issue's empty typed fields from its description fences.
///
/// Returns `true` when anything was written (one-shot semantics: once a
/// field is set — here or by any earlier write — its fence is ignored
/// forever).
fn apply_to_issue(issue: &mut Issue) -> bool {
    let parsed = parse_fences_impl(issue.description.as_deref().unwrap_or_default());
    let mut changed = false;

    let verify_is_empty = issue
        .verify
        .as_deref()
        .unwrap_or_default()
        .trim()
        .is_empty();
    if verify_is_empty && let Some(command) = parsed.verify_command {
        issue.verify = Some(command);
        changed = true;
    }
    if issue.principles.is_empty() && !parsed.citations.is_empty() {
        issue.principles = parsed.citations;
        changed = true;
    }
    changed
}

/// One-shot migration pass over the whole tracker: every issue whose
/// description carries a fence and whose typed fields are still empty gets
/// stamped. Called from `br sync --import-only` (a clone materializing a
/// legacy JSONL) and `br doctor --repair`. Idempotent; safe to re-run.
///
/// # Errors
///
/// Returns an error if reading or updating any issue fails.
pub fn sync_fences_into_typed_fields(
    storage: &mut SqliteStorage,
    actor: &str,
) -> Result<FenceImportOutcome> {
    let candidate_ids = storage.get_fence_import_candidate_ids()?;
    let mut updated = 0usize;

    for id in candidate_ids {
        let Some(mut issue) = storage.get_issue(&id)? else {
            continue;
        };
        if apply_to_issue(&mut issue) {
            let updates = crate::storage::IssueUpdate {
                verify: issue.verify.clone().map(Some),
                principles_append: issue.principles.clone(),
                skip_cache_rebuild: true,
                ..crate::storage::IssueUpdate::default()
            };
            storage.update_issue(&id, &updates, actor)?;
            updated += 1;
        }
    }
    Ok(FenceImportOutcome {
        issues_updated: updated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_extracts_single_command_and_citations() {
        let description = "Body text.\n\n## VERIFY\nnoise before fence\n\
            ```\ntimeout 600 cargo test --offline x\n```\ntrailer\n\n\
            ## PRINCIPLES\nprove-it-works — proof survives git clone\n\
            Bad Name — rejected uppercase\nnodash\n";
        let (verify, citations) = parse_description_fences(description);
        assert_eq!(
            verify.as_deref(),
            Some("timeout 600 cargo test --offline x")
        );
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0].name, "prove-it-works");
    }

    #[test]
    fn multi_line_or_missing_fence_is_illegal() {
        assert_eq!(
            parse_description_fences("## VERIFY\n```\nline one\nline two\n```\n").0,
            None
        );
        assert_eq!(
            parse_description_fences("## VERIFY\nno fence at all\n").0,
            None
        );
        assert_eq!(
            parse_description_fences("## VERIFY\n```\nunterminated\n").0,
            None
        );
    }

    #[test]
    fn apply_is_one_shot_per_field() {
        let mut issue = Issue {
            description: Some("## VERIFY\n```\ncargo build\n```\n".to_string()),
            ..Issue::default()
        };
        assert!(apply_to_issue(&mut issue));
        assert_eq!(issue.verify.as_deref(), Some("cargo build"));

        // Later fence edit ignored: field already set.
        issue.description = Some("## VERIFY\n```\ncargo test\n```\n".to_string());
        assert!(!apply_to_issue(&mut issue));
        assert_eq!(issue.verify.as_deref(), Some("cargo build"));
    }
}
