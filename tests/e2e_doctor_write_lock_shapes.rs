//! E2E coverage for non-regular `.beads/.write.lock` nodes
//! (bead `beads-5sej`).
//!
//! The symlink shape is exercised through the doctor fixture suite
//! (`tests/doctor_fixtures/write_lock_symlink_node/`): startup follows the
//! symlink, doctor runs, and the `write_lock` check fails closed with a
//! typed diagnostic. The **directory** shape cannot reach that check —
//! startup lock acquisition fails first — so this e2e pins the fail-closed
//! behavior at the CLI boundary instead: `br doctor` (and any mutating
//! command) must exit non-zero and must never remove or replace the node.

mod common;

use assert_cmd::Command;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

/// Hermetic `br` invocation rooted at `cwd` (same shape as the doctor
/// chokepoint e2e).
fn br_cmd(cwd: &Path) -> Command {
    let mut cmd = Command::cargo_bin("br").expect("locate br binary");
    cmd.current_dir(cwd);
    cmd.env("NO_COLOR", "1");
    cmd.env("RUST_LOG", "warn");
    cmd.env("HOME", cwd);
    cmd.env("PATH", common::cli::deduplicated_br_path());
    for (key, _) in std::env::vars_os() {
        let key_s = key.to_string_lossy();
        if key_s.starts_with("BD_") || key_s.starts_with("BEADS_") {
            cmd.env_remove(&key);
        }
    }
    cmd
}

#[test]
fn doctor_fails_loudly_when_write_lock_is_a_directory() {
    let tmp = TempDir::new().expect("tempdir");
    let ws = tmp.path();
    let out = br_cmd(ws).arg("init").output().expect("br init spawned");
    assert!(out.status.success(), "br init failed: {out:?}");

    // Control arm, same workspace: doctor succeeds while the lock node is
    // still an ordinary file, so the failure below is attributable to the
    // planted node alone. Without it the assertion pins only "doctor exited
    // non-zero", which any unrelated breakage would also satisfy.
    let healthy = br_cmd(ws)
        .arg("doctor")
        .output()
        .expect("br doctor spawned");
    assert!(
        healthy.status.success(),
        "control: doctor must succeed on a healthy workspace; stdout={} stderr={}",
        String::from_utf8_lossy(&healthy.stdout),
        String::from_utf8_lossy(&healthy.stderr),
    );

    let lock = ws.join(".beads/.write.lock");
    if lock.exists() {
        fs::remove_file(&lock).expect("clear seeded lock file");
    }
    fs::create_dir(&lock).expect("plant directory lock node");

    let out = br_cmd(ws)
        .arg("doctor")
        .output()
        .expect("br doctor spawned");
    assert!(
        !out.status.success(),
        "doctor must fail closed on a directory .write.lock; stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );

    // Fail-closed must not mutate: the directory node survives untouched.
    assert!(
        lock.is_dir(),
        "directory .write.lock was removed or replaced by doctor"
    );
}

/// A create that clears every fail-closed create gate: description, verify,
/// and a principles citation because the seed is P≤2 (`src/cli/commands/
/// create.rs` refuses all three). `br create` became fail-closed after these
/// e2e were written, so the seeds now carry their own gates.
fn gated_create_args() -> Vec<&'static str> {
    vec![
        "create",
        "should not land",
        "--type",
        "task",
        "--priority",
        "2",
        "--description",
        "e2e seed: this create must be refused by the lock node, not by validation",
        "--verify",
        "br doctor",
        "--principle",
        "prove-it-works — this seed must be refused by the lock node, not by the create gate",
    ]
}

#[test]
fn mutating_command_fails_loudly_when_write_lock_is_a_directory() {
    let tmp = TempDir::new().expect("tempdir");
    let ws = tmp.path();
    let out = br_cmd(ws).arg("init").output().expect("br init spawned");
    assert!(out.status.success(), "br init failed: {out:?}");

    // Control arm, same workspace and same argv: the create succeeds while
    // the lock node is still an ordinary file. This is what makes the
    // assertion below non-vacuous — validation would refuse the very same
    // command, so without this arm the test passed with the lock guard
    // deleted and proved nothing about the lock at all.
    let control = br_cmd(ws)
        .args(gated_create_args())
        .output()
        .expect("br create spawned");
    assert!(
        control.status.success(),
        "control: the gated create must succeed before the lock node is planted; \
         stdout={} stderr={}",
        String::from_utf8_lossy(&control.stdout),
        String::from_utf8_lossy(&control.stderr),
    );

    let lock = ws.join(".beads/.write.lock");
    if lock.exists() {
        fs::remove_file(&lock).expect("clear seeded lock file");
    }
    fs::create_dir(&lock).expect("plant directory lock node");

    let out = br_cmd(ws)
        .args(gated_create_args())
        .output()
        .expect("br create spawned");
    assert!(
        !out.status.success(),
        "create must fail when the lock node is a directory; stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    assert!(lock.is_dir(), "directory .write.lock was disturbed");
}
