//! Fail-closed pipe contract (beads_rust-3fna): `br list | head` must be a
//! normal close — exit 0, no SIGABRT/core dump — matching standard CLI
//! convention for read-only commands.

mod common;

use common::cli::{BrWorkspace, run_br};

fn br_bin() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_br"))
}

/// Deterministic variant: the reader end of stdout is closed before `br`
/// writes anything, so every text write hits EPIPE exactly like
/// `br list | head` where head exits early.
#[test]
fn list_into_closed_stdout_exits_zero() {
    let workspace = BrWorkspace::new();
    let init = run_br(&workspace, ["init"], "init");
    assert!(init.status.success(), "init failed: {}", init.stderr);
    for i in 0..30 {
        let create = run_br(
            &workspace,
            ["create", &format!("pipe filler {i}"), "-t", "task"],
            &format!("c_{i}"),
        );
        assert!(create.status.success(), "create failed");
    }

    use std::process::{Command, Stdio};
    let mut child = Command::new(br_bin())
        .current_dir(&workspace.root)
        .arg("list")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn br list");
    // Close the read end immediately: from here on, every br write to
    // stdout returns EPIPE (the Rust runtime ignores SIGPIPE).
    drop(child.stdout.take());
    let status = child.wait().expect("wait for br list");
    assert_eq!(
        status.code(),
        Some(0),
        "closed-stdout text output must be a normal close (exit 0); got {status:?}"
    );
}

/// The literal captain-specified pipeline form.
#[test]
fn list_into_head_exits_zero() {
    let workspace = BrWorkspace::new();
    let init = run_br(&workspace, ["init"], "init");
    assert!(init.status.success(), "init failed: {}", init.stderr);
    for i in 0..30 {
        let create = run_br(
            &workspace,
            ["create", &format!("pipe filler {i}"), "-t", "task"],
            &format!("c_{i}"),
        );
        assert!(create.status.success(), "create failed");
    }

    use std::process::{Command, Stdio};
    let pipeline = format!("{} list | head -1", br_bin().display());
    let out = Command::new("sh")
        .arg("-c")
        .arg(&pipeline)
        .current_dir(&workspace.root)
        .output()
        .expect("spawn sh pipeline");
    let code = out.status.code();
    assert_eq!(
        code,
        Some(0),
        "br list | head -1 must exit 0 (got {code:?}, stderr: {})",
        String::from_utf8_lossy(&out.stderr)
    );
}
