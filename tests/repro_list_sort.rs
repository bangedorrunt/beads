mod common;

use assert_cmd::Command;
use tempfile::TempDir;

#[test]
fn test_list_sort_title_case_insensitive() {
    let temp =
        TempDir::new_in(common::cli::isolated_temp_root()).expect("create isolated temp dir");
    let path = temp.path();

    Command::new(assert_cmd::cargo::cargo_bin!("br"))
        .current_dir(path)
        .arg("init")
        .assert()
        .success();

    Command::new(assert_cmd::cargo::cargo_bin!("br"))
        .current_dir(path)
        .args(common::cli::gate_create_args(["create", "apple"]))
        .assert()
        .success();

    Command::new(assert_cmd::cargo::cargo_bin!("br"))
        .current_dir(path)
        .args(common::cli::gate_create_args(["create", "Banana"]))
        .assert()
        .success();

    let output = Command::new(assert_cmd::cargo::cargo_bin!("br"))
        .current_dir(path)
        .arg("list")
        .arg("--sort")
        .arg("title")
        .output()
        .expect("Failed to list issues");

    let stdout = String::from_utf8_lossy(&output.stdout);

    let banana_pos = stdout.find("Banana").expect("Banana not found");
    let apple_pos = stdout.find("apple").expect("apple not found");

    assert!(
        apple_pos < banana_pos,
        "Expected 'apple' before 'Banana' (case-insensitive sort), but got:\n{stdout}"
    );
}
