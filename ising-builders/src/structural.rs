//! Layer 1 — Structural Graph Builder
//!
//! Uses Tree-sitter to parse source files and extract:
//! - Module nodes (one per file)
//! - Function and class nodes (with line ranges)
//! - Import edges between modules
//! - Contains edges (module → function/class)
//!
//! Per-language extraction is delegated to the `languages` module.
//! Parsing is parallelized with rayon.

use crate::common::Language;
use crate::languages::{self, FileAnalysis};
use ising_core::graph::{EdgeType, Node, UnifiedGraph};
use ising_core::ignore::IgnoreRules;
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Build the structural graph for all supported source files in a directory.
pub fn build_structural_graph(
    repo_path: &Path,
    ignore: &IgnoreRules,
) -> Result<UnifiedGraph, anyhow::Error> {
    let source_files = walk_source_files(repo_path, ignore);

    let file_results: Vec<FileAnalysis> = source_files
        .par_iter()
        .filter_map(|(path, lang)| analyze_file(repo_path, path, *lang).ok())
        .collect();

    let mut graph = UnifiedGraph::new();

    for result in &file_results {
        // Add module node
        let mut module_node = Node::module(&result.module_id, &result.file_path);
        module_node.language = Some(result.language.clone());
        module_node.loc = Some(result.loc);
        graph.add_node(module_node);

        // Add function nodes + contains edges, track total complexity for module
        let mut module_complexity: u32 = 0;

        for func in &result.functions {
            let func_id = format!("{}::{}", result.module_id, func.name);
            let mut func_node =
                Node::function(&func_id, &result.file_path, func.line_start, func.line_end);
            func_node.language = Some(result.language.clone());
            func_node.complexity = Some(func.complexity);
            module_complexity += func.complexity;
            graph.add_node(func_node);
            let _ = graph.add_edge(&result.module_id, &func_id, EdgeType::Contains, 1.0);
        }

        // Add class nodes + contains edges
        for class in &result.classes {
            let class_id = format!("{}::{}", result.module_id, class.name);
            let mut class_node = Node::class(
                &class_id,
                &result.file_path,
                class.line_start,
                class.line_end,
            );
            class_node.language = Some(result.language.clone());
            class_node.complexity = Some(class.complexity);
            module_complexity += class.complexity;
            graph.add_node(class_node);
            let _ = graph.add_edge(&result.module_id, &class_id, EdgeType::Contains, 1.0);
        }

        // Set module-level complexity as sum of all function/class complexities
        if module_complexity > 0
            && let Some(module_node) = graph.get_node_mut(&result.module_id)
        {
            module_node.complexity = Some(module_complexity);
        }
    }

    // Resolve import edges between modules
    let module_ids: std::collections::HashSet<&str> =
        file_results.iter().map(|r| r.module_id.as_str()).collect();

    for result in &file_results {
        for imp in &result.imports {
            if module_ids.contains(imp.source.as_str()) {
                let _ = graph.add_edge(&result.module_id, &imp.source, EdgeType::Imports, 1.0);
            } else if imp.source.ends_with(".py") {
                // Try package resolution: foo/bar.py -> foo/bar/__init__.py
                let package_init = imp.source.trim_end_matches(".py").to_string() + "/__init__.py";
                if module_ids.contains(package_init.as_str()) {
                    let _ =
                        graph.add_edge(&result.module_id, &package_init, EdgeType::Imports, 1.0);
                }
            }
        }
    }

    Ok(graph)
}

/// Walk the repository and collect all supported source files with their language.
fn walk_source_files(repo_path: &Path, ignore: &IgnoreRules) -> Vec<(PathBuf, Language)> {
    WalkDir::new(repo_path)
        .into_iter()
        .filter_entry(|e| {
            // Always allow the root entry
            if e.depth() == 0 {
                return true;
            }
            let name = e.file_name().to_string_lossy();
            // Skip hidden dirs, node_modules, __pycache__, target, .git
            if e.file_type().is_dir() {
                return !name.starts_with('.')
                    && name != "node_modules"
                    && name != "__pycache__"
                    && name != "target"
                    && name != "dist"
                    && name != "build";
            }
            true
        })
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| {
            let ext = e.path().extension()?.to_str()?;
            let lang = Language::from_extension(ext)?;
            let rel = e.path().strip_prefix(repo_path).ok()?;
            let rel_str = rel.to_string_lossy();
            if ignore.is_ignored(&rel_str) {
                return None;
            }
            Some((e.into_path(), lang))
        })
        .collect()
}

/// Analyze a single source file using Tree-sitter.
fn analyze_file(
    repo_path: &Path,
    file_path: &Path,
    lang: Language,
) -> Result<FileAnalysis, anyhow::Error> {
    let source = std::fs::read_to_string(file_path)?;
    let relative_path = file_path
        .strip_prefix(repo_path)
        .unwrap_or(file_path)
        .to_string_lossy()
        .to_string();

    let loc = source.lines().filter(|l| !l.trim().is_empty()).count() as u32;

    let mut functions = Vec::new();
    let mut classes = Vec::new();
    let mut imports = Vec::new();

    let mut parser = tree_sitter::Parser::new();
    let tree_sitter_lang = get_tree_sitter_language(lang, file_path);

    if let Some(ts_lang) = tree_sitter_lang {
        parser.set_language(&ts_lang)?;
        if let Some(tree) = parser.parse(&source, None) {
            let root = tree.root_node();
            match lang {
                Language::Python => {
                    languages::python::extract_nodes(
                        root,
                        &source,
                        &relative_path,
                        &mut functions,
                        &mut classes,
                        &mut imports,
                    );
                }
                Language::TypeScript | Language::JavaScript => {
                    languages::typescript::extract_nodes(
                        root,
                        &source,
                        &mut functions,
                        &mut classes,
                        &mut imports,
                    );
                }
                Language::Rust => {
                    languages::rust_lang::extract_nodes(
                        root,
                        &source,
                        &relative_path,
                        &mut functions,
                        &mut classes,
                        &mut imports,
                    );
                }
            }
        }
    } else {
        tracing::debug!(
            "No tree-sitter grammar for {}, using basic analysis",
            lang.name()
        );
    }

    Ok(FileAnalysis {
        module_id: relative_path.clone(),
        file_path: relative_path,
        language: lang.name().to_string(),
        loc,
        functions,
        classes,
        imports,
    })
}

/// Get the appropriate tree-sitter language grammar for a file.
fn get_tree_sitter_language(lang: Language, file_path: &Path) -> Option<tree_sitter::Language> {
    match lang {
        Language::Python => Some(tree_sitter_python::LANGUAGE.into()),
        Language::TypeScript => {
            let ext = file_path.extension()?.to_str()?;
            if ext == "tsx" {
                Some(tree_sitter_typescript::LANGUAGE_TSX.into())
            } else {
                Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
            }
        }
        Language::JavaScript => {
            let ext = file_path.extension()?.to_str()?;
            if ext == "jsx" {
                Some(tree_sitter_typescript::LANGUAGE_TSX.into())
            } else {
                Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
            }
        }
        Language::Rust => Some(tree_sitter_rust::LANGUAGE.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_walk_source_files() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("main.py"), "print('hello')").unwrap();
        fs::write(dir.path().join("app.ts"), "console.log('hi')").unwrap();
        fs::write(dir.path().join("readme.md"), "# hello").unwrap();

        let files = walk_source_files(dir.path(), &IgnoreRules::parse(""));
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn test_walk_skips_hidden_and_node_modules() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".git")).unwrap();
        fs::write(dir.path().join(".git/config.py"), "x").unwrap();
        fs::create_dir_all(dir.path().join("node_modules/foo")).unwrap();
        fs::write(dir.path().join("node_modules/foo/index.js"), "x").unwrap();
        fs::write(dir.path().join("app.py"), "x").unwrap();

        let files = walk_source_files(dir.path(), &IgnoreRules::parse(""));
        assert_eq!(files.len(), 1);
    }

    #[test]
    fn test_build_python_structural_graph() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("main.py"),
            r#"
def hello():
    pass

def world():
    pass

class MyClass:
    def method(self):
        pass
"#,
        )
        .unwrap();
        fs::write(
            dir.path().join("utils.py"),
            r#"
def helper():
    pass
"#,
        )
        .unwrap();

        let graph = build_structural_graph(dir.path(), &IgnoreRules::parse("")).unwrap();
        assert!(
            graph.node_count() >= 5,
            "Expected >= 5 nodes, got {}",
            graph.node_count()
        );
        assert!(
            graph.edge_count() >= 3,
            "Expected >= 3 contains edges, got {}",
            graph.edge_count()
        );
    }

    #[test]
    fn test_build_typescript_structural_graph() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("app.ts"),
            r#"
function greet(name: string): string {
    return `Hello, ${name}!`;
}

class AppService {
    run() {}
}
"#,
        )
        .unwrap();

        let graph = build_structural_graph(dir.path(), &IgnoreRules::parse("")).unwrap();
        assert!(
            graph.node_count() >= 3,
            "Expected >= 3 nodes, got {}",
            graph.node_count()
        );
    }

    #[test]
    fn test_python_imports_resolved() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(
            dir.path().join("main.py"),
            "from utils import helper\n\ndef main():\n    pass\n",
        )
        .unwrap();
        fs::write(dir.path().join("utils.py"), "def helper():\n    pass\n").unwrap();

        let graph = build_structural_graph(dir.path(), &IgnoreRules::parse("")).unwrap();
        let _import_edges = graph.edges_of_type(&ising_core::graph::EdgeType::Imports);
        assert!(
            graph.node_count() >= 2,
            "Expected >= 2 nodes, got {}",
            graph.node_count()
        );
    }

    #[test]
    fn test_walk_source_files_includes_rust() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("main.rs"), "fn main() {}").unwrap();
        fs::write(dir.path().join("app.py"), "pass").unwrap();
        fs::write(dir.path().join("readme.md"), "# hello").unwrap();

        let files = walk_source_files(dir.path(), &IgnoreRules::parse(""));
        assert_eq!(files.len(), 2);
        let rust_files: Vec<_> = files.iter().filter(|(_, l)| *l == Language::Rust).collect();
        assert_eq!(rust_files.len(), 1);
    }

    #[test]
    fn test_rust_function_extraction() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("lib.rs"),
            r#"
fn hello() {
    println!("hello");
}

fn world() -> i32 {
    42
}
"#,
        )
        .unwrap();

        let graph = build_structural_graph(dir.path(), &IgnoreRules::parse("")).unwrap();
        assert!(
            graph.node_count() >= 3,
            "Expected >= 3 nodes, got {}",
            graph.node_count()
        );
        assert!(
            graph.edge_count() >= 2,
            "Expected >= 2 edges, got {}",
            graph.edge_count()
        );
    }

    #[test]
    fn test_rust_struct_enum_trait_extraction() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("types.rs"),
            r#"
struct MyStruct {
    field: i32,
}

enum MyEnum {
    A,
    B,
}

trait MyTrait {
    fn do_thing(&self);
}
"#,
        )
        .unwrap();

        let graph = build_structural_graph(dir.path(), &IgnoreRules::parse("")).unwrap();
        assert!(
            graph.node_count() >= 4,
            "Expected >= 4 nodes, got {}",
            graph.node_count()
        );
    }

    #[test]
    fn test_rust_impl_method_attribution() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("service.rs"),
            r#"
struct MyStruct;

impl MyStruct {
    fn new() -> Self {
        MyStruct
    }

    fn method(&self) -> i32 {
        42
    }
}
"#,
        )
        .unwrap();

        let graph = build_structural_graph(dir.path(), &IgnoreRules::parse("")).unwrap();
        assert!(
            graph.node_count() >= 4,
            "Expected >= 4 nodes, got {}",
            graph.node_count()
        );
        assert!(
            graph.get_node("service.rs::MyStruct::new").is_some(),
            "Expected node service.rs::MyStruct::new"
        );
        assert!(
            graph.get_node("service.rs::MyStruct::method").is_some(),
            "Expected node service.rs::MyStruct::method"
        );
    }

    #[test]
    fn test_rust_mod_import_resolution() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/lib.rs"), "mod foo;\n\nfn main() {}\n").unwrap();
        fs::write(dir.path().join("src/foo.rs"), "pub fn helper() {}\n").unwrap();

        let graph = build_structural_graph(dir.path(), &IgnoreRules::parse("")).unwrap();
        let import_edges = graph.edges_of_type(&ising_core::graph::EdgeType::Imports);
        assert!(
            !import_edges.is_empty(),
            "Expected at least one import edge for mod foo"
        );
    }

    #[test]
    fn test_rust_use_crate_import_resolution() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("src/bar")).unwrap();
        fs::write(
            dir.path().join("src/main.rs"),
            "use crate::bar::baz;\n\nfn main() {}\n",
        )
        .unwrap();
        fs::write(dir.path().join("src/bar/baz.rs"), "pub fn helper() {}\n").unwrap();

        let graph = build_structural_graph(dir.path(), &IgnoreRules::parse("")).unwrap();
        let import_edges = graph.edges_of_type(&ising_core::graph::EdgeType::Imports);
        assert!(
            !import_edges.is_empty(),
            "Expected at least one import edge for use crate::bar::baz"
        );
    }

    #[test]
    fn test_rust_external_use_ignored() {
        let result =
            languages::rust_lang::resolve_use_import("use std::collections::HashMap;");
        assert!(result.is_none(), "External crate imports should be ignored");

        let result = languages::rust_lang::resolve_use_import("use serde::Serialize;");
        assert!(result.is_none(), "External crate imports should be ignored");
    }

    #[test]
    fn test_rust_complexity() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("complex.rs"),
            r#"
fn complex_function(x: Option<i32>) -> i32 {
    if let Some(val) = x {
        match val {
            1 => 10,
            2 => 20,
            _ => 30,
        }
    } else {
        0
    }
}
"#,
        )
        .unwrap();

        let graph = build_structural_graph(dir.path(), &IgnoreRules::parse("")).unwrap();
        let func_node = graph.get_node("complex.rs::complex_function");
        assert!(func_node.is_some(), "Expected complex_function node");
        let complexity = func_node.unwrap().complexity.unwrap_or(0);
        // 1 base + 1 if_let + 3 match arms = 5
        assert_eq!(complexity, 5, "Expected complexity 5, got {}", complexity);
    }

    #[test]
    fn test_rust_mod_resolution_paths() {
        // src/lib.rs with mod foo → src/foo.rs or src/foo/mod.rs
        let paths = languages::rust_lang::resolve_mod_import("foo", "src/lib.rs");
        assert!(paths.contains(&"src/foo.rs".to_string()));
        assert!(paths.contains(&"src/foo/mod.rs".to_string()));

        // src/bar/mod.rs with mod baz → src/bar/baz.rs or src/bar/baz/mod.rs
        let paths = languages::rust_lang::resolve_mod_import("baz", "src/bar/mod.rs");
        assert!(paths.contains(&"src/bar/baz.rs".to_string()));
        assert!(paths.contains(&"src/bar/baz/mod.rs".to_string()));
    }
}
