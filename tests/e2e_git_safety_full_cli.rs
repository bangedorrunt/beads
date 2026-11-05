//! Comprehensive git safety regression tests for the full br CLI.
//!
//! This test suite implements beads-k1px:
//! - E2E assertions that NO br command invokes git operations or touches .git
//! - Run representative commands across the full CLI surface
//! - Validate .git tree is unchanged after each command batch
//! - Fail fast with artifact diff if any command touches .git
//!
//! Unlike `e2e_sync_git_safety.rs` (which focuses on sync), this tests ALL commands.

#![allow(
    clippy::items_after_statements,
    clippy::too_many_lines,
    clippy::cognitive_complexity
)]

mod common;

use common::cli::{BrWorkspace, run_br};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;

/// Compute SHA256 hash of a file.
fn hash_file(path: &Path) -> Option<String> {
    fs::read(path).ok().map(|contents| {
        let mut hasher = Sha256::new();
        hasher.update(&contents);
        beads::util::hex_encode(&hasher.finalize())
    })
}

/// Recursively collect file hashes from a directory.
fn collect_dir_hashes(dir: &Path, base: &Path, hashes: &mut BTreeMap<String, String>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let rel_path = path
                .strip_prefix(base)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();

            if path.is_file() {
                if let Some(hash) = hash_file(&path) {
                    hashes.insert(rel_path, hash);
                }
            } else if path.is_dir() {
                collect_dir_hashes(&path, base, hashes);
            }
        }
    }
}

/// Snapshot the .git directory.
fn snapshot_git_dir(root: &Path) -> BTreeMap<String, String> {
    let mut hashes = BTreeMap::new();
    let git_dir = root.join(".git");
    if git_dir.exists() {
        collect_dir_hashes(&git_dir, &git_dir, &mut hashes);
    }
    hashes
}

/// Filter out transient git files that can change during normal operations.
fn filter_transient_git_files(hashes: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    hashes
        .iter()
        .filter(|(k, _)| {
            let is_lock = Path::new(k)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("lock"));
            !is_lock
                && !k.contains("index")
                && !k.contains("FETCH_HEAD")
                && !k.contains("ORIG_HEAD")
                && !k.contains("logs/")
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

/// Get HEAD commit hash.
fn get_head_commit(root: &Path) -> Option<String> {
    Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// Get commit count.
fn get_commit_count(root: &Path) -> usize {
    Command::new("git")
        .args(["rev-list", "--count", "HEAD"])
        .current_dir(root)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map_or(0, |o| {
            String::from_utf8_lossy(&o.stdout)
                .trim()
                .parse()
                .unwrap_or(0)
        })
}

/// Initialize a git repo with an initial commit.
fn init_git_repo(workspace: &BrWorkspace) {
    let init = Command::new("git")
        .args(["init"])
        .current_dir(&workspace.root)
        .output()
        .expect("git init");
    assert!(init.status.success(), "git init failed");

    let _ = Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(&workspace.root)
        .output();
    let _ = Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(&workspace.root)
        .output();

    let src_dir = workspace.root.join("src");
    fs::create_dir_all(&src_dir).expect("create src dir");
    fs::write(
        src_dir.join("main.rs"),
        "fn main() { println!(\"Hello\"); }",
    )
    .expect("write main.rs");
    fs::write(
        workspace.root.join("Cargo.toml"),
        "[package]\nname = \"test\"\nversion = \"0.1.0\"\n",
    )
    .expect("write Cargo.toml");
    fs::write(workspace.root.join("README.md"), "# Test Project\n").expect("write README");

    let _ = Command::new("git")
        .args(["add", "."])
        .current_dir(&workspace.root)
        .output();
    let commit = Command::new("git")
        .args(["commit", "-m", "Initial commit"])
        .current_dir(&workspace.root)
        .output()
        .expect("git commit");
    assert!(commit.status.success(), "initial commit failed");
}

/// Git safety check result.
#[derive(Debug)]
struct GitSafetyCheck {
    #[allow(dead_code)]
    command: String,
    passed: bool,
    violations: Vec<String>,
    #[allow(dead_code)]
    head_changed: bool,
    #[allow(dead_code)]
    commit_count_changed: bool,
}

impl GitSafetyCheck {
    fn new(command: &str) -> Self {
        Self {
            command: command.to_string(),
            passed: true,
            violations: Vec::new(),
            head_changed: false,
            commit_count_changed: false,
        }
    }

    fn add_violation(&mut self, msg: &str) {
        self.violations.push(msg.to_string());
        self.passed = false;
    }
}

/// Verify .git is unchanged between snapshots.
fn verify_git_unchanged(
    before: &BTreeMap<String, String>,
    after: &BTreeMap<String, String>,
    head_before: Option<&String>,
    head_after: Option<&String>,
    count_before: usize,
    count_after: usize,
    command: &str,
) -> GitSafetyCheck {
    let mut check = GitSafetyCheck::new(command);

    if head_before != head_after {
        check.head_changed = true;
        check.add_violation(&format!("HEAD changed: {head_before:?} -> {head_after:?}"));
    }

    if count_before != count_after {
        check.commit_count_changed = true;
        check.add_violation(&format!(
            "Commit count changed: {count_before} -> {count_after}"
        ));
    }

    let before_filtered = filter_transient_git_files(before);
    let after_filtered = filter_transient_git_files(after);

    for path in after_filtered.keys() {
        if !before_filtered.contains_key(path) {
            check.add_violation(&format!("New file in .git/: {path}"));
        }
    }

    for (path, hash_before) in &before_filtered {
        if let Some(hash_after) = after_filtered.get(path)
            && hash_before != hash_after
        {
            check.add_violation(&format!("Modified file in .git/: {path}"));
        }
    }

    for path in before_filtered.keys() {
        if !after_filtered.contains_key(path) {
            check.add_violation(&format!("Deleted file from .git/: {path}"));
        }
    }

    check
}

/// Macro to run a command and verify git safety.
macro_rules! check_git_safety {
    ($workspace:expr, $before:expr, $head_before:expr, $count_before:expr, $args:expr, $label:expr) => {{
        let result = run_br($workspace, $args, $label);
        let after = snapshot_git_dir(&$workspace.root);
        let head_after = get_head_commit(&$workspace.root);
        let count_after = get_commit_count(&$workspace.root);

        let check = verify_git_unchanged(
            &$before,
            &after,
            $head_before.as_ref(),
            head_after.as_ref(),
            $count_before,
            count_after,
            $label,
        );

        assert!(
            check.passed,
            "GIT SAFETY VIOLATION in '{}' command:\n{}\nstdout: {}\nstderr: {}",
            $label,
            check.violations.join("\n"),
            result.stdout,
            result.stderr
        );

        (after, head_after, count_after, result)
    }};
}

/// Main test: verify all CLI commands don't touch .git
#[test]
fn regression_full_cli_does_not_touch_git() {
    let workspace = BrWorkspace::new();

    init_git_repo(&workspace);

    let baseline_git = snapshot_git_dir(&workspace.root);
    let baseline_head = get_head_commit(&workspace.root);
    let baseline_count = get_commit_count(&workspace.root);

    eprintln!(
        "Baseline: {} git files, HEAD={:?}, {} commits",
        baseline_git.len(),
        baseline_head,
        baseline_count
    );

    eprintln!("\n[PHASE 1] Testing initialization & workspace commands...");

    let (git_snap, head, count, init_result) = check_git_safety!(
        &workspace,
        baseline_git,
        baseline_head,
        baseline_count,
        ["init"],
        "init"
    );
    assert!(init_result.status.success(), "init failed");

    let (git_snap, head, count, _) =
        check_git_safety!(&workspace, git_snap, head, count, ["version"], "version");

    let (git_snap, head, count, _) = check_git_safety!(
        &workspace,
        git_snap,
        head,
        count,
        ["version", "--json"],
        "version_json"
    );

    let (git_snap, head, count, _) =
        check_git_safety!(&workspace, git_snap, head, count, ["where"], "where");

    let (git_snap, head, count, _) =
        check_git_safety!(&workspace, git_snap, head, count, ["info"], "info");

    let (git_snap, head, count, _) =
        check_git_safety!(&workspace, git_snap, head, count, ["doctor"], "doctor");

    eprintln!("\n[PHASE 2] Testing issue CRUD operations...");

    let (git_snap, head, count, create1) = check_git_safety!(
        &workspace,
        git_snap,
        head,
        count,
        [
            "create",
            "Test issue 1",
            "-p",
            "1",
            "-t",
            "task",
            "--no-auto-flush"
        ],
        "create1"
    );
    assert!(create1.status.success(), "create1 failed");
    let id1 = create1
        .stdout
        .lines()
        .next()
        .and_then(|l| l.strip_prefix("Created "))
        .and_then(|l| l.split(':').next())
        .unwrap_or("")
        .trim()
        .to_string();

    let (git_snap, head, count, create2) = check_git_safety!(
        &workspace,
        git_snap,
        head,
        count,
        [
            "create",
            "Test issue 2",
            "-d",
            "Description here",
            "-t",
            "bug",
            "--no-auto-flush"
        ],
        "create2"
    );
    let id2 = create2
        .stdout
        .lines()
        .next()
        .and_then(|l| l.strip_prefix("Created "))
        .and_then(|l| l.split(':').next())
        .unwrap_or("")
        .trim()
        .to_string();

    let (git_snap, head, count, q_result) = check_git_safety!(
        &workspace,
        git_snap,
        head,
        count,
        ["q", "Quick issue", "--no-auto-flush"],
        "quick_capture"
    );
    let id3 = q_result.stdout.trim().to_string();

    let (git_snap, head, count, _) =
        check_git_safety!(&workspace, git_snap, head, count, ["list"], "list");

    let (git_snap, head, count, _) = check_git_safety!(
        &workspace,
        git_snap,
        head,
        count,
        ["list", "--json"],
        "list_json"
    );

    let (git_snap, head, count, _) =
        check_git_safety!(&workspace, git_snap, head, count, ["show", &id1], "show");

    let (git_snap, head, count, _) = check_git_safety!(
        &workspace,
        git_snap,
        head,
        count,
        ["show", &id1, "--json"],
        "show_json"
    );

    let (git_snap, head, count, _) = check_git_safety!(
        &workspace,
        git_snap,
        head,
        count,
        ["update", &id1, "--priority", "0", "--no-auto-flush"],
        "update"
    );

    let (git_snap, head, count, _) = check_git_safety!(
        &workspace,
        git_snap,
        head,
        count,
        ["update", &id1, "--status", "in_progress", "--no-auto-flush"],
        "update_status"
    );

    eprintln!("\n[PHASE 3] Testing queries and filters...");

    let (git_snap, head, count, _) =
        check_git_safety!(&workspace, git_snap, head, count, ["ready"], "ready");

    let (git_snap, head, count, _) = check_git_safety!(
        &workspace,
        git_snap,
        head,
        count,
        ["ready", "--json"],
        "ready_json"
    );

    let (git_snap, head, count, _) =
        check_git_safety!(&workspace, git_snap, head, count, ["blocked"], "blocked");

    let (git_snap, head, count, _) = check_git_safety!(
        &workspace,
        git_snap,
        head,
        count,
        ["search", "Test"],
        "search"
    );

    let (git_snap, head, count, _) =
        check_git_safety!(&workspace, git_snap, head, count, ["count"], "count");

    let (git_snap, head, count, _) = check_git_safety!(
        &workspace,
        git_snap,
        head,
        count,
        ["count", "--by", "status"],
        "count_by_status"
    );

    let (git_snap, head, count, _) =
        check_git_safety!(&workspace, git_snap, head, count, ["stats"], "stats");

    let (git_snap, head, count, _) =
        check_git_safety!(&workspace, git_snap, head, count, ["stale"], "stale");

    eprintln!("\n[PHASE 4] Testing dependency management...");

    let (git_snap, head, count, _) = check_git_safety!(
        &workspace,
        git_snap,
        head,
        count,
        ["dep", "add", &id1, &id2, "--no-auto-flush"],
        "dep_add"
    );

    let (git_snap, head, count, _) = check_git_safety!(
        &workspace,
        git_snap,
        head,
        count,
        ["dep", "list", &id1],
        "dep_list"
    );

    let (git_snap, head, count, _) = check_git_safety!(
        &workspace,
        git_snap,
        head,
        count,
        ["dep", "tree", &id1],
        "dep_tree"
    );

    let (git_snap, head, count, _) = check_git_safety!(
        &workspace,
        git_snap,
        head,
        count,
        ["dep", "cycles"],
        "dep_cycles"
    );

    let (git_snap, head, count, _) = check_git_safety!(
        &workspace,
        git_snap,
        head,
        count,
        ["dep", "remove", &id1, &id2, "--no-auto-flush"],
        "dep_remove"
    );

    let (git_snap, head, count, _) = check_git_safety!(
        &workspace,
        git_snap,
        head,
        count,
        ["dep", "relate", &id1, &id3, "--no-auto-flush"],
        "dep_relate"
    );

    let (git_snap, head, count, _) = check_git_safety!(
        &workspace,
        git_snap,
        head,
        count,
        ["dep", "unrelate", &id1, &id3, "--no-auto-flush"],
        "dep_unrelate"
    );

    eprintln!("\n[PHASE 5] Testing label management...");

    let (git_snap, head, count, _) = check_git_safety!(
        &workspace,
        git_snap,
        head,
        count,
        ["label", "add", &id1, "priority", "--no-auto-flush"],
        "label_add"
    );

    let (git_snap, head, count, _) = check_git_safety!(
        &workspace,
        git_snap,
        head,
        count,
        ["label", "list", &id1],
        "label_list"
    );

    let (git_snap, head, count, _) = check_git_safety!(
        &workspace,
        git_snap,
        head,
        count,
        ["label", "remove", &id1, "priority", "--no-auto-flush"],
        "label_remove"
    );

    eprintln!("\n[PHASE 6] Testing comments...");

    let (git_snap, head, count, _) = check_git_safety!(
        &workspace,
        git_snap,
        head,
        count,
        [
            "comments",
            "add",
            &id1,
            "--message",
            "This is a test comment",
            "--author",
            "test",
            "--no-auto-flush"
        ],
        "comments_add"
    );

    let (git_snap, head, count, _) = check_git_safety!(
        &workspace,
        git_snap,
        head,
        count,
        ["comments", "list", &id1],
        "comments_list"
    );

    eprintln!("\n[PHASE 7] Testing defer/undefer...");

    let (git_snap, head, count, _) = check_git_safety!(
        &workspace,
        git_snap,
        head,
        count,
        ["defer", &id3, "--days", "7", "--no-auto-flush"],
        "defer"
    );

    let (git_snap, head, count, _) = check_git_safety!(
        &workspace,
        git_snap,
        head,
        count,
        ["undefer", &id3, "--no-auto-flush"],
        "undefer"
    );

    eprintln!("\n[PHASE 8] Testing config...");

    let (git_snap, head, count, _) = check_git_safety!(
        &workspace,
        git_snap,
        head,
        count,
        ["config", "--list"],
        "config_list"
    );

    let (git_snap, head, count, _) = check_git_safety!(
        &workspace,
        git_snap,
        head,
        count,
        ["config", "--get", "id.prefix"],
        "config_get"
    );

    eprintln!("\n[PHASE 9] Testing graph...");

    let (git_snap, head, count, _) =
        check_git_safety!(&workspace, git_snap, head, count, ["graph"], "graph");

    let (git_snap, head, count, _) = check_git_safety!(
        &workspace,
        git_snap,
        head,
        count,
        ["graph", "--json"],
        "graph_json"
    );

    eprintln!("\n[PHASE 10] Testing saved queries...");

    let (git_snap, head, count, _) = check_git_safety!(
        &workspace,
        git_snap,
        head,
        count,
        ["query", "add", "open-bugs", "status:open type:bug"],
        "query_add"
    );

    let (git_snap, head, count, _) = check_git_safety!(
        &workspace,
        git_snap,
        head,
        count,
        ["query", "list"],
        "query_list"
    );

    let (git_snap, head, count, _) = check_git_safety!(
        &workspace,
        git_snap,
        head,
        count,
        ["query", "run", "open-bugs"],
        "query_run"
    );

    let (git_snap, head, count, _) = check_git_safety!(
        &workspace,
        git_snap,
        head,
        count,
        ["query", "delete", "open-bugs"],
        "query_delete"
    );

    eprintln!("\n[PHASE 11] Testing history...");

    let (git_snap, head, count, _) = check_git_safety!(
        &workspace,
        git_snap,
        head,
        count,
        ["history", "list"],
        "history_list"
    );

    eprintln!("\n[PHASE 12] Testing audit...");

    let (git_snap, head, count, _) = check_git_safety!(
        &workspace,
        git_snap,
        head,
        count,
        ["audit", "--message", "Test audit entry", "--label", "test"],
        "audit"
    );

    eprintln!("\n[PHASE 13] Testing lint and orphans...");

    let (git_snap, head, count, _) =
        check_git_safety!(&workspace, git_snap, head, count, ["lint"], "lint");

    let (git_snap, head, count, _) =
        check_git_safety!(&workspace, git_snap, head, count, ["orphans"], "orphans");

    eprintln!("\n[PHASE 14] Testing epic commands...");

    let (git_snap, head, count, epic_result) = check_git_safety!(
        &workspace,
        git_snap,
        head,
        count,
        [
            "create",
            "Epic: Main feature",
            "-t",
            "epic",
            "--no-auto-flush"
        ],
        "create_epic"
    );
    let epic_id = epic_result
        .stdout
        .lines()
        .next()
        .and_then(|l| l.strip_prefix("Created "))
        .and_then(|l| l.split(':').next())
        .unwrap_or("")
        .trim()
        .to_string();

    let (git_snap, head, count, _) = check_git_safety!(
        &workspace,
        git_snap,
        head,
        count,
        [
            "dep",
            "add",
            &id1,
            &epic_id,
            "-t",
            "parent-child",
            "--no-auto-flush"
        ],
        "dep_add_parent"
    );

    let (git_snap, head, count, _) = check_git_safety!(
        &workspace,
        git_snap,
        head,
        count,
        ["epic", "status", &epic_id],
        "epic_status"
    );

    eprintln!("\n[PHASE 15] Testing changelog...");

    let (git_snap, head, count, _) = check_git_safety!(
        &workspace,
        git_snap,
        head,
        count,
        ["close", &id2, "--reason", "Done", "--no-auto-flush"],
        "close_for_changelog"
    );

    let (git_snap, head, count, _) = check_git_safety!(
        &workspace,
        git_snap,
        head,
        count,
        ["changelog"],
        "changelog"
    );

    let (git_snap, head, count, _) = check_git_safety!(
        &workspace,
        git_snap,
        head,
        count,
        ["changelog", "--json"],
        "changelog_json"
    );

    // PHASE 16: Sync operations (brief, detailed tests in e2e_sync_git_safety)
    eprintln!("\n[PHASE 16] Testing sync operations...");

    let (git_snap, head, count, _) = check_git_safety!(
        &workspace,
        git_snap,
        head,
        count,
        ["sync", "--flush-only"],
        "sync_flush"
    );

    let (git_snap, head, count, _) = check_git_safety!(
        &workspace,
        git_snap,
        head,
        count,
        ["sync", "--status"],
        "sync_status"
    );

    let (git_snap, head, count, _) = check_git_safety!(
        &workspace,
        git_snap,
        head,
        count,
        ["sync", "--import-only", "--force"],
        "sync_import"
    );

    eprintln!("\n[PHASE 17] Testing close/reopen/delete...");

    let (git_snap, head, count, _) = check_git_safety!(
        &workspace,
        git_snap,
        head,
        count,
        ["close", &id1, "--reason", "Completed", "--no-auto-flush"],
        "close"
    );

    let (git_snap, head, count, _) = check_git_safety!(
        &workspace,
        git_snap,
        head,
        count,
        ["reopen", &id1, "--no-auto-flush"],
        "reopen"
    );

    let (git_snap, head, count, _) = check_git_safety!(
        &workspace,
        git_snap,
        head,
        count,
        ["delete", &id3, "--reason", "Not needed", "--no-auto-flush"],
        "delete"
    );

    eprintln!("\n[PHASE 18] Testing completions...");

    let (git_snap, head, count, _) = check_git_safety!(
        &workspace,
        git_snap,
        head,
        count,
        ["completions", "bash"],
        "completions_bash"
    );

    let (git_snap, head, count, _) = check_git_safety!(
        &workspace,
        git_snap,
        head,
        count,
        ["completions", "zsh"],
        "completions_zsh"
    );

    let (_git_snap, _head, _count, _) = check_git_safety!(
        &workspace,
        git_snap,
        head,
        count,
        ["completions", "fish"],
        "completions_fish"
    );

    eprintln!("\n[FINAL] Verifying baseline comparison...");

    let final_head = get_head_commit(&workspace.root);
    let final_count = get_commit_count(&workspace.root);

    assert_eq!(
        baseline_head, final_head,
        "HEAD changed during full CLI test!\nBaseline: {baseline_head:?}\nFinal: {final_head:?}"
    );

    assert_eq!(
        baseline_count, final_count,
        "Commit count changed during full CLI test!\nBaseline: {baseline_count}\nFinal: {final_count}"
    );

    eprintln!(
        "\n[PASS] Full CLI git safety test passed!\n\
         - Tested all major command categories\n\
         - HEAD unchanged: {final_head:?}\n\
         - Commit count unchanged: {final_count}"
    );
}

/// Test that auto-flush doesn't touch .git
#[test]
fn regression_auto_flush_does_not_touch_git() {
    let workspace = BrWorkspace::new();
    init_git_repo(&workspace);

    let init = run_br(&workspace, ["init"], "init");
    assert!(init.status.success());

    let baseline_git = snapshot_git_dir(&workspace.root);
    let baseline_head = get_head_commit(&workspace.root);
    let baseline_count = get_commit_count(&workspace.root);

    let create = run_br(&workspace, ["create", "Auto-flush test"], "create_auto");
    assert!(create.status.success());

    let after_git = snapshot_git_dir(&workspace.root);
    let after_head = get_head_commit(&workspace.root);
    let after_count = get_commit_count(&workspace.root);

    let check = verify_git_unchanged(
        &baseline_git,
        &after_git,
        baseline_head.as_ref(),
        after_head.as_ref(),
        baseline_count,
        after_count,
        "auto-flush",
    );

    assert!(
        check.passed,
        "GIT SAFETY VIOLATION: auto-flush touched .git:\n{}",
        check.violations.join("\n")
    );

    eprintln!("[PASS] Auto-flush does not touch .git");
}

/// Test that auto-import doesn't touch .git
#[test]
fn regression_auto_import_does_not_touch_git() {
    let workspace = BrWorkspace::new();
    init_git_repo(&workspace);

    let init = run_br(&workspace, ["init"], "init");
    assert!(init.status.success());

    let _ = run_br(
        &workspace,
        ["create", "Issue 1", "--no-auto-flush"],
        "create1",
    );

    let _ = run_br(&workspace, ["sync", "--flush-only"], "flush");

    let jsonl_path = workspace.root.join(".beads").join("issues.jsonl");
    if jsonl_path.exists() {
        let content = fs::read_to_string(&jsonl_path).expect("read");
        std::thread::sleep(std::time::Duration::from_millis(50));
        fs::write(&jsonl_path, content).expect("write");
    }

    let baseline_git = snapshot_git_dir(&workspace.root);
    let baseline_head = get_head_commit(&workspace.root);
    let baseline_count = get_commit_count(&workspace.root);

    let list = run_br(&workspace, ["list"], "list_auto_import");
    assert!(list.status.success());

    let after_git = snapshot_git_dir(&workspace.root);
    let after_head = get_head_commit(&workspace.root);
    let after_count = get_commit_count(&workspace.root);

    let check = verify_git_unchanged(
        &baseline_git,
        &after_git,
        baseline_head.as_ref(),
        after_head.as_ref(),
        baseline_count,
        after_count,
        "auto-import",
    );

    assert!(
        check.passed,
        "GIT SAFETY VIOLATION: auto-import touched .git:\n{}",
        check.violations.join("\n")
    );

    eprintln!("[PASS] Auto-import does not touch .git");
}
