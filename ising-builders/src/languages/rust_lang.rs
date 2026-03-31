//! Rust AST extraction via Tree-sitter.

use super::{CallInfo, ClassInfo, FunctionInfo, ImportInfo};
use std::path::Path;

/// Extract Rust functions, structs, enums, traits, impl methods, and imports
/// from a tree-sitter parse tree.
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
            "function_item" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let name = name_node
                        .utf8_text(source.as_bytes())
                        .unwrap_or("")
                        .to_string();
                    let complexity = compute_complexity(child);
                    let deprecated = has_deprecated_attribute(child, source);
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
            "struct_item" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let name = name_node
                        .utf8_text(source.as_bytes())
                        .unwrap_or("")
                        .to_string();
                    let complexity = compute_complexity(child);
                    let deprecated = has_deprecated_attribute(child, source);
                    classes.push(ClassInfo {
                        name,
                        line_start: child.start_position().row as u32 + 1,
                        line_end: child.end_position().row as u32 + 1,
                        complexity,
                        deprecated,
                    });
                }
            }
            "enum_item" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let name = name_node
                        .utf8_text(source.as_bytes())
                        .unwrap_or("")
                        .to_string();
                    let complexity = compute_complexity(child);
                    let deprecated = has_deprecated_attribute(child, source);
                    classes.push(ClassInfo {
                        name,
                        line_start: child.start_position().row as u32 + 1,
                        line_end: child.end_position().row as u32 + 1,
                        complexity,
                        deprecated,
                    });
                }
            }
            "trait_item" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let name = name_node
                        .utf8_text(source.as_bytes())
                        .unwrap_or("")
                        .to_string();
                    let complexity = compute_complexity(child);
                    let deprecated = has_deprecated_attribute(child, source);
                    classes.push(ClassInfo {
                        name,
                        line_start: child.start_position().row as u32 + 1,
                        line_end: child.end_position().row as u32 + 1,
                        complexity,
                        deprecated,
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
                            let complexity = compute_complexity(item);
                            let deprecated = has_deprecated_attribute(item, source);
                            let calls = extract_calls(item, source);
                            functions.push(FunctionInfo {
                                name,
                                line_start: item.start_position().row as u32 + 1,
                                line_end: item.end_position().row as u32 + 1,
                                complexity,
                                calls,
                                deprecated,
                            });
                        }
                    }
                }
            }
            "use_declaration" => {
                let use_text = child.utf8_text(source.as_bytes()).unwrap_or("").to_string();
                if let Some(path) = resolve_use_import(&use_text) {
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
                        let resolved = resolve_mod_import(&mod_name, relative_path);
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
pub fn resolve_mod_import(mod_name: &str, current_file: &str) -> Vec<String> {
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
pub fn resolve_use_import(use_text: &str) -> Option<String> {
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

/// Check if a Rust item has a `#[deprecated]` attribute.
fn has_deprecated_attribute(node: tree_sitter::Node<'_>, source: &str) -> bool {
    // Check preceding siblings for attribute_item containing "deprecated"
    let mut sibling = node.prev_sibling();
    while let Some(s) = sibling {
        if s.kind() == "attribute_item" || s.kind() == "inner_attribute_item" {
            let text = s.utf8_text(source.as_bytes()).unwrap_or("").to_lowercase();
            if text.contains("deprecated") {
                return true;
            }
        } else if s.kind() != "line_comment" && s.kind() != "block_comment" {
            break; // Stop at non-attribute, non-comment siblings
        }
        sibling = s.prev_sibling();
    }
    false
}

/// Extract function calls from within a function body.
fn extract_calls(node: tree_sitter::Node<'_>, source: &str) -> Vec<CallInfo> {
    let mut calls = Vec::new();
    fn walk_calls(node: tree_sitter::Node<'_>, source: &str, calls: &mut Vec<CallInfo>) {
        match node.kind() {
            "call_expression" => {
                if let Some(func_node) = node.child_by_field_name("function") {
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
            }
            "macro_invocation" => {
                // e.g., println!(), vec![], format!()
                if let Some(macro_node) = node.child_by_field_name("macro") {
                    let callee = macro_node
                        .utf8_text(source.as_bytes())
                        .unwrap_or("")
                        .to_string();
                    if !callee.is_empty() {
                        calls.push(CallInfo {
                            callee: format!("{}!", callee),
                            line: node.start_position().row as u32 + 1,
                        });
                    }
                }
            }
            _ => {}
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            walk_calls(child, source, calls);
        }
    }
    if let Some(body) = node.child_by_field_name("body") {
        walk_calls(body, source, &mut calls);
    }
    calls
}

/// Compute cyclomatic complexity for Rust code.
fn compute_complexity(node: tree_sitter::Node<'_>) -> u32 {
    let mut decisions = 0;
    fn walk(node: tree_sitter::Node<'_>, decisions: &mut u32) {
        match node.kind() {
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
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            walk(child, decisions);
        }
    }
    walk(node, &mut decisions);
    1 + decisions
}
