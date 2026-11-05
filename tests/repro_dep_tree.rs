mod common;
use common::cli::{BrWorkspace, run_br};

#[test]
fn test_dep_tree_diamond_dependency_visibility() {
    let workspace = BrWorkspace::new();

    run_br(&workspace, ["init"], "init");

    let id_a = run_br(&workspace, ["create", "A", "--silent"], "get_A")
        .stdout
        .trim()
        .to_string();
    let id_b = run_br(&workspace, ["create", "B", "--silent"], "get_B")
        .stdout
        .trim()
        .to_string();
    let id_c = run_br(&workspace, ["create", "C", "--silent"], "get_C")
        .stdout
        .trim()
        .to_string();
    let id_d = run_br(&workspace, ["create", "D", "--silent"], "get_D")
        .stdout
        .trim()
        .to_string();

    run_br(&workspace, ["dep", "add", &id_a, &id_b], "A->B");
    run_br(&workspace, ["dep", "add", &id_a, &id_c], "A->C");
    run_br(&workspace, ["dep", "add", &id_b, &id_d], "B->D");
    run_br(&workspace, ["dep", "add", &id_c, &id_d], "C->D");

    let tree = run_br(&workspace, ["dep", "tree", &id_a], "tree").stdout;
    println!("Tree Output:\n{tree}");

    assert_eq!(
        tree.match_indices(&id_d).count(),
        2,
        "Diamond dependency node D should appear twice in tree view"
    );
}
