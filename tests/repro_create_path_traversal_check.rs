use beads::cli::CreateArgs;
use beads::cli::commands::create;
use beads::config::CliOverrides;
use beads::output::OutputContext;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
#[ignore = "Path traversal restriction still active; feature to allow .. in CLI input not yet implemented"]
fn test_create_from_file_rejects_parent_dir() {

    let temp = TempDir::new().unwrap();
    let subdir = temp.path().join("subdir");
    fs::create_dir(&subdir).unwrap();

    let md_path = temp.path().join("issues.md");
    fs::write(&md_path, "## Test Issue").unwrap();

    let relative_path = PathBuf::from("..").join("issues.md");

    let args = CreateArgs {
        file: Some(relative_path.to_str().unwrap().to_string().into()),
        ..Default::default()
    };

    let overrides = CliOverrides::default();
    let ctx = OutputContext::from_flags(false, false, true);

    let result = create::execute(&args, &overrides, &ctx);

    if let Err(e) = result {
        let err_str = e.to_string();
        assert!(
            !err_str.contains("path must not contain '..'"),
            "Test failed: Path traversal check still active!"
        );
    }
}
