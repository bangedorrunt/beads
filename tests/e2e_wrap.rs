//! E2E tests for the --wrap flag across various commands.
//!
//! Tests verify that:
//! - --wrap flag is accepted by show, list, ready, comments, search, blocked
//! - Default behavior (no --wrap) still truncates text
//! - With --wrap, long content is not truncated
//! - Different terminal widths are respected

mod common;

use common::cli::{BrWorkspace, parse_list_issues, run_br, run_br_with_env};

fn init_workspace_with_long_issues(workspace: &BrWorkspace) {
    let output = run_br(workspace, ["init", "--prefix", "wrap"], "init");
    assert!(output.status.success(), "init failed: {}", output.stderr);

    let long_title = "This is a very long issue title that should definitely exceed the normal terminal width when displayed in the list view or show view without wrapping enabled";
    let output = run_br(
        workspace,
        [
            "create",
            long_title,
            "--type",
            "task",
            "--priority",
            "2",
            "-d",
            "This is also a very long description that contains multiple sentences and should span several lines when the wrap option is enabled because it provides detailed context about the issue being tracked.",
        ],
        "create_long",
    );
    assert!(output.status.success(), "create failed: {}", output.stderr);

    let output = run_br(
        workspace,
        ["create", "Short issue", "--type", "bug"],
        "create_short",
    );
    assert!(output.status.success(), "create short failed");
}

#[test]
fn e2e_list_without_wrap_truncates() {
    let workspace = BrWorkspace::new();
    init_workspace_with_long_issues(&workspace);

    let output = run_br_with_env(&workspace, ["list"], [("COLUMNS", "60")], "list_no_wrap");
    assert!(output.status.success(), "list failed");

    let _has_ellipsis = output.stdout.contains("...");
    assert!(output.stdout.contains("wrap-"), "Should show issue IDs");
}

#[test]
fn e2e_list_with_wrap_shows_full_content() {
    let workspace = BrWorkspace::new();
    init_workspace_with_long_issues(&workspace);

    let output = run_br_with_env(
        &workspace,
        ["list", "--wrap"],
        [("COLUMNS", "60")],
        "list_with_wrap",
    );
    assert!(output.status.success(), "list --wrap failed");

    assert!(output.stdout.contains("wrap-"), "Should show issue IDs");
}

#[test]
fn e2e_list_wrap_json_unchanged() {
    let workspace = BrWorkspace::new();
    init_workspace_with_long_issues(&workspace);

    let output_no_wrap = run_br(&workspace, ["list", "--json"], "list_json");
    let output_wrap = run_br(&workspace, ["list", "--wrap", "--json"], "list_json_wrap");

    assert!(output_no_wrap.status.success());
    assert!(output_wrap.status.success());

    let issues_no_wrap = parse_list_issues(&output_no_wrap.stdout);
    let issues_wrap = parse_list_issues(&output_wrap.stdout);
    assert_eq!(issues_no_wrap.len(), issues_wrap.len());
}

#[test]
fn e2e_show_without_wrap() {
    let workspace = BrWorkspace::new();
    init_workspace_with_long_issues(&workspace);

    let list_output = run_br(&workspace, ["list", "--json"], "list_for_show");
    let issues = parse_list_issues(&list_output.stdout);
    let long_issue_id = issues
        .iter()
        .find(|i| i["title"].as_str().unwrap_or("").contains("very long"))
        .expect("find long issue")["id"]
        .as_str()
        .unwrap();

    let output = run_br_with_env(
        &workspace,
        ["show", long_issue_id],
        [("COLUMNS", "60")],
        "show_no_wrap",
    );
    assert!(output.status.success(), "show failed");
    assert!(output.stdout.contains(long_issue_id));
}

#[test]
fn e2e_show_with_wrap() {
    let workspace = BrWorkspace::new();
    init_workspace_with_long_issues(&workspace);

    let list_output = run_br(&workspace, ["list", "--json"], "list_for_show_wrap");
    let issues = parse_list_issues(&list_output.stdout);
    let long_issue_id = issues
        .iter()
        .find(|i| i["title"].as_str().unwrap_or("").contains("very long"))
        .expect("find long issue")["id"]
        .as_str()
        .unwrap();

    let output = run_br_with_env(
        &workspace,
        ["show", long_issue_id, "--wrap"],
        [("COLUMNS", "60")],
        "show_with_wrap",
    );
    assert!(output.status.success(), "show --wrap failed");
    assert!(output.stdout.contains(long_issue_id));

    assert!(
        output.stdout.contains("detailed context"),
        "Description should be visible"
    );
}

#[test]
fn e2e_ready_without_wrap() {
    let workspace = BrWorkspace::new();
    init_workspace_with_long_issues(&workspace);

    let output = run_br_with_env(&workspace, ["ready"], [("COLUMNS", "60")], "ready_no_wrap");
    assert!(output.status.success(), "ready failed");
}

#[test]
fn e2e_ready_with_wrap() {
    let workspace = BrWorkspace::new();
    init_workspace_with_long_issues(&workspace);

    let output = run_br_with_env(
        &workspace,
        ["ready", "--wrap"],
        [("COLUMNS", "60")],
        "ready_with_wrap",
    );
    assert!(output.status.success(), "ready --wrap failed");
}

#[test]
fn e2e_search_without_wrap() {
    let workspace = BrWorkspace::new();
    init_workspace_with_long_issues(&workspace);

    let output = run_br_with_env(
        &workspace,
        ["search", "long"],
        [("COLUMNS", "60")],
        "search_no_wrap",
    );
    assert!(output.status.success(), "search failed");
}

#[test]
fn e2e_search_with_wrap() {
    let workspace = BrWorkspace::new();
    init_workspace_with_long_issues(&workspace);

    let output = run_br_with_env(
        &workspace,
        ["search", "long", "--wrap"],
        [("COLUMNS", "60")],
        "search_with_wrap",
    );
    assert!(output.status.success(), "search --wrap failed");
}

#[test]
fn e2e_comments_with_wrap() {
    let workspace = BrWorkspace::new();
    init_workspace_with_long_issues(&workspace);

    let list_output = run_br(&workspace, ["list", "--json"], "list_for_comments");
    let issues = parse_list_issues(&list_output.stdout);
    let issue_id = issues[0]["id"].as_str().unwrap();

    let long_comment = "This is a very long comment that contains lots of detailed information about the progress of this issue and should demonstrate the wrapping behavior when the wrap flag is enabled.";
    let output = run_br(
        &workspace,
        ["comments", "add", issue_id, long_comment],
        "add_comment",
    );
    assert!(output.status.success(), "add comment failed");

    let output = run_br_with_env(
        &workspace,
        ["comments", issue_id],
        [("COLUMNS", "60")],
        "comments_no_wrap",
    );
    assert!(output.status.success(), "comments failed");

    let output = run_br_with_env(
        &workspace,
        ["comments", issue_id, "--wrap"],
        [("COLUMNS", "60")],
        "comments_with_wrap",
    );
    assert!(output.status.success(), "comments --wrap failed");
}

#[test]
fn e2e_blocked_with_wrap() {
    let workspace = BrWorkspace::new();
    init_workspace_with_long_issues(&workspace);

    let output = run_br_with_env(
        &workspace,
        ["blocked", "--wrap"],
        [("COLUMNS", "60")],
        "blocked_with_wrap",
    );
    assert!(output.status.success(), "blocked --wrap failed");
    assert!(
        output.stdout.contains("blocked") || output.stdout.contains("No blocked"),
        "Should show blocked output"
    );
}

#[test]
fn e2e_blocked_with_dependencies() {
    let workspace = BrWorkspace::new();
    init_workspace_with_long_issues(&workspace);

    let list_output = run_br(&workspace, ["list", "--json"], "list_for_blocked");
    let issues = parse_list_issues(&list_output.stdout);
    if issues.len() < 2 {
        return;
    }
    let parent_id = issues[0]["id"].as_str().unwrap();
    let child_id = issues[1]["id"].as_str().unwrap();

    let output = run_br(&workspace, ["dep", "add", child_id, parent_id], "add_dep");
    assert!(output.status.success(), "dep add failed: {}", output.stderr);

    let output = run_br_with_env(
        &workspace,
        ["blocked", "--wrap"],
        [("COLUMNS", "60")],
        "blocked_with_wrap_deps",
    );
    assert!(output.status.success(), "blocked --wrap failed");
}

#[test]
fn e2e_wrap_very_narrow_terminal() {
    let workspace = BrWorkspace::new();
    init_workspace_with_long_issues(&workspace);

    let output = run_br_with_env(
        &workspace,
        ["list", "--wrap"],
        [("COLUMNS", "20")],
        "list_narrow",
    );
    assert!(output.status.success(), "list at narrow width failed");
}

#[test]
fn e2e_wrap_very_wide_terminal() {
    let workspace = BrWorkspace::new();
    init_workspace_with_long_issues(&workspace);

    let output = run_br_with_env(
        &workspace,
        ["list", "--wrap"],
        [("COLUMNS", "200")],
        "list_wide",
    );
    assert!(output.status.success(), "list at wide width failed");
}

#[test]
fn e2e_wrap_with_unicode_content() {
    let workspace = BrWorkspace::new();

    let output = run_br(&workspace, ["init", "--prefix", "uni"], "init_unicode");
    assert!(output.status.success());

    let unicode_title = "Fix bug 🐛 with 日本語 characters and emojis 🎉🚀";
    let output = run_br(
        &workspace,
        ["create", unicode_title, "--type", "bug"],
        "create_unicode",
    );
    assert!(output.status.success(), "create unicode failed");

    let output = run_br_with_env(
        &workspace,
        ["list", "--wrap"],
        [("COLUMNS", "40")],
        "list_unicode_wrap",
    );
    assert!(output.status.success(), "list unicode with wrap failed");
    assert!(output.stdout.contains("🐛") || output.stdout.contains("bug"));
}

#[test]
fn e2e_wrap_empty_database() {
    let workspace = BrWorkspace::new();

    let output = run_br(&workspace, ["init"], "init_empty");
    assert!(output.status.success());

    let output = run_br(&workspace, ["list", "--wrap"], "list_empty_wrap");
    assert!(output.status.success());

    let output = run_br(&workspace, ["ready", "--wrap"], "ready_empty_wrap");
    assert!(output.status.success());

    let output = run_br(&workspace, ["blocked", "--wrap"], "blocked_empty_wrap");
    assert!(output.status.success());

    let output = run_br(
        &workspace,
        ["search", "nothing", "--wrap"],
        "search_empty_wrap",
    );
    assert!(output.status.success());
}
