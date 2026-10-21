mod common;

use beads::storage::SqliteStorage;
use common::cli::{BrWorkspace, run_br};
use std::fs;

/// The root-cause test for the gate-verdict destruction.
///
/// The issues JSONL is a PARTIAL source of truth: it carries issues, not gate
/// verdict rows. A rebuild from it therefore emptied the verdict store, and
/// the next flush wrote that emptiness over `.beads/gates.jsonl` - every
/// closed bead then reported "no legal verdict", and `closed -> closed` is not
/// a legal gate transition, so the repair was a reopen/re-gate/re-close per
/// bead.
#[test]
fn doctor_repair_restores_the_gate_verdicts_a_rebuild_would_drop() {
    let workspace = BrWorkspace::new();
    let init = run_br(&workspace, ["init"], "init");
    assert!(init.status.success(), "init failed: {}", init.stderr);

    let created = run_br(
        &workspace,
        [
            "create",
            "Verdict carrier",
            "-p",
            "1",
            "-t",
            "task",
            "-d",
            "why the verdict must survive a rebuild",
            "--verify",
            "true",
            "--principle",
            "prove-it-works — the verdict row is the proof",
        ],
        "create",
    );
    assert!(
        created.status.success(),
        "create failed: {}",
        created.stderr
    );
    let id = parse_created_id(&created.stdout);
    assert!(!id.is_empty(), "create must report the id: {}", created.stdout);

    // Record a PASS verdict for the close.
    let gate = run_br(
        &workspace,
        [
            "gate",
            "report",
            &id,
            "--gate",
            "unit-test-verified",
            "--provider",
            "tester",
            "--status",
            "pass",
            "--to",
            "closed",
            "--note",
            "sha=0000000000000000000000000000000000000000 verdict=unit-test-verified",
        ],
        "gate",
    );
    assert!(gate.status.success(), "gate report failed: {}", gate.stderr);

    // FLUSH: the issues export plus the verdict sidecar.
    let flush = run_br(&workspace, ["sync", "--flush-only"], "flush");
    assert!(flush.status.success(), "flush failed: {}", flush.stderr);

    let beads_dir = workspace.root.join(".beads");
    let gates_path = beads_dir.join("gates.jsonl");
    let sidecar = fs::read_to_string(&gates_path).expect("read the sidecar");
    assert!(
        sidecar.lines().any(|line| !line.trim().is_empty()),
        "the sidecar must carry the verdict row: {sidecar}"
    );

    // Destroy the database: the JSONL pair is now the only source of truth.
    let db_path = beads_dir.join("beads.db");
    fs::remove_file(&db_path).expect("remove the database");

    // The repair rebuilds the database family from the JSONL.
    let repair = run_br(&workspace, ["doctor", "--repair"], "repair");
    assert!(
        repair.status.success(),
        "repair failed: {} {}",
        repair.stdout,
        repair.stderr
    );

    // The verdict must be BACK in the rebuilt database.
    let storage = SqliteStorage::open(&db_path).expect("open the rebuilt database");
    let rows = storage
        .get_scoped_gate_results(&id, "open", "closed")
        .expect("query the verdicts");
    assert_eq!(rows.len(), 1, "the verdict must survive the rebuild");
    assert!(rows[0].passed, "the restored verdict must be the PASS row");

    // And the next flush must NOT rewrite the sidecar empty.
    let flush_again = run_br(&workspace, ["sync", "--flush-only"], "flush_again");
    assert!(
        flush_again.status.success(),
        "second flush failed: {}",
        flush_again.stderr
    );
    let sidecar_after = fs::read_to_string(&gates_path).expect("read the sidecar");
    assert!(
        sidecar_after.lines().any(|line| !line.trim().is_empty()),
        "the flush must not empty the sidecar: {sidecar_after}"
    );
}

fn parse_created_id(stdout: &str) -> String {
    let line = stdout.lines().next().unwrap_or("");
    let normalized = line
        .strip_prefix("✓ ")
        .or_else(|| line.strip_prefix("✗ "))
        .unwrap_or(line);
    normalized
        .strip_prefix("Created ")
        .and_then(|rest| rest.split(':').next())
        .unwrap_or("")
        .trim()
        .to_string()
}
