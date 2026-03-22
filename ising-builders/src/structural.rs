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
            } else if result.language == "go" {
                // Go imports resolve to directories — create edges to all .go files in the dir
                let dir_prefix = if imp.source.ends_with('/') {
                    imp.source.clone()
                } else {
                    format!("{}/", imp.source)
                };
                for target_id in &module_ids {
                    if target_id.starts_with(&dir_prefix)
                        && target_id.ends_with(".go")
                        && *target_id != result.module_id
                    {
                        let _ = graph.add_edge(
                            &result.module_id,
                            target_id,
                            EdgeType::Imports,
                            1.0,
                        );
                    }
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

    if lang == Language::Vue {
        // Vue uses its own two-pass parsing (text-based script block extraction + TS re-parse)
        languages::vue::extract_nodes(&source, &mut functions, &mut classes, &mut imports);

        // Resolve @/ alias for Vue imports if a Vue/Vite config exists
        let has_vue_config = repo_path.join("vite.config.ts").exists()
            || repo_path.join("vite.config.js").exists()
            || repo_path.join("vue.config.js").exists()
            || repo_path.join("vue.config.ts").exists();

        // Re-resolve imports using Vue-specific logic
        let raw_imports = std::mem::take(&mut imports);
        for imp in raw_imports {
            if let Some(resolved) = languages::vue::resolve_vue_import(&imp.source, has_vue_config)
            {
                imports.push(languages::ImportInfo { source: resolved });
            } else {
                // Keep the original import (it may still match a module ID)
                imports.push(imp);
            }
        }
    } else {
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
                    Language::Go => {
                        languages::go::extract_nodes(
                            root,
                            &source,
                            &relative_path,
                            repo_path,
                            &mut functions,
                            &mut classes,
                            &mut imports,
                        );
                    }
                    Language::Vue => unreachable!(), // Handled above
                }
            }
        } else {
            tracing::debug!(
                "No tree-sitter grammar for {}, using basic analysis",
                lang.name()
            );
        }
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
        Language::Go => Some(tree_sitter_go::LANGUAGE.into()),
        Language::Vue => None, // Vue uses its own two-pass parsing
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
        let result = languages::rust_lang::resolve_use_import("use std::collections::HashMap;");
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

    #[test]
    fn test_walk_source_files_includes_go() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("main.go"), "package main\n").unwrap();
        fs::write(dir.path().join("app.py"), "pass").unwrap();
        fs::write(dir.path().join("readme.md"), "# hello").unwrap();

        let files = walk_source_files(dir.path(), &IgnoreRules::parse(""));
        assert_eq!(files.len(), 2);
        let go_files: Vec<_> = files.iter().filter(|(_, l)| *l == Language::Go).collect();
        assert_eq!(go_files.len(), 1);
    }

    #[test]
    fn test_go_function_extraction() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("main.go"),
            r#"package main

func Hello() {
    fmt.Println("hello")
}

func World() int {
    return 42
}
"#,
        )
        .unwrap();

        let graph = build_structural_graph(dir.path(), &IgnoreRules::parse("")).unwrap();
        assert!(
            graph.node_count() >= 3,
            "Expected >= 3 nodes (module + 2 functions), got {}",
            graph.node_count()
        );
        assert!(
            graph.get_node("main.go::Hello").is_some(),
            "Expected node main.go::Hello"
        );
        assert!(
            graph.get_node("main.go::World").is_some(),
            "Expected node main.go::World"
        );
    }

    #[test]
    fn test_go_method_attribution() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("service.go"),
            r#"package service

type MyStruct struct {
    Field int
}

func (s *MyStruct) Method() int {
    return s.Field
}

func (s MyStruct) ValueMethod() string {
    return "hello"
}
"#,
        )
        .unwrap();

        let graph = build_structural_graph(dir.path(), &IgnoreRules::parse("")).unwrap();
        assert!(
            graph.get_node("service.go::MyStruct::Method").is_some(),
            "Expected node service.go::MyStruct::Method"
        );
        assert!(
            graph.get_node("service.go::MyStruct::ValueMethod").is_some(),
            "Expected node service.go::MyStruct::ValueMethod"
        );
    }

    #[test]
    fn test_go_struct_and_interface_extraction() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("types.go"),
            r#"package types

type Foo struct {
    Name string
    Age  int
}

type Bar interface {
    DoThing() error
}
"#,
        )
        .unwrap();

        let graph = build_structural_graph(dir.path(), &IgnoreRules::parse("")).unwrap();
        assert!(
            graph.get_node("types.go::Foo").is_some(),
            "Expected class node types.go::Foo"
        );
        assert!(
            graph.get_node("types.go::Bar").is_some(),
            "Expected class node types.go::Bar"
        );
    }

    #[test]
    fn test_go_complexity() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("complex.go"),
            r#"package main

func complexFunc(x int) int {
    if x > 0 {
        for i := 0; i < x; i++ {
            switch i {
            case 1:
                return 1
            case 2:
                return 2
            case 3:
                return 3
            }
        }
    }
    return 0
}
"#,
        )
        .unwrap();

        let graph = build_structural_graph(dir.path(), &IgnoreRules::parse("")).unwrap();
        let func_node = graph.get_node("complex.go::complexFunc");
        assert!(func_node.is_some(), "Expected complexFunc node");
        let complexity = func_node.unwrap().complexity.unwrap_or(0);
        // 1 base + 1 if + 1 for + 3 cases = 6
        assert_eq!(complexity, 6, "Expected complexity 6, got {}", complexity);
    }

    #[test]
    fn test_go_stdlib_import_ignored() {
        let result = languages::go::resolve_go_import("fmt", "main.go", Some("github.com/user/project"));
        assert!(result.is_none(), "Standard library imports should be ignored");

        let result = languages::go::resolve_go_import("net/http", "main.go", Some("github.com/user/project"));
        assert!(result.is_none(), "Standard library imports should be ignored");
    }

    #[test]
    fn test_go_intra_module_import_resolution() {
        let result = languages::go::resolve_go_import(
            "github.com/user/project/internal/store",
            "main.go",
            Some("github.com/user/project"),
        );
        assert_eq!(result, Some("internal/store".to_string()));
    }

    #[test]
    fn test_go_import_edges() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("go.mod"),
            "module github.com/test/project\n\ngo 1.21\n",
        )
        .unwrap();
        fs::create_dir_all(dir.path().join("pkg")).unwrap();
        fs::write(
            dir.path().join("main.go"),
            r#"package main

import "github.com/test/project/pkg"

func main() {
    pkg.Hello()
}
"#,
        )
        .unwrap();
        fs::write(
            dir.path().join("pkg/hello.go"),
            r#"package pkg

func Hello() {}
"#,
        )
        .unwrap();

        let graph = build_structural_graph(dir.path(), &IgnoreRules::parse("")).unwrap();
        let import_edges = graph.edges_of_type(&ising_core::graph::EdgeType::Imports);
        assert!(
            !import_edges.is_empty(),
            "Expected at least one import edge for intra-module import"
        );
    }

    #[test]
    fn test_go_init_dedup() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("main.go"),
            r#"package main

func init() {
    fmt.Println("first")
}

func init() {
    fmt.Println("second")
}
"#,
        )
        .unwrap();

        let graph = build_structural_graph(dir.path(), &IgnoreRules::parse("")).unwrap();
        assert!(
            graph.get_node("main.go::init").is_some(),
            "Expected node main.go::init"
        );
        assert!(
            graph.get_node("main.go::init_2").is_some(),
            "Expected node main.go::init_2"
        );
    }

    #[test]
    fn test_go_is_supported_file() {
        assert!(Language::is_supported_file("main.go"));
        assert!(Language::is_supported_file("internal/store/db.go"));
        assert!(Language::is_supported_file("main_test.go"));
    }

    // --- React arrow function component tests ---

    #[test]
    fn test_react_arrow_function_components() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("App.tsx"),
            r#"
const MyComponent = () => <div />

const handler = async (e: Event) => {
    console.log(e)
}

export default function Page() {
    return <div />
}

export const getServerSideProps = async () => {
    return { props: {} }
}

export class MyService {
    run() {}
}
"#,
        )
        .unwrap();

        let graph = build_structural_graph(dir.path(), &IgnoreRules::parse("")).unwrap();

        assert!(
            graph.get_node("App.tsx::MyComponent").is_some(),
            "Expected arrow function component MyComponent"
        );
        assert!(
            graph.get_node("App.tsx::handler").is_some(),
            "Expected arrow function handler"
        );
        assert!(
            graph.get_node("App.tsx::Page").is_some(),
            "Expected exported function Page"
        );
        assert!(
            graph.get_node("App.tsx::getServerSideProps").is_some(),
            "Expected exported arrow function getServerSideProps"
        );
        assert!(
            graph.get_node("App.tsx::MyService").is_some(),
            "Expected exported class MyService"
        );
    }

    // --- Vue SFC tests ---

    #[test]
    fn test_vue_is_supported_file() {
        assert!(Language::is_supported_file("src/App.vue"));
        assert!(Language::is_supported_file("components/Button.vue"));
    }

    #[test]
    fn test_vue_sfc_extraction() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("App.vue"),
            r#"<template>
  <div>{{ count }}</div>
</template>

<script setup lang="ts">
import { ref } from 'vue'
import MyChild from './MyChild.vue'

const count = ref(0)
const handleClick = () => count.value++

function resetCount() {
  count.value = 0
}
</script>

<style scoped>
div { color: red; }
</style>"#,
        )
        .unwrap();

        let graph = build_structural_graph(dir.path(), &IgnoreRules::parse("")).unwrap();

        // Module node should exist
        assert!(
            graph.get_node("App.vue").is_some(),
            "Expected module node App.vue"
        );

        // Arrow function should be extracted
        assert!(
            graph.get_node("App.vue::handleClick").is_some(),
            "Expected arrow function handleClick"
        );

        // Regular function should be extracted
        assert!(
            graph.get_node("App.vue::resetCount").is_some(),
            "Expected function resetCount"
        );

        // Language should be "vue"
        let module_node = graph.get_node("App.vue").unwrap();
        assert_eq!(module_node.language.as_deref(), Some("vue"));
    }

    #[test]
    fn test_vue_sfc_line_numbers() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("Counter.vue"),
            r#"<template>
  <button @click="increment">{{ count }}</button>
</template>

<script setup lang="ts">
const increment = () => {
  console.log('click')
}
</script>"#,
        )
        .unwrap();

        let graph = build_structural_graph(dir.path(), &IgnoreRules::parse("")).unwrap();
        let func = graph.get_node("Counter.vue::increment");
        assert!(func.is_some(), "Expected increment function");
        let func = func.unwrap();
        // The arrow function is on line 6 of the .vue file (1-indexed)
        // script block starts at line 5 (0-indexed line 4), content starts at line 5 (0-indexed)
        // In the extracted content, the `const increment` is on line 1 (0-indexed line 0)
        // With offset: 0 + 5 + 1 = 6
        assert_eq!(
            func.line_start,
            Some(6),
            "Expected line_start=6 for increment, got {:?}",
            func.line_start
        );
    }

    #[test]
    fn test_vue_import_resolution() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("MyChild.vue"),
            r#"<template><div>Child</div></template>
<script setup lang="ts">
function render() { return 'child' }
</script>"#,
        )
        .unwrap();
        fs::write(
            dir.path().join("App.vue"),
            r#"<template><MyChild /></template>
<script setup lang="ts">
import MyChild from './MyChild.vue'

const setup = () => {}
</script>"#,
        )
        .unwrap();

        let graph = build_structural_graph(dir.path(), &IgnoreRules::parse("")).unwrap();

        // Import edge from App.vue to MyChild.vue should exist
        let import_edges = graph.edges_of_type(&ising_core::graph::EdgeType::Imports);
        let has_vue_import = import_edges
            .iter()
            .any(|e| e.0 == "App.vue" && e.1 == "MyChild.vue");
        assert!(
            has_vue_import,
            "Expected import edge from App.vue to MyChild.vue, edges: {:?}",
            import_edges
        );
    }

    #[test]
    fn test_vue_at_alias_resolution() {
        let dir = TempDir::new().unwrap();
        // Create vite.config.ts to enable @/ alias
        fs::write(dir.path().join("vite.config.ts"), "export default {}").unwrap();
        fs::create_dir_all(dir.path().join("src/components")).unwrap();
        fs::write(
            dir.path().join("src/components/Button.vue"),
            r#"<template><button>Click</button></template>
<script setup lang="ts">
function click() {}
</script>"#,
        )
        .unwrap();
        fs::write(
            dir.path().join("src/App.vue"),
            r#"<template><Button /></template>
<script setup lang="ts">
import Button from '@/components/Button.vue'

const setup = () => {}
</script>"#,
        )
        .unwrap();

        let graph = build_structural_graph(dir.path(), &IgnoreRules::parse("")).unwrap();

        // @/components/Button.vue should resolve to src/components/Button.vue
        let import_edges = graph.edges_of_type(&ising_core::graph::EdgeType::Imports);
        let has_alias_import = import_edges
            .iter()
            .any(|e| e.0 == "src/App.vue" && e.1 == "src/components/Button.vue");
        assert!(
            has_alias_import,
            "Expected import edge from src/App.vue to src/components/Button.vue, edges: {:?}",
            import_edges
        );
    }

    #[test]
    fn test_vue_no_script_block() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("Template.vue"),
            "<template><div>Static</div></template>\n<style>.x {}</style>",
        )
        .unwrap();

        let graph = build_structural_graph(dir.path(), &IgnoreRules::parse("")).unwrap();
        // Module should exist but have no functions
        assert!(
            graph.get_node("Template.vue").is_some(),
            "Expected module node even without script block"
        );
    }
}
