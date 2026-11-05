mod common;
use common::cli::{BrWorkspace, parse_list_issues, run_br};

#[test]
fn test_ready_limit_with_external_blockers() {
    let workspace = BrWorkspace::new();
    run_br(&workspace, ["init"], "init");

    for i in 1..=10 {
        run_br(
            &workspace,
            [
                "create",
                &format!("Issue {i}"),
                "--verify",
                "cargo test --lib",
                "--principle",
                "testing — verify ready limit",
            ],
            &format!("create_{i}"),
        );
    }

    let list = run_br(&workspace, ["list", "--json"], "list");
    let issues = parse_list_issues(&list.stdout);

    let mut ids: Vec<String> = issues
        .iter()
        .map(|i| i["id"].as_str().unwrap().to_string())
        .collect();
    ids.sort();

    assert_eq!(ids.len(), 10);

    for (i, id) in ids.iter().take(5).enumerate() {
        run_br(
            &workspace,
            ["dep", "add", id.as_str(), "external:missing:dep"],
            &format!("block_{i}"),
        );
    }

    let ready = run_br(&workspace, ["ready", "--limit", "5", "--json"], "ready");
    let ready_page: serde_json::Value = serde_json::from_str(&ready.stdout).unwrap();
    let ready_issues = ready_page["issues"].as_array().cloned().unwrap_or_default();

    assert_eq!(
        ready_issues.len(),
        5,
        "Expected 5 ready issues, got {}",
        ready_issues.len()
    );

    for issue in ready_issues {
        let id = issue["id"].as_str().unwrap();
        assert!(
            !ids[0..5].contains(&id.to_string()),
            "Blocked issue {id} returned in ready list"
        );
    }
}
