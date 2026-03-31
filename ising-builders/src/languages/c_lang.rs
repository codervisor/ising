//! C AST extraction via Tree-sitter.

use super::{ClassInfo, FunctionInfo, ImportInfo};
use std::path::Path;

/// Extract C functions, structs, enums, and includes from a tree-sitter parse tree.
pub fn extract_nodes(
    node: tree_sitter::Node<'_>,
    source: &str,
    relative_path: &str,
    repo_path: &Path,
    functions: &mut Vec<FunctionInfo>,
    classes: &mut Vec<ClassInfo>,
    imports: &mut Vec<ImportInfo>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "function_definition" => {
                extract_function(child, source, functions);
            }
            "struct_specifier" => {
                // Only extract struct definitions (with body), not forward declarations
                if has_body(child, "field_declaration_list") {
                    extract_type(child, source, classes);
                }
            }
            "enum_specifier" => {
                if has_body(child, "enumerator_list") {
                    extract_type(child, source, classes);
                }
            }
            "preproc_include" => {
                extract_include(child, source, relative_path, repo_path, imports);
            }
            "declaration" => {
                // Skip declarations (prototypes) — only extract definitions
            }
            _ => {}
        }
    }
}

fn extract_function(node: tree_sitter::Node<'_>, source: &str, functions: &mut Vec<FunctionInfo>) {
    // The function name is in the declarator
    let declarator = match node.child_by_field_name("declarator") {
        Some(d) => d,
        None => return,
    };

    let name = find_identifier(declarator, source);
    if name.is_empty() {
        return;
    }

    let complexity = compute_complexity(node);
    functions.push(FunctionInfo {
        name,
        line_start: node.start_position().row as u32 + 1,
        line_end: node.end_position().row as u32 + 1,
        complexity,
        calls: Vec::new(),
        deprecated: false,
    });
}

fn extract_type(node: tree_sitter::Node<'_>, source: &str, classes: &mut Vec<ClassInfo>) {
    let name = node
        .child_by_field_name("name")
        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
        .unwrap_or("")
        .to_string();

    // Anonymous structs/enums — skip
    if name.is_empty() {
        return;
    }

    let complexity = compute_complexity(node);
    classes.push(ClassInfo {
        name,
        line_start: node.start_position().row as u32 + 1,
        line_end: node.end_position().row as u32 + 1,
        complexity,
        deprecated: false,
    });
}

/// Extract #include "local.h" (quoted includes only, skip system <...> includes).
///
/// Also used by the C++ parser for shared include handling.
pub fn extract_include_from(
    node: tree_sitter::Node<'_>,
    source: &str,
    relative_path: &str,
    repo_path: &Path,
    imports: &mut Vec<ImportInfo>,
) {
    extract_include(node, source, relative_path, repo_path, imports);
}

fn extract_include(
    node: tree_sitter::Node<'_>,
    source: &str,
    relative_path: &str,
    repo_path: &Path,
    imports: &mut Vec<ImportInfo>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "string_literal" || child.kind() == "string_content" {
            let text = child
                .utf8_text(source.as_bytes())
                .unwrap_or("")
                .trim_matches('"')
                .to_string();

            if text.is_empty() {
                continue;
            }

            if let Some(resolved) = resolve_include(&text, relative_path, repo_path) {
                imports.push(ImportInfo { source: resolved });
            }
        }
        // Skip system_lib_string (<...>) includes
    }
}

/// Resolve a quoted include path to a relative file path.
pub fn resolve_include(
    include_path: &str,
    relative_path: &str,
    repo_path: &Path,
) -> Option<String> {
    let current_dir = Path::new(relative_path).parent().unwrap_or(Path::new(""));

    // Try relative to current file's directory
    let candidate = current_dir.join(include_path);
    let normalized = normalize_path(&candidate.to_string_lossy());
    if repo_path.join(&normalized).exists() {
        return Some(normalized);
    }

    // Try common include directories
    for search_dir in &["include", "src", ""] {
        let candidate = if search_dir.is_empty() {
            include_path.to_string()
        } else {
            format!("{}/{}", search_dir, include_path)
        };
        if repo_path.join(&candidate).exists() {
            return Some(candidate);
        }
    }

    // Return the relative path even if we can't verify it exists
    Some(normalized)
}

/// Check if a node has a child body of the given kind.
fn has_body(node: tree_sitter::Node<'_>, body_kind: &str) -> bool {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .any(|child| child.kind() == body_kind)
}

/// Find the identifier name in a declarator (which may be nested in pointer/function declarators).
fn find_identifier(node: tree_sitter::Node<'_>, source: &str) -> String {
    if node.kind() == "identifier" {
        return node.utf8_text(source.as_bytes()).unwrap_or("").to_string();
    }

    // Check declarator field first (for function_declarator, pointer_declarator, etc.)
    if let Some(declarator) = node.child_by_field_name("declarator") {
        return find_identifier(declarator, source);
    }

    // Fallback: find any identifier child
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "identifier" {
            return child.utf8_text(source.as_bytes()).unwrap_or("").to_string();
        }
    }

    // Recurse into children
    let mut cursor2 = node.walk();
    for child in node.children(&mut cursor2) {
        let found = find_identifier(child, source);
        if !found.is_empty() {
            return found;
        }
    }

    String::new()
}

/// Normalize a path by resolving `.` and `..` components.
fn normalize_path(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "." | "" => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    parts.join("/")
}

/// Compute cyclomatic complexity for C code.
pub fn compute_complexity(node: tree_sitter::Node<'_>) -> u32 {
    let mut decisions = 0;
    fn walk(node: tree_sitter::Node<'_>, decisions: &mut u32) {
        match node.kind() {
            "if_statement" | "for_statement" | "while_statement" | "do_statement" => {
                *decisions += 1;
            }
            "case_statement" => {
                *decisions += 1;
            }
            "binary_expression" => {
                if let Some(op) = node.child_by_field_name("operator")
                    && (op.kind() == "&&" || op.kind() == "||")
                {
                    *decisions += 1;
                }
            }
            "conditional_expression" => {
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
