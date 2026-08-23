// governed-by: ADR-0003
//! Robot envelope contract + capabilities manifest + argv0 bv compat mode
//! (beads_rust-gu7ts.5; ADR-0003 §3.1 envelope contract, §3.4 bv
//! compatibility, §5 proof items 1 and 5).
//!
//! RED until gu7ts.5 lands:
//! - argv0 compat (`argv[0] == "bv"` maps `--robot-X` flags 1:1)
//! - unknown `--robot-X` fails closed with `{"error", "not_ported": true}`
//!   and exit 2 (clap's default unknown-flag path emits help text instead)
//! - `br capabilities --json` carries the machine-readable `bv_compat`
//!   section declaring the flag mapping

use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

mod common;

use common::cli::BrWorkspace;

const FIXTURE_ISSUES: &str = include_str!("fixtures/bv_parity/fixture_issues.jsonl");

/// Fields that legitimately vary between runs (ADR-0003 §5 proof 1).
const VOLATILE_FIELDS: &[&str] = &["generated_at", "data_hash", "version", "ms"];

/// Every robot body must carry these envelope keys (ADR-0003 §3.1).
const ENVELOPE_REQUIRED_KEYS: &[&str] =
    &["generated_at", "data_hash", "analysis_config", "status", "usage_hints"];

fn strip_volatile(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(key, _)| !VOLATILE_FIELDS.contains(&key.as_str()))
                .map(|(key, inner)| (key.clone(), strip_volatile(inner)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(strip_volatile).collect()),
        other => other.clone(),
    }
}

fn workspace_with_fixture() -> BrWorkspace {
    let workspace = BrWorkspace::new();
    let beads_dir = workspace.root.join(".beads");
    std::fs::create_dir_all(&beads_dir).expect("create .beads dir");
    // Dangling dep refs are stripped: see beads_rust-svtxe and the
    // plan/insights parity test for why replaying them diverges from bv.
    let stripped: Vec<String> = FIXTURE_ISSUES
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let mut issue: Value = serde_json::from_str(line).expect("fixture line parses");
            issue
                .as_object_mut()
                .and_then(|map| map.remove("dependencies"));
            issue.to_string()
        })
        .collect();
    std::fs::write(beads_dir.join("issues.jsonl"), stripped.join("\n"))
        .expect("write fixture issues.jsonl");
    let import = Command::new(env!("CARGO_BIN_EXE_br"))
        .current_dir(&workspace.root)
        .args(["sync", "--import-only", "--force"])
        .output()
        .expect("run br sync --import-only");
    assert!(
        import.status.success(),
        "fixture import failed: {}",
        String::from_utf8_lossy(&import.stderr)
    );
    workspace
}

fn run_in(workspace: &BrWorkspace, program: &std::ffi::OsStr, args: &[&str]) -> std::process::Output {
    Command::new(program)
        .current_dir(&workspace.root)
        .args(args)
        .env("NO_COLOR", "1")
        .output()
        .expect("spawn br")
}

/// Copy the compiled binary under an alternate file stem so the process can
/// be launched with a chosen argv[0] without touching the real binary.
fn copied_binary(workspace: &BrWorkspace, name: &str) -> PathBuf {
    let target = workspace.root.join(name);
    std::fs::copy(env!("CARGO_BIN_EXE_br"), &target).expect("copy binary");
    target
}

#[test]
fn argv0_bv_robot_flags_map_to_br_subcommands_with_identical_output() {
    let workspace = workspace_with_fixture();
    let bv = copied_binary(&workspace, "bv");

    let as_bv = run_in(&workspace, bv.as_os_str(), &["--robot-plan"]);
    assert!(
        as_bv.status.success(),
        "`bv --robot-plan` failed: {}",
        String::from_utf8_lossy(&as_bv.stderr)
    );

    let as_br = run_in(&workspace, env!("CARGO_BIN_EXE_br").as_ref(), &["plan", "--json"]);
    assert!(as_br.status.success(), "`br plan --json` failed");

    let from_bv: Value =
        serde_json::from_slice(&as_bv.stdout).expect("`bv --robot-plan` must emit JSON");
    let from_br: Value =
        serde_json::from_slice(&as_br.stdout).expect("`br plan --json` must emit JSON");
    assert_eq!(
        strip_volatile(&from_bv),
        strip_volatile(&from_br),
        "argv0 bv compat must produce identical shapes to the br subcommand"
    );
}

#[test]
fn unknown_robot_flag_fails_closed_with_not_ported_error() {
    let workspace = BrWorkspace::new();
    let output = run_in(&workspace, env!("CARGO_BIN_EXE_br").as_ref(), &[
        "--robot-teleport",
    ]);

    assert_eq!(
        output.status.code(),
        Some(2),
        "unknown --robot-* flags must exit 2"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    let parsed: Value = serde_json::from_str(stderr.trim())
        .unwrap_or_else(|error| panic!("unknown robot flag must emit JSON on stderr: {error}\n{stderr}"));
    assert_eq!(
        parsed.get("not_ported").and_then(Value::as_bool),
        Some(true),
        "the error payload must carry not_ported: true"
    );
    assert!(
        parsed.get("error").is_some(),
        "the error payload must describe the unported flag"
    );
}

#[test]
fn capabilities_manifest_declares_bv_compat_mapping() {
    let workspace = BrWorkspace::new();
    let output = run_in(&workspace, env!("CARGO_BIN_EXE_br").as_ref(), &[
        "capabilities",
        "--format",
        "json",
    ]);
    assert!(output.status.success(), "capabilities failed");
    let manifest: Value = serde_json::from_slice(&output.stdout).expect("JSON manifest");

    let bv_compat = manifest
        .get("bv_compat")
        .unwrap_or_else(|| panic!("manifest must declare a bv_compat section"));
    assert_eq!(
        bv_compat.get("contract_version").and_then(Value::as_str),
        Some("1"),
        "bv_compat declares the frozen robot contract version"
    );

    let mapping = bv_compat
        .get("flag_map")
        .and_then(Value::as_object)
        .expect("bv_compat.flag_map object");
    for robot_flag in ["--robot-triage", "--robot-next", "--robot-plan", "--robot-insights"] {
        assert!(
            mapping.contains_key(robot_flag),
            "{robot_flag} must map to a br subcommand"
        );
    }
}

#[test]
fn every_robot_body_carries_the_envelope_contract_keys() {
    let workspace = workspace_with_fixture();
    for args in [["plan"], ["insights"]] {
        let mut argv = vec![args[0]];
        argv.push("--json");
        let output = run_in(&workspace, env!("CARGO_BIN_EXE_br").as_ref(), &argv);
        assert!(output.status.success(), "`br {}` failed", args.join(" "));
        let body: Value = serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|error| panic!("`br {}` stdout must be JSON: {error}", args.join(" ")));
        for key in ENVELOPE_REQUIRED_KEYS {
            assert!(
                body.get(*key).is_some(),
                "`br {}` envelope is missing `{key}` (ADR-0003 §3.1)",
                args.join(" ")
            );
        }
    }
}
