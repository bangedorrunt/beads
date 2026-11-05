//! Version command implementation.

use super::github_latest_release_api_url;
use crate::cli::VersionArgs;
use crate::error::{BeadsError, Result};
use crate::output::{OutputContext, OutputMode};
use rich_rust::prelude::*;
use serde::Serialize;
use std::fmt::Write as _;

#[derive(Serialize)]
struct VersionOutput<'a> {
    version: &'a str,
    build: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    commit: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    branch: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rust_version: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    target: Option<&'a str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    features: Vec<&'a str>,
}

/// Execute the version command.
///
/// # Errors
///
/// Returns an error if JSON serialization fails or update check fails.
pub fn execute(args: &VersionArgs, ctx: &OutputContext) -> Result<()> {
    let version = env!("CARGO_PKG_VERSION");

    if args.short {
        if !ctx.is_quiet() {
            println!("{version}");
        }
        return Ok(());
    }

    if args.check {
        execute_update_check(version, ctx);
        return Ok(());
    }

    let build = if cfg!(debug_assertions) {
        "dev"
    } else {
        "release"
    };

    let commit = option_env!("VERGEN_GIT_SHA").filter(|s| !s.trim().is_empty());
    let branch = option_env!("VERGEN_GIT_BRANCH").filter(|s| !s.trim().is_empty());
    let rust_version = option_env!("VERGEN_RUSTC_SEMVER").filter(|s| !s.trim().is_empty());
    let target = option_env!("VERGEN_CARGO_TARGET_TRIPLE").filter(|s| !s.trim().is_empty());

    let mut features = Vec::new();
    if cfg!(feature = "self_update") {
        features.push("self_update");
    }

    if ctx.is_toon() {
        let output = VersionOutput {
            version,
            build,
            commit,
            branch,
            rust_version,
            target,
            features,
        };
        ctx.toon(&output);
        return Ok(());
    }

    if ctx.is_json() {
        let output = VersionOutput {
            version,
            build,
            commit,
            branch,
            rust_version,
            target,
            features,
        };
        ctx.json(&output);
        return Ok(());
    }

    if ctx.is_quiet() {
        return Ok(());
    }

    if matches!(ctx.mode(), OutputMode::Rich) {
        render_version_rich(
            version,
            build,
            commit,
            branch,
            rust_version,
            target,
            &features,
            ctx,
        );
        return Ok(());
    }

    let mut line = format!("br version {version} ({build})");
    match (branch, commit) {
        (Some(branch), Some(commit)) => {
            let short = &commit[..commit.len().min(7)];
            let _ = write!(line, " ({branch}@{short})");
        }
        (Some(branch), None) => {
            let _ = write!(line, " ({branch})");
        }
        (None, Some(commit)) => {
            let short = &commit[..commit.len().min(7)];
            let _ = write!(line, " ({short})");
        }
        (None, None) => {}
    }

    println!("{line}");
    Ok(())
}

/// Render version information with rich formatting.
#[allow(clippy::too_many_arguments)]
fn render_version_rich(
    version: &str,
    build: &str,
    commit: Option<&str>,
    branch: Option<&str>,
    rust_version: Option<&str>,
    target: Option<&str>,
    features: &[&str],
    ctx: &OutputContext,
) {
    let console = Console::default();
    let theme = ctx.theme();
    let width = ctx.width();

    let mut content = Text::new("");

    content.append_styled(&format!("br {version}"), theme.emphasis.clone());
    content.append_styled(&format!(" ({build})"), theme.dimmed.clone());
    content.append("\n\n");

    let has_build_info =
        commit.is_some() || branch.is_some() || rust_version.is_some() || target.is_some();

    if has_build_info {
        content.append_styled("Build Info:\n", theme.section.clone());

        let mut info_items: Vec<(&str, String)> = Vec::new();

        if let Some(commit) = commit {
            let short = &commit[..commit.len().min(7)];
            info_items.push(("Commit", short.to_string()));
        }
        if let Some(branch) = branch {
            info_items.push(("Branch", branch.to_string()));
        }
        if let Some(rust_ver) = rust_version {
            info_items.push(("Rust", rust_ver.to_string()));
        }
        if let Some(tgt) = target {
            info_items.push(("Target", tgt.to_string()));
        }

        let last_idx = info_items.len().saturating_sub(1);
        for (idx, (label, value)) in info_items.iter().enumerate() {
            let prefix = if idx == last_idx {
                "└── "
            } else {
                "├── "
            };
            content.append_styled(prefix, theme.dimmed.clone());
            content.append_styled(&format!("{:<8}", label), theme.accent.clone());
            content.append(&format!("{value}\n"));
        }
        content.append("\n");
    }

    if !features.is_empty() {
        content.append_styled("Features: ", theme.section.clone());
        content.append_styled(&features.join(", "), theme.success.clone());
        content.append("\n");
    }

    let panel = Panel::from_rich_text(&content, width)
        .title(Text::styled("br version", theme.panel_title.clone()))
        .box_style(theme.box_style);

    console.print_renderable(&panel);
}

/// Check for updates and exit with appropriate code.
///
/// Exit codes:
/// - 0: Up-to-date
/// - 1: Update available
/// - 2: Error checking for updates
fn execute_update_check(current_version: &str, ctx: &OutputContext) {
    let latest = match fetch_latest_version() {
        Ok(v) => v,
        Err(e) => {
            if ctx.is_toon() {
                ctx.toon(&serde_json::json!({
                    "current": current_version,
                    "latest": null,
                    "update_available": null,
                    "error": e.to_string()
                }));
            } else if ctx.is_json() {
                ctx.json(&serde_json::json!({
                    "current": current_version,
                    "latest": null,
                    "update_available": null,
                    "error": e.to_string()
                }));
            } else if !ctx.is_quiet() {
                eprintln!("Error checking for updates: {e}");
            }
            crate::shutdown::exit_process(2);
        }
    };

    let current = semver::Version::parse(current_version).ok();
    let latest_ver = semver::Version::parse(&latest).ok();

    let update_available = match (&current, &latest_ver) {
        (Some(c), Some(l)) => l > c,
        _ => false,
    };

    if ctx.is_toon() {
        ctx.toon(&serde_json::json!({
            "current": current_version,
            "latest": latest,
            "update_available": update_available
        }));
    } else if ctx.is_json() {
        ctx.json(&serde_json::json!({
            "current": current_version,
            "latest": latest,
            "update_available": update_available
        }));
    } else if ctx.is_quiet() {
    } else if update_available {
        println!("Update available: {current_version} → {latest}");
        println!("Run `br upgrade` to update.");
    } else {
        println!("br {current_version} is up to date (latest: {latest})");
    }

    if update_available {
        crate::shutdown::exit_process(1);
    }
}

/// Fetch the latest release version from GitHub.
fn fetch_latest_version() -> Result<String> {
    use std::io::Read;

    let url = github_latest_release_api_url();

    let mut handle = std::process::Command::new("curl")
        .args(["-sS", "-H", "User-Agent: br-cli", url.as_str()])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| BeadsError::external_command("curl", format!("Failed to spawn curl: {e}")))?;

    let mut output = String::new();
    if let Some(ref mut stdout) = handle.stdout {
        stdout.read_to_string(&mut output)?;
    }

    let status = handle.wait()?;
    if !status.success() {
        return Err(BeadsError::external_command(
            "curl",
            format!("curl failed with status {status}"),
        ));
    }

    let json: serde_json::Value = serde_json::from_str(&output)?;

    let tag = json
        .get("tag_name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| BeadsError::validation("GitHub response", "missing tag_name"))?;

    let version = tag.strip_prefix('v').unwrap_or(tag);
    Ok(version.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_json_schema() {
        let output = VersionOutput {
            version: "1.0.0",
            build: "release",
            commit: Some("abc1234"),
            branch: Some("main"),
            rust_version: Some("1.85.0"),
            target: Some("x86_64-unknown-linux-gnu"),
            features: vec!["self_update"],
        };

        let json = serde_json::to_value(&output).unwrap();
        assert_eq!(json["version"], "1.0.0");
        assert_eq!(json["build"], "release");
        assert_eq!(json["commit"], "abc1234");
        assert_eq!(json["branch"], "main");
        assert_eq!(json["rust_version"], "1.85.0");
        assert_eq!(json["target"], "x86_64-unknown-linux-gnu");
        assert_eq!(json["features"], serde_json::json!(["self_update"]));
    }

    #[test]
    fn test_version_json_omits_none_fields() {
        let output = VersionOutput {
            version: "1.0.0",
            build: "dev",
            commit: None,
            branch: None,
            rust_version: None,
            target: None,
            features: vec![],
        };

        let json = serde_json::to_value(&output).unwrap();
        assert!(json.get("commit").is_none());
        assert!(json.get("branch").is_none());
        assert!(json.get("rust_version").is_none());
        assert!(json.get("target").is_none());
        assert!(json.get("features").is_none());
    }

    #[test]
    fn test_build_info_present() {
        let version = env!("CARGO_PKG_VERSION");
        assert!(!version.is_empty());

        let commit = option_env!("VERGEN_GIT_SHA");
        let branch = option_env!("VERGEN_GIT_BRANCH");

        if let Some(c) = commit {
            assert!(!c.trim().is_empty() || c.is_empty());
        }
        if let Some(b) = branch {
            assert!(!b.trim().is_empty() || b.is_empty());
        }
    }

    #[test]
    fn test_feature_flags_detection() {
        let mut features = Vec::new();
        if cfg!(feature = "self_update") {
            features.push("self_update");
        }

        #[cfg(feature = "self_update")]
        assert!(features.contains(&"self_update"));

        #[cfg(not(feature = "self_update"))]
        assert!(features.is_empty());
    }

    #[test]
    fn test_version_short_format() {
        let version = env!("CARGO_PKG_VERSION");
        assert!(
            version.contains('.'),
            "Version should contain dots: {version}"
        );
        assert!(
            version.split('.').count() >= 2,
            "Version should have at least major.minor"
        );
    }

    #[test]
    fn latest_release_api_url_uses_shared_repo_constants() {
        assert_eq!(
            github_latest_release_api_url(),
            format!(
                "https://api.github.com/repos/{}/{}/releases/latest",
                crate::cli::commands::GITHUB_REPO_OWNER,
                crate::cli::commands::GITHUB_REPO_NAME
            )
        );
    }
}
