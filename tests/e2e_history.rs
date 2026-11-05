//! E2E tests for the `history` command.
//!
//! Tests cover:
//! - history list: List available backup snapshots
//! - history diff: Show diff between current and a backup
//! - history restore: Restore a backup to issues.jsonl
//! - history prune: Prune old backups
//! - Error handling: Before init, missing files, restore without force
//! - Edge cases: Many backups, backup deduplication

mod common;

use common::cli::{BrWorkspace, run_br};
use std::fs;
use std::thread;
use std::time::Duration;

/// Helper to run sync --flush-only.
fn sync_flush(workspace: &BrWorkspace) {
    let sync = run_br(workspace, ["sync", "--flush-only"], "sync_flush");
    assert!(
        sync.status.success(),
        "sync should succeed: {}",
        sync.stderr
    );
}

/// Helper to create an issue without auto-flush.
/// This ensures the dirty flag is preserved for explicit sync calls.
fn create_issue(workspace: &BrWorkspace, title: &str, label: &str) {
    let create = run_br(workspace, ["--no-auto-flush", "create", title], label);
    assert!(create.status.success(), "create failed: {}", create.stderr);
}

/// Helper to set up a workspace with issues.jsonl already existing.
/// This creates an initial issue and syncs to establish the base JSONL file.
/// Returns the workspace ready for backup tests.
///
/// Note: We use --no-auto-flush to prevent automatic export after create,
/// which would clear dirty flags and prevent the explicit sync from triggering backups.
fn setup_workspace_with_jsonl() -> BrWorkspace {
    let workspace = BrWorkspace::new();

    let init = run_br(&workspace, ["init"], "init");
    assert!(init.status.success(), "init failed: {}", init.stderr);

    create_issue(&workspace, "Initial issue", "create_initial");

    sync_flush(&workspace);

    workspace
}

/// Read backup files from the history directory.
fn list_backup_files(workspace: &BrWorkspace) -> Vec<String> {
    let history_dir = workspace.root.join(".beads").join(".br_history");
    if !history_dir.exists() {
        return vec![];
    }
    let mut files: Vec<String> = fs::read_dir(&history_dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| {
            let path = e.path();
            let has_prefix = path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("issues."));
            let has_jsonl = path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("jsonl"));
            has_prefix && has_jsonl
        })
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    files.sort();
    files
}

#[test]
fn e2e_history_list_empty_initially() {
    let _log = common::test_log("e2e_history_list_empty_initially");
    let workspace = BrWorkspace::new();

    let init = run_br(&workspace, ["init"], "init");
    assert!(init.status.success(), "init failed: {}", init.stderr);

    let list = run_br(&workspace, ["history", "list"], "history_list_empty");
    assert!(
        list.status.success(),
        "history list failed: {}",
        list.stderr
    );
    assert!(
        list.stdout.contains("No backups found")
            || list.stdout.contains('0')
            || !list.stdout.contains("issues."),
        "should show no backups: {}",
        list.stdout
    );
}

#[test]
fn e2e_history_list_after_sync_creates_backup() {
    let _log = common::test_log("e2e_history_list_after_sync_creates_backup");
    let workspace = setup_workspace_with_jsonl();

    create_issue(&workspace, "Second issue", "create_second");

    sync_flush(&workspace);

    let list = run_br(&workspace, ["history", "list"], "history_list_with_backup");
    assert!(
        list.status.success(),
        "history list failed: {}",
        list.stderr
    );

    let backups = list_backup_files(&workspace);
    assert!(
        !backups.is_empty(),
        "should have at least one backup after sync"
    );
}

#[test]
fn e2e_history_list_shows_backup_details() {
    let _log = common::test_log("e2e_history_list_shows_backup_details");
    let workspace = setup_workspace_with_jsonl();

    create_issue(&workspace, "Issue for backup details", "create_details");
    sync_flush(&workspace);

    let list = run_br(&workspace, ["history", "list"], "history_list_details");
    assert!(list.status.success());

    let stdout = list.stdout.to_uppercase();
    assert!(
        stdout.contains("FILENAME") || stdout.contains("ISSUES."),
        "should show filename: {}",
        list.stdout
    );
}

#[test]
fn e2e_history_multiple_backups_chronological_order() {
    let _log = common::test_log("e2e_history_multiple_backups_chronological_order");
    let workspace = setup_workspace_with_jsonl();

    for i in 0..3 {
        thread::sleep(Duration::from_millis(1100));

        create_issue(&workspace, &format!("Issue {i}"), &format!("create_{i}"));
        sync_flush(&workspace);
    }

    let list = run_br(&workspace, ["history", "list"], "history_list_multiple");
    assert!(list.status.success());

    let backups = list_backup_files(&workspace);
    assert!(
        backups.len() >= 2,
        "should have multiple backups: {backups:?}"
    );

    let sorted_backups = {
        let mut b = backups.clone();
        b.sort();
        b
    };
    assert_eq!(backups, sorted_backups, "backups should be sorted");
}

#[test]
fn e2e_history_restore_backup() {
    let _log = common::test_log("e2e_history_restore_backup");
    let workspace = setup_workspace_with_jsonl();

    create_issue(&workspace, "Issue before restore", "create_before_restore");
    sync_flush(&workspace);

    let backups = list_backup_files(&workspace);
    assert!(!backups.is_empty(), "should have backup");
    let backup_file = &backups[0];

    create_issue(&workspace, "Issue after backup", "create_after_backup");

    let restore = run_br(
        &workspace,
        ["history", "restore", backup_file, "--force"],
        "history_restore",
    );
    assert!(
        restore.status.success(),
        "history restore failed: {}",
        restore.stderr
    );
    assert!(
        restore.stdout.contains("Restored") || restore.stdout.contains("restored"),
        "should confirm restoration: {}",
        restore.stdout
    );
}

#[test]
fn e2e_history_diff_shows_differences() {
    let _log = common::test_log("e2e_history_diff_shows_differences");
    let workspace = setup_workspace_with_jsonl();

    create_issue(&workspace, "Issue for diff", "create_diff");
    sync_flush(&workspace);

    let backups = list_backup_files(&workspace);
    assert!(!backups.is_empty(), "should have backup");
    let backup_file = &backups[0];

    create_issue(&workspace, "New issue for diff", "create_new_diff");
    sync_flush(&workspace);

    let diff = run_br(&workspace, ["history", "diff", backup_file], "history_diff");
    assert!(diff.status.success(), "diff failed: {}", diff.stderr);
    assert!(
        diff.stdout.contains("--- ") && diff.stdout.contains("+++ ") && diff.stdout.contains("@@"),
        "diff should render unified diff output: {}",
        diff.stdout
    );
}

#[test]
fn e2e_history_prune_keeps_recent() {
    let _log = common::test_log("e2e_history_prune_keeps_recent");
    let workspace = setup_workspace_with_jsonl();

    for i in 0..4 {
        thread::sleep(Duration::from_millis(1100));
        create_issue(
            &workspace,
            &format!("Issue for prune {i}"),
            &format!("create_prune_{i}"),
        );
        sync_flush(&workspace);
    }

    let backups_before = list_backup_files(&workspace);
    assert!(
        backups_before.len() >= 3,
        "should have multiple backups: {backups_before:?}"
    );

    let prune = run_br(
        &workspace,
        ["history", "prune", "--keep", "2"],
        "history_prune",
    );
    assert!(prune.status.success(), "prune failed: {}", prune.stderr);

    assert!(
        prune.stdout.contains("Pruned") || prune.stdout.contains('0'),
        "should report prune result: {}",
        prune.stdout
    );
}

#[test]
fn e2e_history_list_before_init_fails() {
    let _log = common::test_log("e2e_history_list_before_init_fails");
    let workspace = BrWorkspace::new();

    let list = run_br(&workspace, ["history", "list"], "history_no_init");
    assert!(
        !list.status.success(),
        "history list should fail before init"
    );
    assert!(
        list.stderr.contains("not initialized")
            || list.stderr.contains("NotInitialized")
            || list.stderr.contains("not found")
            || list.stderr.contains(".beads"),
        "error should mention initialization: {}",
        list.stderr
    );
}

#[test]
fn e2e_history_restore_nonexistent_backup_fails() {
    let _log = common::test_log("e2e_history_restore_nonexistent_backup_fails");
    let workspace = BrWorkspace::new();

    let init = run_br(&workspace, ["init"], "init");
    assert!(init.status.success(), "init failed: {}", init.stderr);

    let restore = run_br(
        &workspace,
        ["history", "restore", "nonexistent.20990101_120000.jsonl"],
        "history_restore_missing",
    );
    assert!(
        !restore.status.success(),
        "restore should fail for non-existent backup"
    );
    assert!(
        restore.stderr.contains("not found")
            || restore.stderr.contains("No such file")
            || restore.stderr.contains("Backup file not found"),
        "error should mention file not found: {}",
        restore.stderr
    );
}

#[test]
fn e2e_history_restore_without_force_fails_when_exists() {
    let _log = common::test_log("e2e_history_restore_without_force_fails_when_exists");
    let workspace = setup_workspace_with_jsonl();

    create_issue(&workspace, "Issue for force test", "create_force");
    sync_flush(&workspace);

    let backups = list_backup_files(&workspace);
    assert!(!backups.is_empty(), "should have backup");
    let backup_file = &backups[0];

    let restore = run_br(
        &workspace,
        ["history", "restore", backup_file],
        "history_restore_no_force",
    );
    assert!(
        !restore.status.success(),
        "restore should fail without --force when issues.jsonl exists"
    );
    assert!(
        restore.stderr.contains("force")
            || restore.stderr.contains("exists")
            || restore.stderr.contains("overwrite"),
        "error should mention --force: {}",
        restore.stderr
    );
}

#[test]
fn e2e_history_diff_nonexistent_backup_fails() {
    let _log = common::test_log("e2e_history_diff_nonexistent_backup_fails");
    let workspace = setup_workspace_with_jsonl();

    let diff = run_br(
        &workspace,
        ["history", "diff", "nonexistent.20990101_120000.jsonl"],
        "history_diff_missing",
    );
    assert!(
        !diff.status.success(),
        "diff should fail for non-existent backup"
    );
    assert!(
        diff.stderr.contains("not found") || diff.stderr.contains("Backup file not found"),
        "error should mention file not found: {}",
        diff.stderr
    );
}

#[test]
fn e2e_history_backup_deduplication() {
    let _log = common::test_log("e2e_history_backup_deduplication");
    let workspace = setup_workspace_with_jsonl();

    create_issue(&workspace, "Dedup test issue", "create_dedup");
    sync_flush(&workspace);

    let backups_after_first = list_backup_files(&workspace);
    let count_after_first = backups_after_first.len();

    sync_flush(&workspace);

    let backups_after_second = list_backup_files(&workspace);
    assert_eq!(
        backups_after_second.len(),
        count_after_first,
        "should not create duplicate backup for identical content: before={backups_after_first:?}, after={backups_after_second:?}"
    );
}

#[test]
fn e2e_history_with_many_issues() {
    let _log = common::test_log("e2e_history_with_many_issues");
    let workspace = setup_workspace_with_jsonl();

    for i in 0..50 {
        let create = run_br(
            &workspace,
            ["--no-auto-flush", "create", &format!("Issue number {i}")],
            &format!("create_{i}"),
        );
        assert!(create.status.success(), "create failed: {}", create.stderr);
    }

    sync_flush(&workspace);

    let list = run_br(&workspace, ["history", "list"], "history_list_many");
    assert!(list.status.success(), "list failed: {}", list.stderr);

    let jsonl_path = workspace.root.join(".beads").join("issues.jsonl");
    if jsonl_path.exists() {
        let jsonl_size = fs::metadata(&jsonl_path).unwrap().len();
        assert!(jsonl_size > 0, "jsonl should have content after 50 issues");
    }
}

#[test]
fn e2e_history_default_command_is_list() {
    let _log = common::test_log("e2e_history_default_command_is_list");
    let workspace = BrWorkspace::new();

    let init = run_br(&workspace, ["init"], "init");
    assert!(init.status.success(), "init failed: {}", init.stderr);

    let history = run_br(&workspace, ["history"], "history_default");
    assert!(
        history.status.success(),
        "history default failed: {}",
        history.stderr
    );
    assert!(
        history.stdout.contains("No backups")
            || history.stdout.contains("Backups")
            || history.stdout.contains("FILENAME"),
        "should show list output: {}",
        history.stdout
    );
}

#[test]
fn e2e_history_prune_with_older_than() {
    let _log = common::test_log("e2e_history_prune_with_older_than");
    let workspace = setup_workspace_with_jsonl();

    create_issue(&workspace, "Issue for prune age", "create_prune_age");
    sync_flush(&workspace);

    let prune = run_br(
        &workspace,
        ["history", "prune", "--keep", "1", "--older-than", "1"],
        "history_prune_age",
    );
    assert!(
        prune.status.success(),
        "prune with older-than failed: {}",
        prune.stderr
    );

    assert!(
        prune.stdout.contains("Pruned"),
        "should report prune result: {}",
        prune.stdout
    );
}

#[test]
fn e2e_history_restore_without_force_succeeds_when_no_current() {
    let _log = common::test_log("e2e_history_restore_without_force_succeeds_when_no_current");
    let workspace = setup_workspace_with_jsonl();

    create_issue(&workspace, "Issue for restore test", "create_restore_test");
    sync_flush(&workspace);

    let backups = list_backup_files(&workspace);
    assert!(!backups.is_empty(), "should have backup");
    let backup_file = &backups[0];

    let jsonl_path = workspace.root.join(".beads").join("issues.jsonl");
    fs::remove_file(&jsonl_path).expect("remove issues.jsonl");

    let restore = run_br(
        &workspace,
        ["history", "restore", backup_file],
        "history_restore_no_current",
    );
    assert!(
        restore.status.success(),
        "restore should succeed without --force when no issues.jsonl exists: {}",
        restore.stderr
    );

    assert!(jsonl_path.exists(), "issues.jsonl should be restored");
}

#[test]
fn e2e_history_restore_verifies_content() {
    let _log = common::test_log("e2e_history_restore_verifies_content");
    let workspace = setup_workspace_with_jsonl();

    create_issue(&workspace, "Known issue for restore", "create_known");
    sync_flush(&workspace);

    thread::sleep(Duration::from_millis(1100));

    create_issue(&workspace, "Different issue", "create_different");
    sync_flush(&workspace);

    let backups = list_backup_files(&workspace);
    assert!(
        backups.len() >= 2,
        "should have at least 2 backups: {backups:?}"
    );
    let backup_file = &backups[backups.len() - 1];

    let backup_path = workspace
        .root
        .join(".beads")
        .join(".br_history")
        .join(backup_file);
    let backup_content = fs::read_to_string(&backup_path).expect("read backup");

    let restore = run_br(
        &workspace,
        ["history", "restore", backup_file, "--force"],
        "history_restore_verify",
    );
    assert!(
        restore.status.success(),
        "restore failed: {}",
        restore.stderr
    );

    let jsonl_path = workspace.root.join(".beads").join("issues.jsonl");
    let restored_content = fs::read_to_string(&jsonl_path).expect("read restored");
    assert_eq!(
        backup_content, restored_content,
        "restored content should match backup exactly"
    );
}

#[test]
fn e2e_history_diff_fails_when_no_current_jsonl() {
    let _log = common::test_log("e2e_history_diff_fails_when_no_current_jsonl");
    let workspace = setup_workspace_with_jsonl();

    create_issue(&workspace, "Issue for diff test", "create_diff_test");
    sync_flush(&workspace);

    let backups = list_backup_files(&workspace);
    assert!(!backups.is_empty(), "should have backup");
    let backup_file = &backups[0];

    let jsonl_path = workspace.root.join(".beads").join("issues.jsonl");
    fs::remove_file(&jsonl_path).expect("remove issues.jsonl");

    let diff = run_br(
        &workspace,
        ["history", "diff", backup_file],
        "history_diff_no_current",
    );
    assert!(
        !diff.status.success(),
        "diff should fail when no current issues.jsonl"
    );
    assert!(
        diff.stderr.contains("not found") || diff.stderr.contains("issues.jsonl"),
        "error should mention missing issues.jsonl: {}",
        diff.stderr
    );
}

#[test]
fn e2e_history_prune_removes_oldest_backups() {
    let _log = common::test_log("e2e_history_prune_removes_oldest_backups");
    let workspace = setup_workspace_with_jsonl();

    for i in 0..5 {
        thread::sleep(Duration::from_millis(1100));
        create_issue(
            &workspace,
            &format!("Issue for prune test {i}"),
            &format!("create_prune_{i}"),
        );
        sync_flush(&workspace);
    }

    let backups_before = list_backup_files(&workspace);
    assert!(
        backups_before.len() >= 4,
        "should have at least 4 backups: {backups_before:?}"
    );

    let expected_kept: Vec<_> = backups_before.iter().rev().take(2).cloned().collect();

    let prune = run_br(
        &workspace,
        ["history", "prune", "--keep", "2", "--older-than", "0"],
        "history_prune_oldest",
    );
    assert!(prune.status.success(), "prune failed: {}", prune.stderr);

    let backups_after = list_backup_files(&workspace);

    assert!(
        backups_after.len() <= 2,
        "should have at most 2 backups after prune: {backups_after:?}"
    );

    for backup in &backups_after {
        assert!(
            expected_kept.contains(backup),
            "kept backup {backup} should be one of the newest: {expected_kept:?}"
        );
    }
}

#[test]
fn e2e_history_list_json_output() {
    let _log = common::test_log("e2e_history_list_json_output");
    let workspace = setup_workspace_with_jsonl();

    create_issue(&workspace, "Issue for JSON test", "create_json_test");
    sync_flush(&workspace);

    let list = run_br(
        &workspace,
        ["--json", "history", "list"],
        "history_list_json",
    );
    assert!(list.status.success(), "list failed: {}", list.stderr);

    let json: serde_json::Value = serde_json::from_str(&list.stdout).expect("should be valid JSON");

    assert!(
        json.get("directory").is_some(),
        "should have directory field"
    );
    assert!(json.get("count").is_some(), "should have count field");
    assert!(json.get("backups").is_some(), "should have backups array");

    let backups = json["backups"].as_array().expect("backups should be array");
    if !backups.is_empty() {
        let backup = &backups[0];
        assert!(
            backup.get("filename").is_some(),
            "backup should have filename"
        );
        assert!(
            backup.get("size_bytes").is_some(),
            "backup should have size_bytes"
        );
        assert!(
            backup.get("timestamp").is_some(),
            "backup should have timestamp"
        );
    }
}

#[test]
fn e2e_history_restore_json_output() {
    let _log = common::test_log("e2e_history_restore_json_output");
    let workspace = setup_workspace_with_jsonl();

    create_issue(&workspace, "Issue for JSON restore", "create_json_restore");
    sync_flush(&workspace);

    let backups = list_backup_files(&workspace);
    assert!(!backups.is_empty(), "should have backup");
    let backup_file = &backups[0];

    let restore = run_br(
        &workspace,
        ["--json", "history", "restore", backup_file, "--force"],
        "history_restore_json",
    );
    assert!(
        restore.status.success(),
        "restore failed: {}",
        restore.stderr
    );

    let json: serde_json::Value =
        serde_json::from_str(&restore.stdout).expect("should be valid JSON");

    assert_eq!(json["action"], "restore", "action should be restore");
    assert_eq!(
        json["backup"].as_str(),
        Some(backup_file.as_str()),
        "backup field should match"
    );
    assert_eq!(json["restored"], true, "restored should be true");
    assert!(json.get("next_step").is_some(), "should have next_step");
}

#[test]
fn e2e_history_diff_json_reports_internal_diff_status() {
    let _log = common::test_log("e2e_history_diff_json_reports_internal_diff_status");
    let workspace = setup_workspace_with_jsonl();

    create_issue(&workspace, "Issue for JSON diff", "create_json_diff");
    sync_flush(&workspace);

    let backups = list_backup_files(&workspace);
    assert!(!backups.is_empty(), "should have backup");
    let backup_file = &backups[0];

    create_issue(&workspace, "New JSON diff issue", "create_json_diff_new");
    sync_flush(&workspace);

    let diff = run_br(
        &workspace,
        ["--json", "history", "diff", backup_file],
        "history_diff_json",
    );
    assert!(diff.status.success(), "diff failed: {}", diff.stderr);

    let json: serde_json::Value =
        serde_json::from_str(&diff.stdout).expect("history diff should emit JSON");
    assert_eq!(json["action"], "diff", "action should be diff");
    assert_eq!(
        json["backup"].as_str(),
        Some(backup_file.as_str()),
        "backup field should match"
    );
    assert_eq!(
        json["status"], "different",
        "status should report differences"
    );
    assert_eq!(
        json["diff_available"], true,
        "pure-Rust diff should always be available for readable JSONL files"
    );
    assert!(
        json["current_size_bytes"].is_null() && json["backup_size_bytes"].is_null(),
        "size fallback fields should be null when the internal diff engine runs: {json}"
    );
}

#[test]
fn e2e_history_prune_json_output() {
    let _log = common::test_log("e2e_history_prune_json_output");
    let workspace = setup_workspace_with_jsonl();

    create_issue(&workspace, "Issue for JSON prune", "create_json_prune");
    sync_flush(&workspace);

    let prune = run_br(
        &workspace,
        ["--json", "history", "prune", "--keep", "10"],
        "history_prune_json",
    );
    assert!(prune.status.success(), "prune failed: {}", prune.stderr);

    let json: serde_json::Value =
        serde_json::from_str(&prune.stdout).expect("should be valid JSON");

    assert_eq!(json["action"], "prune", "action should be prune");
    assert!(json.get("deleted").is_some(), "should have deleted count");
    assert_eq!(json["keep"], 10, "keep should be 10");
}
