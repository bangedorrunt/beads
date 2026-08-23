//! E2E tests for the `lint` command.
//!
//! ADR-0001 §5.2 one brief schema: lint checks the typed work-ledger fields
//! (non-empty `verify`; P≤2 non-empty, well-formed `principles`) and no
//! longer requires markdown template headings.
//!
//! Test coverage:
//! - Clean workspace scenarios (no warnings)
//! - Typed-field warnings (verify, principles, kebab-case names)
//! - Filter tests (--type, --status, specific IDs)
//! - JSON output structure verification
//! - Error handling (before init, invalid filters)

mod common;

use common::cli::{BrWorkspace, extract_json_payload, run_br};
use serde_json::Value;

// =============================================================================
// Helper Functions
// =============================================================================

fn parse_created_id(stdout: &str) -> String {
    let line = stdout.lines().next().unwrap_or("");
    // Handle both formats: "Created bd-xxx: title" and "✓ Created bd-xxx: title"
    let normalized = line.strip_prefix("✓ ").unwrap_or(line);
    let id_part = normalized
        .strip_prefix("Created ")
        .and_then(|rest| rest.split(':').next())
        .unwrap_or("");
    id_part.trim().to_string()
}

fn init_workspace(workspace: &BrWorkspace) {
    let init = run_br(workspace, ["init"], "init");
    assert!(init.status.success(), "init failed: {}", init.stderr);
}

fn create_issue_with_description(
    workspace: &BrWorkspace,
    title: &str,
    issue_type: &str,
    description: Option<&str>,
) -> String {
    let mut args: Vec<String> = vec![
        "create".to_string(),
        title.to_string(),
        "--type".to_string(),
        issue_type.to_string(),
    ];

    if let Some(desc) = description {
        args.push("--description".to_string());
        args.push(desc.to_string());
    }

    let create = run_br(workspace, &args, &format!("create_{issue_type}"));
    assert!(create.status.success(), "create failed: {}", create.stderr);
    parse_created_id(&create.stdout)
}

// =============================================================================
// Clean Workspace Tests
// =============================================================================

#[test]
fn e2e_lint_clean_workspace_no_issues() {
    let _log = common::test_log("e2e_lint_clean_workspace_no_issues");
    // Lint on empty workspace (no issues) should pass with no warnings
    let workspace = BrWorkspace::new();
    init_workspace(&workspace);

    let lint = run_br(&workspace, ["lint"], "lint_empty");
    assert!(lint.status.success(), "lint failed: {}", lint.stderr);
    assert!(
        lint.stdout.contains("No template warnings found"),
        "expected clean message, got: {}",
        lint.stdout
    );
}

#[test]
fn e2e_lint_clean_workspace_json_empty_results() {
    let _log = common::test_log("e2e_lint_clean_workspace_json_empty_results");
    // JSON output on empty workspace should have empty results array
    let workspace = BrWorkspace::new();
    init_workspace(&workspace);

    let lint = run_br(&workspace, ["lint", "--json"], "lint_empty_json");
    assert!(lint.status.success(), "lint failed: {}", lint.stderr);

    let json_str = extract_json_payload(&lint.stdout);
    let json: Value = serde_json::from_str(&json_str).expect("valid JSON");

    assert_eq!(json["total"], 0, "expected 0 warnings");
    assert_eq!(json["issues"], 0, "expected 0 issues with warnings");
    assert!(
        json["results"].as_array().unwrap().is_empty(),
        "expected empty results array"
    );
}

#[test]
fn e2e_lint_issue_with_all_required_sections_passes() {
    let _log = common::test_log("e2e_lint_issue_with_all_required_sections_passes");
    // A bug with a well-formed brief (verify + principle, no headings) lints clean
    let workspace = BrWorkspace::new();
    init_workspace(&workspace);

    let create = run_br(
        &workspace,
        [
            "create",
            "Complete bug",
            "--type",
            "bug",
            "--priority",
            "3",
            "--verify",
            "cargo build",
            "--principle",
            "prove-it-works — lint contract carried by typed fields",
        ],
        "create_complete_bug",
    );
    assert!(create.status.success(), "create failed: {}", create.stderr);

    let lint = run_br(&workspace, ["lint"], "lint_complete_bug");
    assert!(lint.status.success(), "lint failed: {}", lint.stderr);
    assert!(
        lint.stdout.contains("No template warnings found"),
        "expected no warnings for complete bug, got: {}",
        lint.stdout
    );
}

// =============================================================================
// Typed Brief Field Tests
// =============================================================================

#[test]
fn e2e_lint_missing_verify_warns() {
    let _log = common::test_log("e2e_lint_missing_verify_warns");
    // An issue without a VERIFY command should warn on `verify`
    let workspace = BrWorkspace::new();
    init_workspace(&workspace);

    let id = create_issue_with_description(&workspace, "No verify", "bug", Some("Some bug"));

    let lint = run_br(&workspace, ["lint", "--json"], "lint_bug_missing_verify");
    // In JSON mode, exit code is always 0
    assert!(lint.status.success(), "lint failed: {}", lint.stderr);

    let json_str = extract_json_payload(&lint.stdout);
    let json: Value = serde_json::from_str(&json_str).expect("valid JSON");

    assert!(
        json["total"].as_i64().unwrap() >= 1,
        "expected at least 1 warning"
    );

    let results = json["results"].as_array().unwrap();
    let issue_result = results.iter().find(|r| r["id"] == id);
    assert!(issue_result.is_some(), "issue {id} not in results");

    let missing = issue_result.unwrap()["missing"].as_array().unwrap();
    assert!(
        missing.iter().any(|m| m.as_str().unwrap() == "verify"),
        "expected missing 'verify', got: {missing:?}"
    );
}

#[test]
fn e2e_lint_priority_at_or_below_two_requires_principles() {
    let _log = common::test_log("e2e_lint_priority_at_or_below_two_requires_principles");
    // P2 issue with verify but no principles should warn on `principles`
    let workspace = BrWorkspace::new();
    init_workspace(&workspace);

    let create = run_br(
        &workspace,
        ["create", "P2 no principles", "--verify", "cargo build"],
        "create_p2_no_principles",
    );
    assert!(create.status.success(), "create failed: {}", create.stderr);
    let id = parse_created_id(&create.stdout);

    let lint = run_br(&workspace, ["lint", "--json"], "lint_p2_no_principles");
    assert!(lint.status.success(), "lint failed: {}", lint.stderr);

    let json_str = extract_json_payload(&lint.stdout);
    let json: Value = serde_json::from_str(&json_str).expect("valid JSON");

    let results = json["results"].as_array().unwrap();
    let issue_result = results.iter().find(|r| r["id"] == id);
    assert!(issue_result.is_some(), "issue {id} not in results");

    let missing = issue_result.unwrap()["missing"].as_array().unwrap();
    assert!(
        missing.iter().any(|m| m.as_str().unwrap() == "principles"),
        "expected missing 'principles', got: {missing:?}"
    );
}

#[test]
fn e2e_lint_non_kebab_principle_name_warns() {
    let _log = common::test_log("e2e_lint_non_kebab_principle_name_warns");
    let workspace = BrWorkspace::new();
    init_workspace(&workspace);

    let create = run_br(
        &workspace,
        [
            "create",
            "Bad principle name",
            "--priority",
            "3",
            "--verify",
            "cargo build",
        ],
        "create_bad_principle",
    );
    assert!(create.status.success(), "create failed: {}", create.stderr);
    let id = parse_created_id(&create.stdout);

    let update = run_br(
        &workspace,
        [
            "update",
            &id,
            "--principle",
            "Fix Root Causes — chose storage-layer fix over CLI shim",
        ],
        "update_bad_principle",
    );
    assert!(
        !update.status.success(),
        "non-kebab-case citation must be rejected at the CLI boundary"
    );
}

#[test]
fn e2e_lint_epic_without_verify_warns() {
    let _log = common::test_log("e2e_lint_epic_without_verify_warns");
    // One brief schema: epics lint under the same rules as every other type
    let workspace = BrWorkspace::new();
    init_workspace(&workspace);

    let id = create_issue_with_description(
        &workspace,
        "Epic without verify",
        "epic",
        Some("Big project"),
    );

    let lint = run_br(&workspace, ["lint", "--json"], "lint_epic_missing_sc");
    assert!(lint.status.success(), "lint failed: {}", lint.stderr);

    let json_str = extract_json_payload(&lint.stdout);
    let json: Value = serde_json::from_str(&json_str).expect("valid JSON");

    let results = json["results"].as_array().unwrap();
    let issue_result = results.iter().find(|r| r["id"] == id);
    assert!(issue_result.is_some(), "issue {id} not in results");

    let missing = issue_result.unwrap()["missing"].as_array().unwrap();
    assert!(
        missing.iter().any(|m| m.as_str().unwrap() == "verify"),
        "expected missing 'verify' for epic, got: {missing:?}"
    );
}

#[test]
fn e2e_lint_chore_with_well_formed_brief_passes() {
    let _log = common::test_log("e2e_lint_chore_with_well_formed_brief_passes");
    let workspace = BrWorkspace::new();
    init_workspace(&workspace);

    let create = run_br(
        &workspace,
        [
            "create",
            "Simple chore",
            "--type",
            "chore",
            "--priority",
            "3",
            "--verify",
            "cargo build",
            "--principle",
            "subtract-before-you-add — no extra template machinery",
        ],
        "create_chore_clean",
    );
    assert!(create.status.success(), "create failed: {}", create.stderr);

    let lint = run_br(&workspace, ["lint"], "lint_chore_no_sections");
    assert!(lint.status.success(), "lint failed: {}", lint.stderr);
    assert!(
        lint.stdout.contains("No template warnings found"),
        "chore with a well-formed brief should be clean, got: {}",
        lint.stdout
    );
}

#[test]
fn e2e_lint_bug_missing_all_typed_fields() {
    let _log = common::test_log("e2e_lint_bug_missing_all_sections");
    // Bare P2 bug: both verify and principles warn
    let workspace = BrWorkspace::new();
    init_workspace(&workspace);

    let id = create_issue_with_description(&workspace, "Bare bug", "bug", Some("Just a bug"));

    let lint = run_br(&workspace, ["lint", "--json"], "lint_bug_missing_all");
    assert!(lint.status.success(), "lint failed: {}", lint.stderr);

    let json_str = extract_json_payload(&lint.stdout);
    let json: Value = serde_json::from_str(&json_str).expect("valid JSON");

    let results = json["results"].as_array().unwrap();
    let issue_result = results.iter().find(|r| r["id"] == id);
    assert!(issue_result.is_some(), "issue {id} not in results");

    let warnings = issue_result.unwrap()["warnings"].as_i64().unwrap();
    assert_eq!(
        warnings, 2,
        "expected 2 warnings for bug missing verify and principles"
    );
}

#[test]
fn e2e_lint_heading_free_bug_with_brief_passes() {
    let _log = common::test_log("e2e_lint_task_missing_acceptance_criteria");
    // Headings are dead: a bug whose description carries no headings but
    // whose typed fields satisfy the brief lints clean
    let workspace = BrWorkspace::new();
    init_workspace(&workspace);

    let create = run_br(
        &workspace,
        [
            "create",
            "Heading-free task",
            "--priority",
            "3",
            "--verify",
            "cargo test --offline lint",
            "--principle",
            "prove-it-works — typed fields, not heading grep",
        ],
        "create_heading_free_task",
    );
    assert!(create.status.success(), "create failed: {}", create.stderr);

    let lint = run_br(&workspace, ["lint", "--json"], "lint_task_no_headings");
    assert!(lint.status.success(), "lint failed: {}", lint.stderr);

    let json_str = extract_json_payload(&lint.stdout);
    let json: Value = serde_json::from_str(&json_str).expect("valid JSON");

    assert_eq!(json["total"], 0, "heading-free brief must not warn");
}

// =============================================================================
// Filter Tests
// =============================================================================

#[test]
fn e2e_lint_filter_by_type_bug() {
    let _log = common::test_log("e2e_lint_filter_by_type_bug");
    // --type bug should only lint bug issues
    let workspace = BrWorkspace::new();
    init_workspace(&workspace);

    // Create bug without required sections
    let bug_id = create_issue_with_description(&workspace, "Buggy bug", "bug", Some("Bug desc"));
    // Create task without required sections
    create_issue_with_description(&workspace, "Tasky task", "task", Some("Task desc"));

    let lint = run_br(
        &workspace,
        ["lint", "--type", "bug", "--json"],
        "lint_filter_bug",
    );
    assert!(lint.status.success(), "lint failed: {}", lint.stderr);

    let json_str = extract_json_payload(&lint.stdout);
    let json: Value = serde_json::from_str(&json_str).expect("valid JSON");

    let results = json["results"].as_array().unwrap();
    // Should only have the bug in results
    assert!(
        results.iter().all(|r| r["type"] == "bug"),
        "expected only bugs in results when filtering by type=bug"
    );
    assert!(
        results.iter().any(|r| r["id"] == bug_id),
        "bug {bug_id} should be in results"
    );
}

#[test]
fn e2e_lint_filter_by_status_all() {
    let _log = common::test_log("e2e_lint_filter_by_status_all");
    // --status all should include non-default issue states.
    let workspace = BrWorkspace::new();
    init_workspace(&workspace);

    // Create and close a bug without required fields
    let bug_id = create_issue_with_description(&workspace, "Closed bug", "bug", Some("Closed"));
    let gate = run_br(
        &workspace,
        [
            "gate",
            "report",
            &bug_id,
            "--gate",
            "unit-test-verified",
            "--provider",
            "e2e-lint-test",
            "--status",
            "pass",
            "--to",
            "closed",
        ],
        "gate_report_bug",
    );
    assert!(gate.status.success(), "gate report failed: {}", gate.stderr);
    let close = run_br(
        &workspace,
        ["close", &bug_id, "--commit-sha", "abc1234"],
        "close_bug",
    );
    assert!(close.status.success(), "close failed: {}", close.stderr);
    let deferred_bug =
        create_issue_with_description(&workspace, "Deferred bug", "bug", Some("Deferred"));
    let defer = run_br(
        &workspace,
        [
            "update",
            &deferred_bug,
            "--status",
            "deferred",
            "--defer",
            "2100-01-01T00:00:00Z",
        ],
        "defer_bug_for_status_all",
    );
    assert!(defer.status.success(), "defer failed: {}", defer.stderr);

    // Default lint should not include closed or deferred.
    let lint_default = run_br(&workspace, ["lint", "--json"], "lint_status_default");
    let json_str = extract_json_payload(&lint_default.stdout);
    let json: Value = serde_json::from_str(&json_str).expect("valid JSON");
    let default_results = json["results"].as_array().unwrap();
    assert!(
        !default_results.iter().any(|r| r["id"] == bug_id),
        "closed issue should not appear in default lint"
    );
    assert!(
        !default_results.iter().any(|r| r["id"] == deferred_bug),
        "deferred issue should not appear in default lint"
    );

    // --status all should include closed and deferred.
    let lint_all = run_br(
        &workspace,
        ["lint", "--status", "all", "--json"],
        "lint_status_all",
    );
    assert!(
        lint_all.status.success(),
        "lint failed: {}",
        lint_all.stderr
    );

    let json_str = extract_json_payload(&lint_all.stdout);
    let json: Value = serde_json::from_str(&json_str).expect("valid JSON");
    let all_results = json["results"].as_array().unwrap();
    assert!(
        all_results.iter().any(|r| r["id"] == bug_id),
        "closed issue should appear with --status all"
    );
    assert!(
        all_results.iter().any(|r| r["id"] == deferred_bug),
        "deferred issue should appear with --status all"
    );
}

#[test]
fn e2e_lint_filter_by_status_deferred() {
    let _log = common::test_log("e2e_lint_filter_by_status_deferred");
    let workspace = BrWorkspace::new();
    init_workspace(&workspace);

    let open_bug = create_issue_with_description(&workspace, "Open bug", "bug", Some("Open"));
    let deferred_bug =
        create_issue_with_description(&workspace, "Deferred bug", "bug", Some("Deferred"));
    let defer = run_br(
        &workspace,
        [
            "update",
            &deferred_bug,
            "--status",
            "deferred",
            "--defer",
            "2100-01-01T00:00:00Z",
        ],
        "defer_bug",
    );
    assert!(defer.status.success(), "defer failed: {}", defer.stderr);

    let lint = run_br(
        &workspace,
        ["lint", "--status", "deferred", "--json"],
        "lint_status_deferred",
    );
    assert!(lint.status.success(), "lint failed: {}", lint.stderr);

    let json_str = extract_json_payload(&lint.stdout);
    let json: Value = serde_json::from_str(&json_str).expect("valid JSON");
    let results = json["results"].as_array().expect("results array");

    assert!(
        results.iter().any(|r| r["id"] == deferred_bug),
        "deferred issue should appear when filtering deferred"
    );
    assert!(
        !results.iter().any(|r| r["id"] == open_bug),
        "open issue should not appear when filtering deferred"
    );
}

#[test]
fn e2e_lint_specific_issue_id() {
    let _log = common::test_log("e2e_lint_specific_issue_id");
    // Lint specific issue by ID
    let workspace = BrWorkspace::new();
    init_workspace(&workspace);

    // Create two bugs without required sections
    let bug1_id = create_issue_with_description(&workspace, "Bug one", "bug", Some("First"));
    let _bug2_id = create_issue_with_description(&workspace, "Bug two", "bug", Some("Second"));

    // Lint only bug1
    let lint = run_br(&workspace, ["lint", &bug1_id, "--json"], "lint_specific_id");
    assert!(lint.status.success(), "lint failed: {}", lint.stderr);

    let json_str = extract_json_payload(&lint.stdout);
    let json: Value = serde_json::from_str(&json_str).expect("valid JSON");

    let results = json["results"].as_array().unwrap();
    assert_eq!(
        results.len(),
        1,
        "expected exactly 1 result for specific ID"
    );
    assert_eq!(
        results[0]["id"], bug1_id,
        "result should be the specified bug"
    );
}

// =============================================================================
// JSON Output Structure Tests
// =============================================================================

#[test]
fn e2e_lint_json_output_structure() {
    let _log = common::test_log("e2e_lint_json_output_structure");
    // Verify JSON output has correct structure
    let workspace = BrWorkspace::new();
    init_workspace(&workspace);

    create_issue_with_description(&workspace, "Test bug", "bug", Some("Minimal"));

    let lint = run_br(&workspace, ["lint", "--json"], "lint_json_structure");
    assert!(lint.status.success(), "lint failed: {}", lint.stderr);

    let json_str = extract_json_payload(&lint.stdout);
    let json: Value = serde_json::from_str(&json_str).expect("valid JSON");

    // Check top-level fields
    assert!(json.get("total").is_some(), "missing 'total' field");
    assert!(json.get("issues").is_some(), "missing 'issues' field");
    assert!(json.get("results").is_some(), "missing 'results' field");

    // Check results array structure
    let results = json["results"].as_array().unwrap();
    if !results.is_empty() {
        let result = &results[0];
        assert!(result.get("id").is_some(), "result missing 'id' field");
        assert!(
            result.get("title").is_some(),
            "result missing 'title' field"
        );
        assert!(result.get("type").is_some(), "result missing 'type' field");
        assert!(
            result.get("warnings").is_some(),
            "result missing 'warnings' field"
        );
        assert!(
            result.get("missing").is_some(),
            "result missing 'missing' field"
        );
        let suggestions = result["suggestions"]
            .as_array()
            .expect("result missing 'suggestions' array");
        assert!(
            suggestions.iter().any(|suggestion| {
                suggestion["section"].as_str() == Some("verify")
                    && suggestion["hint"]
                        .as_str()
                        .is_some_and(|hint| hint.contains("VERIFY"))
            }),
            "result suggestions should include the verify hint: {suggestions:?}"
        );
    }
}

#[test]
fn e2e_lint_json_exit_code_always_zero() {
    let _log = common::test_log("e2e_lint_json_exit_code_always_zero");
    // In JSON mode, exit code should always be 0 (even with warnings)
    let workspace = BrWorkspace::new();
    init_workspace(&workspace);

    // Create bug without required sections (will have warnings)
    create_issue_with_description(&workspace, "Buggy", "bug", Some("No sections"));

    let lint = run_br(&workspace, ["lint", "--json"], "lint_json_exit_code");
    assert!(
        lint.status.success(),
        "JSON mode should always exit 0, got: {}",
        lint.status
    );
}

// =============================================================================
// Text Output Tests
// =============================================================================

#[test]
fn e2e_lint_text_output_warnings() {
    let _log = common::test_log("e2e_lint_text_output_warnings");
    // Text mode with warnings should show formatted output
    let workspace = BrWorkspace::new();
    init_workspace(&workspace);

    let id = create_issue_with_description(&workspace, "Warning bug", "bug", Some("No sections"));

    let lint = run_br(&workspace, ["lint"], "lint_text_warnings");
    // Text mode exits non-zero when there are warnings
    // But we should still check the output format

    assert!(
        lint.stdout.contains(&id) || lint.stdout.contains("bug"),
        "text output should mention the issue"
    );
    assert!(
        lint.stdout.contains("Missing") || lint.stdout.contains("warning"),
        "text output should indicate missing fields"
    );
    assert!(
        lint.stdout.contains("VERIFY"),
        "text output should include the verify hint"
    );
}

#[test]
fn e2e_lint_text_exit_code_nonzero_with_warnings() {
    let _log = common::test_log("e2e_lint_text_exit_code_nonzero_with_warnings");
    // In text mode, exit code should be 1 when there are warnings
    let workspace = BrWorkspace::new();
    init_workspace(&workspace);

    create_issue_with_description(&workspace, "Warning bug", "bug", Some("No sections"));

    let lint = run_br(&workspace, ["lint"], "lint_text_exit_nonzero");
    assert!(
        !lint.status.success(),
        "text mode with warnings should exit non-zero"
    );
}

// =============================================================================
// Error Handling Tests
// =============================================================================

#[test]
fn e2e_lint_before_init_fails() {
    let _log = common::test_log("e2e_lint_before_init_fails");
    // Lint without init should fail
    let workspace = BrWorkspace::new();
    // Do NOT init

    let lint = run_br(&workspace, ["lint"], "lint_before_init");
    assert!(!lint.status.success(), "lint before init should fail");
    assert!(
        lint.stderr.contains("not found")
            || lint.stderr.contains("initialize")
            || lint.stderr.contains("No .beads"),
        "error should mention workspace not initialized, got: {}",
        lint.stderr
    );
}

#[test]
fn e2e_lint_nonexistent_id_error() {
    let _log = common::test_log("e2e_lint_nonexistent_id_error");
    // Lint with nonexistent ID should handle gracefully
    let workspace = BrWorkspace::new();
    init_workspace(&workspace);

    let lint = run_br(
        &workspace,
        ["lint", "bd-nonexistent"],
        "lint_nonexistent_id",
    );
    // Should either fail or print an error message
    assert!(
        !lint.status.success()
            || lint.stderr.contains("not found")
            || lint.stdout.contains("not found"),
        "nonexistent ID should be handled"
    );
}

#[test]
fn e2e_lint_unknown_type_filter_no_matches() {
    let _log = common::test_log("e2e_lint_unknown_type_filter_no_matches");
    // Unknown --type value is rejected (bd conformance: only task, bug, feature, epic, chore)
    let workspace = BrWorkspace::new();
    init_workspace(&workspace);

    // Create a bug issue
    create_issue_with_description(&workspace, "Sample bug", "bug", None);

    let lint = run_br(
        &workspace,
        ["lint", "--type", "unknown_custom_type"],
        "lint_unknown_type",
    );
    // For bd conformance, CLI rejects unknown types (they may exist in imported data
    // but cannot be specified via CLI). See src/model/mod.rs FromStr for IssueType.
    assert!(
        !lint.status.success(),
        "unknown type should fail for bd conformance, got stdout: {}",
        lint.stdout
    );
    assert!(
        lint.stderr.contains("INVALID_TYPE") || lint.stderr.contains("Invalid issue type"),
        "should report invalid type error, got stderr: {}",
        lint.stderr
    );
}

// =============================================================================
// Heading Independence Tests
// =============================================================================

#[test]
fn e2e_lint_ignores_markdown_headings_entirely() {
    let _log = common::test_log("e2e_lint_case_insensitive_section_matching");
    // Headings neither satisfy nor trigger warnings: only typed fields count.
    // A P3 bug with headings but no verify still warns on verify.
    let workspace = BrWorkspace::new();
    init_workspace(&workspace);

    let description = "## Steps to Reproduce\n1. Steps\n\n## Acceptance Criteria\n- Done";
    let id = create_issue_with_description(
        &workspace,
        "Headings but no verify",
        "bug",
        Some(description),
    );

    let lint = run_br(&workspace, ["lint", "--json"], "lint_case_insensitive");
    assert!(lint.status.success(), "lint failed: {}", lint.stderr);

    let json_str = extract_json_payload(&lint.stdout);
    let json: Value = serde_json::from_str(&json_str).expect("valid JSON");

    let results = json["results"].as_array().unwrap();
    let issue_result = results.iter().find(|r| r["id"] == id);
    assert!(issue_result.is_some(), "issue {id} not in results");

    let missing = issue_result.unwrap()["missing"].as_array().unwrap();
    assert!(
        missing.iter().any(|m| m.as_str().unwrap() == "verify"),
        "headings must not substitute for verify, got: {missing:?}"
    );
    assert!(
        missing.iter().all(|m| !m.as_str().unwrap().contains('#')),
        "no markdown headings in lint output: {missing:?}"
    );
}

// =============================================================================
// Multiple Issues Tests
// =============================================================================

#[test]
fn e2e_lint_multiple_issues_with_warnings() {
    let _log = common::test_log("e2e_lint_multiple_issues_with_warnings");
    // Multiple issues with warnings should all be reported
    let workspace = BrWorkspace::new();
    init_workspace(&workspace);

    let bug1 = create_issue_with_description(&workspace, "Bug 1", "bug", Some("Missing"));
    let bug2 = create_issue_with_description(&workspace, "Bug 2", "bug", Some("Also missing"));
    let task = create_issue_with_description(&workspace, "Task 1", "task", Some("Missing too"));

    let lint = run_br(&workspace, ["lint", "--json"], "lint_multiple");
    assert!(lint.status.success(), "lint failed: {}", lint.stderr);

    let json_str = extract_json_payload(&lint.stdout);
    let json: Value = serde_json::from_str(&json_str).expect("valid JSON");

    let issues_count = json["issues"].as_i64().unwrap();
    assert!(
        issues_count >= 3,
        "expected at least 3 issues with warnings, got {issues_count}"
    );

    let results = json["results"].as_array().unwrap();
    assert!(
        results.iter().any(|r| r["id"] == bug1),
        "bug1 should be in results"
    );
    assert!(
        results.iter().any(|r| r["id"] == bug2),
        "bug2 should be in results"
    );
    assert!(
        results.iter().any(|r| r["id"] == task),
        "task should be in results"
    );
}
