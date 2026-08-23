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
        ],
        "create_issue",
    );
    assert!(create.status.success(), "create failed: {}", create.stderr);
    let id = parse_created_id(&create.stdout);

    (workspace, id)
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

    let closed = run_br(
        &workspace,
        ["close", &id, "--commit-sha", "deadbee", "--json"],
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
    assert_eq!(issues[0]["status"], "open", "{issues}");
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

    let closed = run_br(
        &workspace,
        ["close", &id, "--commit-sha", "9ef55ebf", "--json"],
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
    assert_eq!(issues[0]["status"], "closed", "{issues}");

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
        meta.contains("commit_sha=9ef55ebf"),
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

    let closed = run_br(
        workspace,
        ["close", id, "--commit-sha", "deadbee", "--json"],
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
    assert_eq!(issues[0]["status"], "closed", "{issues}");
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
#[test]
fn gate_report_default_explicit_file_still_governs() {
    let _log = common::test_log("gate_report_default_explicit_file_still_governs");
    let (workspace, id) = setup_workspace_with_issue();
    std::fs::write(
        workspace.root.join(".beads").join("policy.yaml"),
        "workflow:\n  strict: true\n  statuses: [open, closed]\n  transitions:\n    open: [closed]\n",
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
        !report.status.success(),
        "explicit file without legal-close gating must still refuse: {report:?}"
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
