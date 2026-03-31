//! Python AST extraction via Tree-sitter.

use super::{CallInfo, ClassInfo, FunctionInfo, ImportInfo};
use std::path::{Path, PathBuf};

/// Extract Python functions, classes, and imports from a tree-sitter parse tree.
pub fn extract_nodes(
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
                    let complexity = compute_complexity(child);
                    let deprecated = has_deprecated_decorator(child, source);
                    let calls = extract_calls(child, source);
                    functions.push(FunctionInfo {
                        name,
                        line_start: child.start_position().row as u32 + 1,
                        line_end: child.end_position().row as u32 + 1,
                        complexity,
                        calls,
                        deprecated,
                    });
                }
            }
            "class_definition" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let name = name_node
                        .utf8_text(source.as_bytes())
                        .unwrap_or("")
                        .to_string();
                    let complexity = compute_complexity(child);
                    let deprecated = has_deprecated_decorator(child, source);
                    classes.push(ClassInfo {
                        name,
                        line_start: child.start_position().row as u32 + 1,
                        line_end: child.end_position().row as u32 + 1,
                        complexity,
                        deprecated,
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

/// Check if a function or class has a `@deprecated` or `@typing.deprecated` decorator.
fn has_deprecated_decorator(node: tree_sitter::Node<'_>, source: &str) -> bool {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "decorator" {
            let text = child
                .utf8_text(source.as_bytes())
                .unwrap_or("")
                .to_lowercase();
            if text.contains("deprecated") {
                return true;
            }
        }
    }
    false
}

/// Extract function calls from within a function body.
fn extract_calls(node: tree_sitter::Node<'_>, source: &str) -> Vec<CallInfo> {
    let mut calls = Vec::new();
    fn walk_calls(node: tree_sitter::Node<'_>, source: &str, calls: &mut Vec<CallInfo>) {
        if node.kind() == "call"
            && let Some(func_node) = node.child_by_field_name("function")
        {
            let callee = func_node
                .utf8_text(source.as_bytes())
                .unwrap_or("")
                .to_string();
            if !callee.is_empty() {
                calls.push(CallInfo {
                    callee,
                    line: node.start_position().row as u32 + 1,
                });
            }
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            walk_calls(child, source, calls);
        }
    }
    // Walk the function body (skip parameters, decorators, etc.)
    if let Some(body) = node.child_by_field_name("body") {
        walk_calls(body, source, &mut calls);
    }
    calls
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

/// Compute cyclomatic complexity for Python code.
fn compute_complexity(node: tree_sitter::Node<'_>) -> u32 {
    let mut decisions = 0;
    fn walk(node: tree_sitter::Node<'_>, decisions: &mut u32) {
        match node.kind() {
            "if_statement" | "elif_clause" | "for_statement" | "while_statement"
            | "except_clause" | "with_statement" | "assert_statement" => {
                *decisions += 1;
            }
            "boolean_operator" => {
                *decisions += 1;
            }
            "conditional_expression" => {
                *decisions += 1;
            }
            "case_clause" => {
                *decisions += 1;
            }
            _ => {}
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            walk(child, decisions);
        }
    }
    walk(node, &mut decisions);
    1 + decisions
}
