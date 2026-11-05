#![allow(clippy::similar_names)]

mod common;

use common::cli::{BrWorkspace, run_br};

fn parse_created_id(stdout: &str) -> String {
    let line = stdout.lines().next().unwrap_or("");
    // Handle both formats: "Created bd-xxx: title" and "✓ Created bd-xxx: title"
    let normalized = line.strip_prefix("✓ ").unwrap_or(line);
    let id_part = normalized
        .strip_prefix("Created ")
        .and_then(|rest| rest.split(':').next())
        .unwrap_or("");
    id_part.trim().to_string()
}

#[test]
fn e2e_graph_dfs_ordering() {
    let _log = common::test_log("e2e_graph_dfs_ordering");
    let workspace = BrWorkspace::new();
    run_br(&workspace, ["init"], "init");

    let root = run_br(&workspace, ["create", "Root"], "root");
    let id_root = parse_created_id(&root.stdout);

    let a = run_br(&workspace, ["create", "A"], "a");
    let id_a = parse_created_id(&a.stdout);

    let b = run_br(&workspace, ["create", "B"], "b");
    let id_b = parse_created_id(&b.stdout);

    let c = run_br(&workspace, ["create", "C"], "c");
    let id_c = parse_created_id(&c.stdout);

    run_br(&workspace, ["dep", "add", &id_a, &id_root], "dep_a_root");
    run_br(&workspace, ["dep", "add", &id_b, &id_root], "dep_b_root");
    run_br(&workspace, ["dep", "add", &id_c, &id_a], "dep_c_a");

    let graph = run_br(&workspace, ["graph", &id_root], "graph");

    let output = graph.stdout;
    let lines: Vec<&str> = output.lines().filter(|l: &&str| l.contains("←")).collect();

    let line_a = lines
        .iter()
        .find(|l: &&&str| l.contains(&id_a))
        .expect("line A");
    let line_b = lines
        .iter()
        .find(|l: &&&str| l.contains(&id_b))
        .expect("line B");
    let line_c = lines
        .iter()
        .find(|l: &&&str| l.contains(&id_c))
        .expect("line C");

    let indent_a = line_a.chars().take_while(|c| *c == ' ').count();
    let indent_b = line_b.chars().take_while(|c| *c == ' ').count();
    let indent_c = line_c.chars().take_while(|c| *c == ' ').count();

    assert_eq!(indent_a, 4, "A should have depth 1 (indent 4)");
    assert_eq!(indent_b, 4, "B should have depth 1 (indent 4)");
    assert_eq!(indent_c, 6, "C should have depth 2 (indent 6)");

    let pos_a = output.find(&id_a).unwrap();
    let pos_b = output.find(&id_b).unwrap();
    let pos_c = output.find(&id_c).unwrap();

    if pos_a < pos_b {
        assert!(
            pos_c > pos_a && pos_c < pos_b,
            "C should assume position between A and B (DFS order). Got A={pos_a}, C={pos_c}, B={pos_b}"
        );
    } else {
        assert!(pos_c > pos_a, "C should be after A (DFS order)");
    }
}
