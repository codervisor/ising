//! Integration tests for the graph builders.
//!
//! These tests create temporary git repositories with real commits to verify
//! the structural and change graph builders work end-to-end.

use ising_core::config::Config;
use ising_core::graph::EdgeType;
use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn git(dir: &std::path::Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("git command failed");
    assert!(status.success(), "git {:?} failed", args);
}

fn create_test_repo() -> TempDir {
    let dir = TempDir::new().unwrap();
    git(dir.path(), &["init"]);
    git(dir.path(), &["config", "user.email", "test@test.com"]);
    git(dir.path(), &["config", "user.name", "Test"]);
    git(dir.path(), &["config", "commit.gpgsign", "false"]);
    dir
}

#[test]
fn test_structural_graph_with_python_files() {
    let dir = create_test_repo();

    fs::write(
        dir.path().join("main.py"),
        r#"
from utils import helper

def main():
    result = helper()
    print(result)

class Application:
    def run(self):
        main()
"#,
    )
    .unwrap();

    fs::write(
        dir.path().join("utils.py"),
        r#"
def helper():
    return 42

def unused():
    pass
"#,
    )
    .unwrap();

    let graph = ising_builders::structural::build_structural_graph(dir.path(), &ising_core::ignore::IgnoreRules::parse("")).unwrap();

    // Should have module nodes for both files
    assert!(graph.get_node("main.py").is_some(), "main.py module node missing");
    assert!(graph.get_node("utils.py").is_some(), "utils.py module node missing");

    // Should have function nodes
    assert!(graph.get_node("main.py::main").is_some(), "main function missing");
    assert!(graph.get_node("main.py::Application").is_some(), "Application class missing");
    assert!(graph.get_node("utils.py::helper").is_some(), "helper function missing");
    assert!(graph.get_node("utils.py::unused").is_some(), "unused function missing");

    // Should have contains edges
    let contains = graph.edges_of_type(&EdgeType::Contains);
    assert!(contains.len() >= 4, "Expected >= 4 contains edges, got {}", contains.len());

    // Should have import edge from main.py -> utils.py
    let imports = graph.edges_of_type(&EdgeType::Imports);
    assert!(
        imports.iter().any(|(src, tgt, _)| *src == "main.py" && *tgt == "utils.py"),
        "Expected import edge main.py -> utils.py, got: {:?}",
        imports
    );
}

#[test]
fn test_structural_graph_with_typescript_files() {
    let dir = create_test_repo();

    fs::write(
        dir.path().join("app.ts"),
        r#"
function greet(name: string): string {
    return `Hello, ${name}!`;
}

class UserService {
    getUser(id: number) {
        return { id, name: "test" };
    }
}

function main() {
    const svc = new UserService();
    console.log(greet("world"));
}
"#,
    )
    .unwrap();

    let graph = ising_builders::structural::build_structural_graph(dir.path(), &ising_core::ignore::IgnoreRules::parse("")).unwrap();

    assert!(graph.get_node("app.ts").is_some(), "app.ts module node missing");
    assert!(graph.get_node("app.ts::greet").is_some(), "greet function missing");
    assert!(graph.get_node("app.ts::UserService").is_some(), "UserService class missing");
    assert!(graph.get_node("app.ts::main").is_some(), "main function missing");
}

#[test]
fn test_change_graph_with_git_history() {
    let dir = create_test_repo();

    // Create initial files and commit
    fs::write(dir.path().join("a.py"), "x = 1\n").unwrap();
    fs::write(dir.path().join("b.py"), "y = 2\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "initial"]);

    // Commit 2: change both a.py and b.py together
    fs::write(dir.path().join("a.py"), "x = 10\n").unwrap();
    fs::write(dir.path().join("b.py"), "y = 20\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "update both"]);

    // Commit 3: change only a.py
    fs::write(dir.path().join("a.py"), "x = 100\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "update a only"]);

    let config = Config::default();
    let graph = ising_builders::change::build_change_graph(dir.path(), &config, &ising_core::ignore::IgnoreRules::parse("")).unwrap();

    // Should have nodes for both files
    assert!(graph.get_node("a.py").is_some(), "a.py missing from change graph");
    assert!(graph.get_node("b.py").is_some(), "b.py missing from change graph");

    // Check change frequencies
    let a_metrics = graph.change_metrics.get("a.py").expect("a.py change metrics missing");
    assert!(a_metrics.change_freq >= 2, "a.py should have freq >= 2, got {}", a_metrics.change_freq);

    let b_metrics = graph.change_metrics.get("b.py").expect("b.py change metrics missing");
    assert!(b_metrics.change_freq >= 1, "b.py should have freq >= 1, got {}", b_metrics.change_freq);
}

#[test]
fn test_full_build_pipeline() {
    let dir = create_test_repo();

    fs::write(
        dir.path().join("main.py"),
        "def main():\n    print('hello')\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("utils.py"),
        "def helper():\n    return 42\n",
    )
    .unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "initial"]);

    // Run the full build pipeline
    let config = Config::default();
    let graph = ising_builders::build_all(dir.path(), &config).unwrap();

    // Should have merged structural + change nodes
    assert!(graph.node_count() >= 2, "Expected >= 2 nodes, got {}", graph.node_count());

    // Verify it can be stored in the database
    let db = ising_db::Database::open_in_memory().unwrap();
    db.store_graph(&graph).unwrap();

    let stats = db.get_stats().unwrap();
    assert!(stats.node_count >= 2, "DB should have >= 2 nodes");
}
