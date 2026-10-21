//! ADR-0001 §5.3 fail-closed close contract (beads_rust-fail-closed-close-rpay).
//!
//! The five §8.4 named tests: a close is a legal PASS gate row plus a commit
//! SHA, never a reason string. Default policy is ON for fresh workspaces
//! (`br init` writes it; absent-file workspaces get the same semantics).

// governed-by: ADR-0001

mod common;

use common::cli::{BrWorkspace, extract_json_payload, parse_created_id, run_br};

fn setup_workspace_with_issue() -> (BrWorkspace, String) {
    let workspace = BrWorkspace::new();

    let init = run_br(&workspace, ["init"], "init");
    assert!(init.status.success(), "init failed: {}", init.stderr);

    let create = run_br(
        &workspace,
        [
            "create",
            "Fail-closed close contract",
            "-p",
            "2",
            "-t",
            "task",
            "-d",
            "close contract brief",
            "--verify",
            "true",
            "--principle",
            "prove-it-works — close contract brief",
        ],
        "create_issue",
    );
    assert!(create.status.success(), "create failed: {}", create.stderr);
    let id = parse_created_id(&create.stdout);

    (workspace, id)
}

/// A REAL commit in the workspace's git repo, citing the bead ids.
///
/// The close path verifies that the sha it is given RESOLVES and CITES the
/// bead, so a placeholder sha proves nothing about the rule under test - and
/// a sha that resolves nowhere is now refused rather than warned about.
fn commit_citing(workspace: &BrWorkspace, bead_ids: &[&str]) -> String {
    let git = |args: &[&str]| -> String {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(&workspace.root)
            .output()
            .expect("git runs");
        assert!(out.status.success(), "git {args:?} failed: {out:?}");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    if !workspace.root.join(".git").exists() {
        git(&["init", "-q"]);
    }
    git(&["config", "user.email", "test@example.com"]);
    git(&["config", "user.name", "test"]);
    // One work file per call: a second call must still have something to
    // commit (`git commit` refuses an empty index).
    let count = std::process::Command::new("git")
        .args(["rev-list", "--count", "HEAD"])
        .current_dir(&workspace.root)
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .unwrap_or_else(|| "0".to_string());
    let work_file = format!("work-{count}.txt");
    std::fs::write(
        workspace.root.join(&work_file),
        format!("work for {}\n", bead_ids.join(", ")),
    )
    .expect("write work");
    git(&["add", &work_file]);
    git(&[
        "commit",
        "-q",
        "-m",
        &format!("fix: carry the work for {}", bead_ids.join(", ")),
    ]);
    git(&["rev-parse", "HEAD"])
}

fn record_unit_test_pass(workspace: &BrWorkspace, id: &str) {
    let report = run_br(
        workspace,
        [
            "gate",
            "report",
            id,
            "--gate",
            "unit-test-verified",
            "--provider",
            "verifier",
            "--status",
            "pass",
            "--to",
            "closed",
        ],
        "gate_report_pass",
    );
    assert!(
        report.status.success(),
        "gate report failed: {}",
        report.stderr
    );
}

/// §8.4: `close_without_pass_gate_is_nonzero` — no PASS row, no close.
#[test]
fn close_without_pass_gate_is_nonzero() {
    let _log = common::test_log("close_without_pass_gate_is_nonzero");
    let (workspace, id) = setup_workspace_with_issue();

    let sha = commit_citing(&workspace, &[&id]);
    let closed = run_br(
        &workspace,
        ["close", &id, "--commit-sha", &sha, "--json"],
        "close_without_gate",
    );
    assert!(
        !closed.status.success(),
        "close without a legal PASS gate row must be non-zero: {}",
        closed.stdout
    );
    assert!(
        closed.stdout.contains("fail-closed") || closed.stdout.contains("no legal PASS gate row"),
        "error should name the fail-closed gate: {}",
        closed.stdout
    );

    // The bead must still be open.
    let show = run_br(&workspace, ["show", &id, "--json"], "show");
    assert!(show.status.success(), "{}", show.stderr);
    let payload = extract_json_payload(&show.stdout);
    let issues: serde_json::Value = serde_json::from_str(&payload).expect("valid json");
    assert_eq!(issues["status"], "open", "{issues}");
}

/// §8.4: `close_without_commit_sha_is_nonzero` — a legal PASS row alone does
/// not authorize a close; the SHA is required.
#[test]
fn close_without_commit_sha_is_nonzero() {
    let _log = common::test_log("close_without_commit_sha_is_nonzero");
    let (workspace, id) = setup_workspace_with_issue();
    record_unit_test_pass(&workspace, &id);

    let closed = run_br(&workspace, ["close", &id, "--json"], "close_without_sha");
    assert!(
        !closed.status.success(),
        "close without --commit-sha must be non-zero: {}",
        closed.stdout
    );
    assert!(
        closed.stdout.contains("commit-sha") || closed.stdout.contains("--commit-sha"),
        "error should name the missing SHA requirement: {}",
        closed.stdout
    );
}

/// §8.4: `close_with_legal_gate_and_sha_sets_close_verdict` — gate row first,
/// then close with the SHA; the authorizing verdict lands in the close
/// metadata audit trail.
#[test]
fn close_with_legal_gate_and_sha_sets_close_verdict() {
    let _log = common::test_log("close_with_legal_gate_and_sha_sets_close_verdict");
    let (workspace, id) = setup_workspace_with_issue();
    record_unit_test_pass(&workspace, &id);

    let sha = commit_citing(&workspace, &[&id]);
    let closed = run_br(
        &workspace,
        ["close", &id, "--commit-sha", &sha, "--json"],
        "close_legal",
    );
    assert!(
        closed.status.success(),
        "legal close failed: {}",
        closed.stderr
    );

    let show = run_br(&workspace, ["show", &id, "--json"], "show_closed");
    let payload = extract_json_payload(&show.stdout);
    let issues: serde_json::Value = serde_json::from_str(&payload).expect("valid json");
    assert_eq!(issues["status"], "closed", "{issues}");

    // The verdict kind whose PASS row authorized the close is recorded in
    // close_metadata's gates JSON (`close_verdict=<name>` audit entry) until
    // schema v18 promotes issue.close_verdict to a real column.
    let db = workspace.root.join(".beads").join("beads.db");
    let output = std::process::Command::new("sqlite3")
        .arg(&db)
        .arg(format!(
            "SELECT policy_gates_fired FROM close_metadata WHERE issue_id = '{id}';"
        ))
        .output()
        .expect("query close_metadata");
    let meta = String::from_utf8_lossy(&output.stdout);
    assert!(
        meta.contains("close_verdict=unit-test-verified"),
        "expected close_verdict=unit-test-verified in metadata, got: {meta}"
    );
    assert!(
        meta.contains(&format!("commit_sha={sha}")),
        "expected commit_sha in metadata, got: {meta}"
    );
}

/// §8.4: `bypass_without_br_operator_is_rejected` — bypass is an operator
/// action; agents must satisfy the gates instead.
#[test]
fn bypass_without_br_operator_is_rejected() {
    let _log = common::test_log("bypass_without_br_operator_is_rejected");
    let (workspace, id) = setup_workspace_with_issue();

    let closed = run_br(
        &workspace,
        [
            "close",
            &id,
            "--bypass-policy",
            "--bypass-reason",
            "operator override attempt",
        ],
        "bypass_without_operator",
    );
    assert!(
        !closed.status.success(),
        "bypass without BR_OPERATOR=1 must be rejected: {} {}",
        closed.stdout,
        closed.stderr
    );
    let combined = format!("{}{}", closed.stdout, closed.stderr);
    assert!(
        combined.contains("BR_OPERATOR"),
        "rejection should name BR_OPERATOR=1: {combined}"
    );
}

/// ADR-0001 §5.3 defaulting contract (beads_rust-gate-report-default-uro91):
/// when policy.yaml is ABSENT or `workflow.strict` is unset with no gates,
/// BOTH the gate-record path and the fail-closed close path must resolve the
/// same ADR default (`require_legal_close: true`). Before uro91 the record
/// side inverted this: `br gate report` refused verdict rows while `br close`
/// demanded them — the catch-22 that forced CalmLantern's operator bypass.
fn setup_workspace_without_policy() -> (BrWorkspace, String) {
    let workspace = BrWorkspace::new();
    std::fs::create_dir_all(workspace.root.join(".beads")).expect("beads dir");
    // Deliberately NO `br init`: `.beads/policy.yaml` stays absent.
    let create = run_br(
        &workspace,
        [
            "create",
            "Absent-policy gate-report default",
            "-p",
            "2",
            "-t",
            "task",
            "-d",
            "absent-policy brief",
            "--verify",
            "true",
            "--principle",
            "prove-it-works — absent-policy brief",
        ],
        "create_issue",
    );
    assert!(create.status.success(), "create failed: {}", create.stderr);
    let id = parse_created_id(&create.stdout);
    (workspace, id)
}

fn record_and_close_legally(workspace: &BrWorkspace, id: &str, label_suffix: &str) {
    let report = run_br(
        workspace,
        [
            "gate",
            "report",
            id,
            "--gate",
            "unit-test-verified",
            "--provider",
            "verifier",
            "--status",
            "pass",
            "--to",
            "closed",
        ],
        Box::leak(format!("gate_report_pass_{label_suffix}").into_boxed_str()),
    );
    assert!(
        report.status.success(),
        "gate report must accept the ADR default when policy is absent/unstrict: {} {}",
        report.stdout,
        report.stderr
    );

    let sha = commit_citing(workspace, &[id]);
    let closed = run_br(
        workspace,
        ["close", id, "--commit-sha", &sha, "--json"],
        Box::leak(format!("close_legal_{label_suffix}").into_boxed_str()),
    );
    assert!(
        closed.status.success(),
        "close must accept the recorded PASS row WITHOUT bypass: {} {}",
        closed.stdout,
        closed.stderr
    );
}

/// uro91: absent policy.yaml — gate report records the verdict row and close
/// accepts it legally, no bypass.
#[test]
fn gate_report_default_records_row_when_policy_absent() {
    let _log = common::test_log("gate_report_default_records_row_when_policy_absent");
    let (workspace, id) = setup_workspace_without_policy();
    record_and_close_legally(&workspace, &id, "absent");

    let show = run_br(&workspace, ["show", &id, "--json"], "show_closed");
    assert!(show.status.success(), "{}", show.stderr);
    let payload = extract_json_payload(&show.stdout);
    let issues: serde_json::Value = serde_json::from_str(&payload).expect("valid json");
    assert_eq!(issues["status"], "closed", "{issues}");
}

/// uro91: policy.yaml present but `workflow.strict` unset with no gates —
/// same fail-closed default applies on BOTH sides.
#[test]
fn gate_report_default_records_row_when_strict_unset() {
    let _log = common::test_log("gate_report_default_records_row_when_strict_unset");
    let (workspace, id) = setup_workspace_with_issue();
    std::fs::write(
        workspace.root.join(".beads").join("policy.yaml"),
        "workflow:\n  statuses: [open, closed]\n  transitions:\n    open: [closed]\n",
    )
    .expect("write strict-unset policy");
    record_and_close_legally(&workspace, &id, "strict_unset");
}

/// uro91: an installed explicit policy wins — the ADR default must NOT leak
/// into a project that configured its own (here: gate-less) workflow.
///
/// The GATE RULES are the one thing the default still supplies, because
/// "strict: true" with no rules is otherwise a lie (ADR-0001 §5.3,
/// stack-audit §18): the close demanded no verdict AND `br gate report --to
/// closed` found no rule to record one against, so the gate discipline was
/// unreachable. Strict means strict.
///
/// The project's own STATUSES and TRANSITIONS govern — the ADR default's
/// transitions must not leak into it, and this is what proves that.
#[test]
fn gate_report_default_explicit_file_still_governs() {
    let _log = common::test_log("gate_report_default_explicit_file_still_governs");
    let (workspace, id) = setup_workspace_with_issue();
    std::fs::write(
        workspace.root.join(".beads").join("policy.yaml"),
        "workflow:\n  strict: true\n  statuses: [open, in_progress, closed]\n  transitions:\n    open: [closed]\n",
    )
    .expect("write explicit policy");

    let report = run_br(
        &workspace,
        [
            "gate",
            "report",
            &id,
            "--gate",
            "unit-test-verified",
            "--provider",
            "verifier",
            "--status",
            "pass",
            "--to",
            "closed",
        ],
        "gate_report_explicit_refusal",
    );
    assert!(
        report.status.success(),
        "a strict policy with no gate rules still gets the default's gate rules: strict means \
         strict, or the close would demand a verdict that nothing could record: {report:?}"
    );

    // The default's TRANSITIONS must not leak: the ADR default allows
    // open -> in_progress, this policy does not.
    let leaked = run_br(
        &workspace,
        [
            "gate",
            "report",
            &id,
            "--gate",
            "unit-test-verified",
            "--provider",
            "verifier",
            "--status",
            "pass",
            "--to",
            "in_progress",
        ],
        "gate_report_transition_leak",
    );
    assert!(
        !leaked.status.success(),
        "the ADR default's transitions must not leak into a project that configured its own: \
         {leaked:?}"
    );
}

/// §8.4: `init_writes_require_legal_close_policy` — `br init` writes the
/// ADR-0001 default policy; the illegal require_all-of-five shape never
/// appears.
#[test]
fn init_writes_require_legal_close_policy() {
    let _log = common::test_log("init_writes_require_legal_close_policy");
    let workspace = BrWorkspace::new();

    let init = run_br(&workspace, ["init"], "init");
    assert!(init.status.success(), "init failed: {}", init.stderr);

    let policy_path = workspace.root.join(".beads").join("policy.yaml");
    let policy = std::fs::read_to_string(&policy_path).expect("br init writes .beads/policy.yaml");
    assert!(
        policy.contains("require_legal_close: true"),
        "default policy must enable require_legal_close: {policy}"
    );

    // The written file must LOAD cleanly — i.e. it must not carry the
    // illegal require_all-of-five shape that load_for_beads_dir rejects.
    let list = run_br(&workspace, ["list", "--json"], "list_after_init");
    assert!(
        list.status.success(),
        "policy.yaml written by init failed to load: {} {}",
        list.stdout,
        list.stderr
    );
}

/// bd-2qu9: `close_sha_cites_bead_foreign_sha_fails_loud` — closing with a
/// real commit that cites a DIFFERENT bead fails naming the expected id.
#[test]
fn close_sha_cites_bead_foreign_sha_fails_loud() {
    let _log = common::test_log("close_sha_cites_bead_foreign_sha_fails_loud");
    let (workspace, id) = setup_workspace_with_issue();
    record_unit_test_pass(&workspace, &id);

    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(&workspace.root)
            .output()
            .expect("git runs");
        assert!(out.status.success(), "git {args:?} failed: {out:?}");
        out
    };
    git(&["init"]);
    git(&["config", "user.email", "test@example.com"]);
    git(&["config", "user.name", "test"]);
    std::fs::write(workspace.root.join("work.txt"), "work\n").expect("write work");
    git(&["add", "."]);
    git(&["commit", "-m", "fix bd-someone-else done"]);
    let sha_out = git(&["rev-parse", "HEAD"]);
    let sha = String::from_utf8_lossy(&sha_out.stdout).trim().to_string();

    let closed = run_br(
        &workspace,
        [
            "close",
            &id,
            "--commit-sha",
            &sha,
            "--reason",
            "done",
            "--json",
        ],
        "close_foreign_sha",
    );
    assert!(
        !closed.status.success(),
        "close with a foreign sha must be non-zero: {}",
        closed.stdout
    );
    let transcript = format!("{}{}", closed.stdout, closed.stderr);
    assert!(
        transcript.contains(&id),
        "refusal must name the expected bead id {id}: {transcript}"
    );

    let show = run_br(&workspace, ["show", &id, "--json"], "show_still_open");
    assert!(show.status.success(), "{}", show.stderr);
    let payload = extract_json_payload(&show.stdout);
    let issues: serde_json::Value = serde_json::from_str(&payload).expect("valid json");
    assert_eq!(issues["status"], "open", "{issues}");
}

/// bd-2qu9: `close_sha_cites_bead_citing_sha_closes` — a real commit citing
/// the bead closes legally (gate row + citing sha).
#[test]
fn close_sha_cites_bead_citing_sha_closes() {
    let _log = common::test_log("close_sha_cites_bead_citing_sha_closes");
    let (workspace, id) = setup_workspace_with_issue();
    record_unit_test_pass(&workspace, &id);

    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(&workspace.root)
            .output()
            .expect("git runs");
        assert!(out.status.success(), "git {args:?} failed: {out:?}");
        out
    };
    git(&["init"]);
    git(&["config", "user.email", "test@example.com"]);
    git(&["config", "user.name", "test"]);
    std::fs::write(workspace.root.join("work.txt"), "work\n").expect("write work");
    git(&["add", "."]);
    git(&["commit", "-m", &format!("feat: done ({id})")]);
    let sha_out = git(&["rev-parse", "HEAD"]);
    let sha = String::from_utf8_lossy(&sha_out.stdout).trim().to_string();

    let closed = run_br(
        &workspace,
        [
            "close",
            &id,
            "--commit-sha",
            &sha,
            "--reason",
            "done",
            "--json",
        ],
        "close_citing_sha",
    );
    assert!(
        closed.status.success(),
        "close with a citing sha must succeed: {} {}",
        closed.stdout,
        closed.stderr
    );
}
