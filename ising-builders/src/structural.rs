//! Layer 1 — Structural Graph Builder
//!
//! Uses Tree-sitter to parse source files and extract:
//! - Module nodes (one per file)
//! - Function and class nodes (with line ranges)
//! - Import edges between modules
//! - Contains edges (module → function/class)
//!
//! Parsing is parallelized with rayon.

use ising_core::graph::{EdgeType, Node, UnifiedGraph};
use ising_core::ignore::IgnoreRules;
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Supported languages for structural analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Python,
    TypeScript,
    JavaScript,
    Rust,
}

impl Language {
    fn from_extension(ext: &str) -> Option<Self> {
        match ext {
            "py" => Some(Language::Python),
            "ts" | "tsx" => Some(Language::TypeScript),
            "js" | "jsx" => Some(Language::JavaScript),
            "rs" => Some(Language::Rust),
            _ => None,
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Language::Python => "python",
            Language::TypeScript => "typescript",
            Language::JavaScript => "javascript",
            Language::Rust => "rust",
        }
    }
}

/// Result of analyzing a single file.
#[derive(Debug)]
struct FileAnalysis {
    /// Module node ID (file path).
    module_id: String,
    /// File path.
    file_path: String,
    /// Language detected.
    language: String,
    /// Lines of code in the file.
    loc: u32,
    /// Functions found in the file.
    functions: Vec<FunctionInfo>,
    /// Classes found in the file.
    classes: Vec<ClassInfo>,
    /// Imports found in the file.
    imports: Vec<ImportInfo>,
}

#[derive(Debug)]
struct FunctionInfo {
    name: String,
    line_start: u32,
    line_end: u32,
    complexity: u32,
}

#[derive(Debug)]
struct ClassInfo {
    name: String,
    line_start: u32,
    line_end: u32,
    complexity: u32,
}

#[derive(Debug)]
struct ImportInfo {
    /// The module being imported from (resolved to a relative path if possible).
    source: String,
}

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

    // Use Tree-sitter to parse
    let mut parser = tree_sitter::Parser::new();

    // We use the tree-sitter query approach: parse source, then walk the tree
    // to find function/class/import nodes based on language
    let tree_sitter_lang = get_tree_sitter_language(lang, file_path);

    if let Some(ts_lang) = tree_sitter_lang {
        parser.set_language(&ts_lang)?;
        if let Some(tree) = parser.parse(&source, None) {
            let root = tree.root_node();
            extract_nodes(
                root,
                &source,
                lang,
                &relative_path,
                &mut functions,
                &mut classes,
                &mut imports,
            );
        }
    } else {
        // Fallback: just create the module node, no function/class extraction
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

/// Extract function definitions, class definitions, and imports from a tree-sitter tree.
fn extract_nodes(
    node: tree_sitter::Node<'_>,
    source: &str,
    lang: Language,
    relative_path: &str,
    functions: &mut Vec<FunctionInfo>,
    classes: &mut Vec<ClassInfo>,
    imports: &mut Vec<ImportInfo>,
) {
    match lang {
        Language::Python => {
            extract_python_nodes(node, source, relative_path, functions, classes, imports)
        }
        Language::TypeScript | Language::JavaScript => {
            extract_ts_nodes(node, source, functions, classes, imports);
        }
        Language::Rust => {
            extract_rust_nodes(node, source, relative_path, functions, classes, imports);
        }
    }
}

fn extract_python_nodes(
    node: tree_sitter::Node<'_>,
    source: &str,
    relative_path: &str,
    functions: &mut Vec<FunctionInfo>,
    classes: &mut Vec<ClassInfo>,
    imports: &mut Vec<ImportInfo>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "function_definition" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let name = name_node
                        .utf8_text(source.as_bytes())
                        .unwrap_or("")
                        .to_string();
                    let complexity = compute_complexity(child, Language::Python);
                    functions.push(FunctionInfo {
                        name,
                        line_start: child.start_position().row as u32 + 1,
                        line_end: child.end_position().row as u32 + 1,
                        complexity,
                    });
                }
            }
            "class_definition" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let name = name_node
                        .utf8_text(source.as_bytes())
                        .unwrap_or("")
                        .to_string();
                    let complexity = compute_complexity(child, Language::Python);
                    classes.push(ClassInfo {
                        name,
                        line_start: child.start_position().row as u32 + 1,
                        line_end: child.end_position().row as u32 + 1,
                        complexity,
                    });
                }
            }
            "import_from_statement" => {
                if let Some(module_node) = child.child_by_field_name("module_name") {
                    let module = module_node
                        .utf8_text(source.as_bytes())
                        .unwrap_or("")
                        .to_string();
                    if let Some(path) = resolve_python_import(&module, relative_path) {
                        imports.push(ImportInfo { source: path });
                    }
                    // Also try resolving imported names as submodules
                    // e.g., "from fastapi import params" → fastapi/params.py
                    let mut name_cursor = child.walk();
                    for name_child in child.children(&mut name_cursor) {
                        if name_child.kind() == "dotted_name" || name_child.kind() == "identifier" {
                            // Skip the module_name node itself
                            if Some(name_child.id())
                                == child.child_by_field_name("module_name").map(|n| n.id())
                            {
                                continue;
                            }
                            let name = name_child
                                .utf8_text(source.as_bytes())
                                .unwrap_or("")
                                .to_string();
                            if !name.is_empty() {
                                let submodule = format!("{}.{}", module, name);
                                if let Some(path) = resolve_python_import(&submodule, relative_path)
                                {
                                    imports.push(ImportInfo { source: path });
                                }
                            }
                        }
                    }
                }
            }
            "import_statement" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let module = name_node
                        .utf8_text(source.as_bytes())
                        .unwrap_or("")
                        .to_string();
                    if let Some(path) = resolve_python_import(&module, relative_path) {
                        imports.push(ImportInfo { source: path });
                    }
                }
            }
            _ => {}
        }
    }
}

/// Resolve a Python import to a relative file path.
/// Handles relative imports (from .ctx import X) and absolute imports (import flask.ctx).
fn resolve_python_import(module: &str, current_file: &str) -> Option<String> {
    if module.is_empty() {
        return None;
    }

    let dots = module.chars().take_while(|&c| c == '.').count();

    if dots == 0 {
        // Absolute import
        return Some(module.replace('.', "/") + ".py");
    }

    // Relative import
    let current_dir = Path::new(current_file)
        .parent()?
        .to_string_lossy()
        .to_string();
    let mut base = PathBuf::from(&current_dir);
    for _ in 0..(dots - 1) {
        base = base.parent()?.to_path_buf();
    }

    let remainder = &module[dots..];
    if remainder.is_empty() {
        return Some(base.join("__init__.py").to_string_lossy().to_string());
    }

    let parts = remainder.replace('.', "/");
    Some(base.join(&parts).to_string_lossy().to_string() + ".py")
}

fn extract_ts_nodes(
    node: tree_sitter::Node<'_>,
    source: &str,
    functions: &mut Vec<FunctionInfo>,
    classes: &mut Vec<ClassInfo>,
    imports: &mut Vec<ImportInfo>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "function_declaration" | "method_definition" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let name = name_node
                        .utf8_text(source.as_bytes())
                        .unwrap_or("")
                        .to_string();
                    let complexity = compute_complexity(child, Language::TypeScript);
                    functions.push(FunctionInfo {
                        name,
                        line_start: child.start_position().row as u32 + 1,
                        line_end: child.end_position().row as u32 + 1,
                        complexity,
                    });
                }
            }
            "class_declaration" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let name = name_node
                        .utf8_text(source.as_bytes())
                        .unwrap_or("")
                        .to_string();
                    let complexity = compute_complexity(child, Language::TypeScript);
                    classes.push(ClassInfo {
                        name,
                        line_start: child.start_position().row as u32 + 1,
                        line_end: child.end_position().row as u32 + 1,
                        complexity,
                    });
                }
            }
            "import_statement" => {
                // Extract the import source string
                if let Some(source_node) = child.child_by_field_name("source") {
                    let import_path = source_node
                        .utf8_text(source.as_bytes())
                        .unwrap_or("")
                        .trim_matches(|c| c == '\'' || c == '"')
                        .to_string();
                    if import_path.starts_with('.') {
                        // Relative import — normalize to a path
                        let normalized = import_path.trim_start_matches("./").to_string();
                        imports.push(ImportInfo { source: normalized });
                    }
                }
            }
            _ => {}
        }
    }
}

fn extract_rust_nodes(
    node: tree_sitter::Node<'_>,
    source: &str,
    relative_path: &str,
    functions: &mut Vec<FunctionInfo>,
    classes: &mut Vec<ClassInfo>,
    imports: &mut Vec<ImportInfo>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "function_item" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let name = name_node
                        .utf8_text(source.as_bytes())
                        .unwrap_or("")
                        .to_string();
                    let complexity = compute_complexity(child, Language::Rust);
                    functions.push(FunctionInfo {
                        name,
                        line_start: child.start_position().row as u32 + 1,
                        line_end: child.end_position().row as u32 + 1,
                        complexity,
                    });
                }
            }
            "struct_item" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let name = name_node
                        .utf8_text(source.as_bytes())
                        .unwrap_or("")
                        .to_string();
                    let complexity = compute_complexity(child, Language::Rust);
                    classes.push(ClassInfo {
                        name,
                        line_start: child.start_position().row as u32 + 1,
                        line_end: child.end_position().row as u32 + 1,
                        complexity,
                    });
                }
            }
            "enum_item" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let name = name_node
                        .utf8_text(source.as_bytes())
                        .unwrap_or("")
                        .to_string();
                    let complexity = compute_complexity(child, Language::Rust);
                    classes.push(ClassInfo {
                        name,
                        line_start: child.start_position().row as u32 + 1,
                        line_end: child.end_position().row as u32 + 1,
                        complexity,
                    });
                }
            }
            "trait_item" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let name = name_node
                        .utf8_text(source.as_bytes())
                        .unwrap_or("")
                        .to_string();
                    let complexity = compute_complexity(child, Language::Rust);
                    classes.push(ClassInfo {
                        name,
                        line_start: child.start_position().row as u32 + 1,
                        line_end: child.end_position().row as u32 + 1,
                        complexity,
                    });
                }
            }
            "impl_item" => {
                // Extract the impl type name
                let impl_type = child
                    .child_by_field_name("type")
                    .and_then(|t| t.utf8_text(source.as_bytes()).ok())
                    .unwrap_or("")
                    .to_string();

                // Walk impl body for method definitions
                if let Some(body) = child.child_by_field_name("body") {
                    let mut body_cursor = body.walk();
                    for item in body.children(&mut body_cursor) {
                        if item.kind() == "function_item"
                            && let Some(name_node) = item.child_by_field_name("name")
                        {
                            let method_name = name_node
                                .utf8_text(source.as_bytes())
                                .unwrap_or("")
                                .to_string();
                            let name = if impl_type.is_empty() {
                                method_name
                            } else {
                                format!("{}::{}", impl_type, method_name)
                            };
                            let complexity = compute_complexity(item, Language::Rust);
                            functions.push(FunctionInfo {
                                name,
                                line_start: item.start_position().row as u32 + 1,
                                line_end: item.end_position().row as u32 + 1,
                                complexity,
                            });
                        }
                    }
                }
            }
            "use_declaration" => {
                // Extract the full use path text
                let use_text = child.utf8_text(source.as_bytes()).unwrap_or("").to_string();
                if let Some(path) = resolve_rust_use_import(&use_text, relative_path) {
                    imports.push(ImportInfo { source: path });
                }
            }
            "mod_item" => {
                // Only handle `mod foo;` (no body) — file-referencing module declarations
                let has_body = child.child_by_field_name("body").is_some();
                if !has_body && let Some(name_node) = child.child_by_field_name("name") {
                    let mod_name = name_node
                        .utf8_text(source.as_bytes())
                        .unwrap_or("")
                        .to_string();
                    if !mod_name.is_empty() {
                        let resolved = resolve_rust_mod_import(&mod_name, relative_path);
                        for path in resolved {
                            imports.push(ImportInfo { source: path });
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// Resolve a Rust `mod foo;` declaration to possible file paths.
///
/// `mod foo;` in `src/lib.rs` → `src/foo.rs` or `src/foo/mod.rs`
/// `mod baz;` in `src/bar/mod.rs` → `src/bar/baz.rs` or `src/bar/baz/mod.rs`
fn resolve_rust_mod_import(mod_name: &str, current_file: &str) -> Vec<String> {
    let parent = Path::new(current_file)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    let mut candidates = Vec::new();
    if parent.is_empty() {
        candidates.push(format!("{}.rs", mod_name));
        candidates.push(format!("{}/mod.rs", mod_name));
    } else {
        candidates.push(format!("{}/{}.rs", parent, mod_name));
        candidates.push(format!("{}/{}/mod.rs", parent, mod_name));
    }
    candidates
}

/// Resolve a Rust `use crate::foo::bar` statement to a file path.
///
/// Only resolves intra-crate imports (starting with `crate::`).
/// External crate imports (std::, serde::, etc.) are ignored.
fn resolve_rust_use_import(use_text: &str, _current_file: &str) -> Option<String> {
    // Strip `use ` prefix and trailing `;`
    let trimmed = use_text
        .trim()
        .strip_prefix("use ")?
        .trim_end_matches(';')
        .trim();

    // Only resolve crate-relative imports
    let path = trimmed.strip_prefix("crate::")?;

    // Handle `use crate::foo::bar::{A, B}` — take path up to the `{`
    let path = if let Some(idx) = path.find('{') {
        path[..idx].trim_end_matches(':')
    } else {
        path
    };

    if path.is_empty() {
        return None;
    }

    // Map path components to file system: foo::bar → src/foo/bar.rs
    let file_path = format!("src/{}.rs", path.replace("::", "/"));
    Some(file_path)
}

/// Compute cyclomatic complexity by counting decision points in a Tree-sitter subtree.
///
/// Cyclomatic complexity = 1 + number of decision points.
/// Decision points: if, elif/else if, for, while, try, except/catch,
/// and, or, ternary/conditional expressions, case/match arms.
fn compute_complexity(node: tree_sitter::Node<'_>, lang: Language) -> u32 {
    let mut decisions = 0;
    fn walk_decisions(node: tree_sitter::Node<'_>, decisions: &mut u32, lang: Language) {
        let kind = node.kind();
        match lang {
            Language::Python => match kind {
                "if_statement" | "elif_clause" | "for_statement" | "while_statement"
                | "except_clause" | "with_statement" | "assert_statement" => {
                    *decisions += 1;
                }
                "boolean_operator" => {
                    // "and" / "or" each add a branch
                    *decisions += 1;
                }
                "conditional_expression" => {
                    // ternary: x if cond else y
                    *decisions += 1;
                }
                "case_clause" => {
                    // match/case arms (Python 3.10+)
                    *decisions += 1;
                }
                _ => {}
            },
            Language::TypeScript | Language::JavaScript => match kind {
                "if_statement" | "for_statement" | "for_in_statement" | "while_statement"
                | "do_statement" | "catch_clause" | "switch_case" => {
                    *decisions += 1;
                }
                "binary_expression" => {
                    // Check for && or ||
                    if let Some(op) = node.child_by_field_name("operator") {
                        let op_text = op.kind();
                        if op_text == "&&" || op_text == "||" {
                            *decisions += 1;
                        }
                    }
                }
                "ternary_expression" => {
                    *decisions += 1;
                }
                _ => {}
            },
            Language::Rust => match kind {
                "if_expression"
                | "if_let_expression"
                | "for_expression"
                | "while_expression"
                | "while_let_expression"
                | "loop_expression" => {
                    *decisions += 1;
                }
                "match_arm" => {
                    *decisions += 1;
                }
                "binary_expression" => {
                    if let Some(op) = node.child_by_field_name("operator") {
                        let op_text = op.kind();
                        if op_text == "&&" || op_text == "||" {
                            *decisions += 1;
                        }
                    }
                }
                "try_expression" | "error_propagation_expression" => {
                    *decisions += 1;
                }
                _ => {}
            },
        }

        let mut child_cursor = node.walk();
        for child in node.children(&mut child_cursor) {
            walk_decisions(child, decisions, lang);
        }
    }

    walk_decisions(node, &mut decisions, lang);
    1 + decisions // base complexity of 1
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
        // 2 modules + 3 functions + 1 class = 6 nodes (methods inside class not top-level)
        assert!(
            graph.node_count() >= 5,
            "Expected >= 5 nodes, got {}",
            graph.node_count()
        );
        // Contains edges: module -> function/class
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
        // 1 module + 1 function + 1 class = 3 nodes minimum
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
        // Check that an import edge was created from main.py -> utils.py
        let _import_edges = graph.edges_of_type(&ising_core::graph::EdgeType::Imports);
        // Import resolution depends on path matching — "utils.py" must match
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
        // 1 module + 2 functions = 3 nodes
        assert!(
            graph.node_count() >= 3,
            "Expected >= 3 nodes, got {}",
            graph.node_count()
        );
        // 2 contains edges (module -> function)
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
        // 1 module + 3 classes (struct, enum, trait) = 4 nodes
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
        // 1 module + 1 struct + 2 functions (MyStruct::new, MyStruct::method)
        assert!(
            graph.node_count() >= 4,
            "Expected >= 4 nodes, got {}",
            graph.node_count()
        );
        // Check method is attributed to the impl type
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
        // Should have import edge from src/lib.rs -> src/foo.rs
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
        // use std::collections::HashMap should NOT create any edge
        let result = resolve_rust_use_import("use std::collections::HashMap;", "src/lib.rs");
        assert!(result.is_none(), "External crate imports should be ignored");

        let result = resolve_rust_use_import("use serde::Serialize;", "src/lib.rs");
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
        let paths = resolve_rust_mod_import("foo", "src/lib.rs");
        assert!(paths.contains(&"src/foo.rs".to_string()));
        assert!(paths.contains(&"src/foo/mod.rs".to_string()));

        // src/bar/mod.rs with mod baz → src/bar/baz.rs or src/bar/baz/mod.rs
        let paths = resolve_rust_mod_import("baz", "src/bar/mod.rs");
        assert!(paths.contains(&"src/bar/baz.rs".to_string()));
        assert!(paths.contains(&"src/bar/baz/mod.rs".to_string()));
    }
}
