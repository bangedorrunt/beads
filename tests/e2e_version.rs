//! E2E tests for the version command.
//!
//! Tests the `br version` command and its flags: --check, --short, --json.
//! Part of beads-1hof.

mod common;

use common::cli::{BrWorkspace, extract_json_payload, run_br};
use serde_json::Value;

#[test]
fn e2e_version_short_flag() {
    let _log = common::test_log("e2e_version_short_flag");
    let workspace = BrWorkspace::new();

    let version = run_br(&workspace, ["version", "--short"], "version_short");
    assert!(
        version.status.success(),
        "version --short failed: {}",
        version.stderr
    );

    let stdout = version.stdout.trim();
    assert!(
        stdout.chars().all(|c| c.is_numeric() || c == '.'),
        "version --short should contain only version number, got: '{}'",
        stdout
    );
    assert!(
        stdout.contains('.'),
        "version --short should look like semver, got: '{}'",
        stdout
    );
}

#[test]
fn e2e_version_json_flag() {
    let _log = common::test_log("e2e_version_json_flag");
    let workspace = BrWorkspace::new();

    let version = run_br(&workspace, ["version", "--json"], "version_json");
    assert!(
        version.status.success(),
        "version --json failed: {}",
        version.stderr
    );

    let payload = extract_json_payload(&version.stdout);
    let json: Value = serde_json::from_str(&payload).expect("valid JSON");

    assert!(json.get("version").is_some(), "missing version field");
    assert!(json.get("build").is_some(), "missing build field");
    if option_env!("VERGEN_GIT_SHA").is_some() {
        assert!(json.get("commit").is_some(), "missing commit field");
    }
    #[cfg(feature = "self_update")]
    assert!(json.get("features").is_some(), "missing features field");
    #[cfg(not(feature = "self_update"))]
    assert!(
        json.get("features").is_none(),
        "features field should be absent without self_update"
    );
}

#[test]
fn e2e_version_check_flag() {
    let _log = common::test_log("e2e_version_check_flag");
    let workspace = BrWorkspace::new();

    let version = run_br(&workspace, ["version", "--check"], "version_check");

    if version.status.success() {
        assert!(
            version.stdout.contains("up to date") || version.stdout.contains("Update available"),
            "version --check stdout unexpected: {}",
            version.stdout
        );
    } else {
        let code = version.status.code().unwrap_or(0);
        assert!(code == 1 || code == 2, "unexpected exit code: {}", code);
    }
}

#[test]
fn e2e_version_check_json() {
    let _log = common::test_log("e2e_version_check_json");
    let workspace = BrWorkspace::new();

    let version = run_br(
        &workspace,
        ["version", "--check", "--json"],
        "version_check_json",
    );

    let payload = extract_json_payload(&version.stdout);
    let json: Value = serde_json::from_str(&payload).expect("valid JSON");

    assert!(json.get("current").is_some(), "missing current field");

    if let Some(error) = json.get("error") {
        assert!(
            !error.as_str().unwrap_or("").is_empty(),
            "error field should be non-empty"
        );
    } else {
        assert!(json.get("latest").is_some(), "missing latest field");
        assert!(
            json.get("update_available").is_some(),
            "missing update_available field"
        );
    }
}
