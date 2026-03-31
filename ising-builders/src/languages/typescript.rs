//! TypeScript/JavaScript AST extraction via Tree-sitter.

use super::{CallInfo, ClassInfo, FunctionInfo, ImportInfo};

/// Extract TypeScript/JavaScript functions, classes, and imports from a tree-sitter parse tree.
///
/// The `line_offset` parameter is added to all line numbers — used by the Vue extractor to
/// adjust positions relative to the `.vue` file rather than the extracted script block.
pub fn extract_nodes(
    node: tree_sitter::Node<'_>,
    source: &str,
    functions: &mut Vec<FunctionInfo>,
    classes: &mut Vec<ClassInfo>,
    imports: &mut Vec<ImportInfo>,
) {
    extract_nodes_with_offset(node, source, functions, classes, imports, 0);
}

/// Extract nodes with a line offset applied to all positions.
pub fn extract_nodes_with_offset(
    node: tree_sitter::Node<'_>,
    source: &str,
    functions: &mut Vec<FunctionInfo>,
    classes: &mut Vec<ClassInfo>,
    imports: &mut Vec<ImportInfo>,
    line_offset: u32,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "function_declaration" | "method_definition" => {
                extract_function(child, source, functions, line_offset);
            }
            "class_declaration" => {
                extract_class(child, source, classes, line_offset);
            }
            "lexical_declaration" => {
                // Case A: const Foo = () => {} or const Foo = function() {}
                extract_arrow_or_fn_expr(child, source, functions, line_offset);
            }
            "export_statement" => {
                // Case B: export default function / export const / export class
                let mut inner_cursor = child.walk();
                for inner in child.children(&mut inner_cursor) {
                    match inner.kind() {
                        "function_declaration" => {
                            extract_function(inner, source, functions, line_offset);
                        }
                        "class_declaration" => {
                            extract_class(inner, source, classes, line_offset);
                        }
                        "lexical_declaration" => {
                            extract_arrow_or_fn_expr(inner, source, functions, line_offset);
                        }
                        _ => {}
                    }
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

/// Check if a node has a `@deprecated` JSDoc comment preceding it.
fn has_deprecated_jsdoc(node: tree_sitter::Node<'_>, source: &str) -> bool {
    if let Some(prev) = node.prev_sibling()
        && prev.kind() == "comment"
    {
        let text = prev
            .utf8_text(source.as_bytes())
            .unwrap_or("")
            .to_lowercase();
        if text.contains("@deprecated") {
            return true;
        }
    }
    false
}

/// Extract function calls from within a function body.
fn extract_calls_from_body(node: tree_sitter::Node<'_>, source: &str) -> Vec<CallInfo> {
    let mut calls = Vec::new();
    fn walk_calls(node: tree_sitter::Node<'_>, source: &str, calls: &mut Vec<CallInfo>) {
        if node.kind() == "call_expression"
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
    if let Some(body) = node.child_by_field_name("body") {
        walk_calls(body, source, &mut calls);
    }
    calls
}

/// Extract a function_declaration or method_definition node.
fn extract_function(
    node: tree_sitter::Node<'_>,
    source: &str,
    functions: &mut Vec<FunctionInfo>,
    line_offset: u32,
) {
    if let Some(name_node) = node.child_by_field_name("name") {
        let name = name_node
            .utf8_text(source.as_bytes())
            .unwrap_or("")
            .to_string();
        let complexity = compute_complexity(node);
        let deprecated = has_deprecated_jsdoc(node, source);
        let calls = extract_calls_from_body(node, source);
        functions.push(FunctionInfo {
            name,
            line_start: node.start_position().row as u32 + 1 + line_offset,
            line_end: node.end_position().row as u32 + 1 + line_offset,
            complexity,
            calls,
            deprecated,
        });
    }
}

/// Extract a class_declaration node.
fn extract_class(
    node: tree_sitter::Node<'_>,
    source: &str,
    classes: &mut Vec<ClassInfo>,
    line_offset: u32,
) {
    if let Some(name_node) = node.child_by_field_name("name") {
        let name = name_node
            .utf8_text(source.as_bytes())
            .unwrap_or("")
            .to_string();
        let complexity = compute_complexity(node);
        let deprecated = has_deprecated_jsdoc(node, source);
        classes.push(ClassInfo {
            name,
            line_start: node.start_position().row as u32 + 1 + line_offset,
            line_end: node.end_position().row as u32 + 1 + line_offset,
            complexity,
            deprecated,
        });
    }
}

/// Extract arrow functions or function expressions from a lexical_declaration.
/// Handles: `const Foo = () => {}`, `const handler = async (e) => {}`, `const foo = function() {}`
fn extract_arrow_or_fn_expr(
    node: tree_sitter::Node<'_>,
    source: &str,
    functions: &mut Vec<FunctionInfo>,
    line_offset: u32,
) {
    let mut cursor = node.walk();
    for declarator in node.children(&mut cursor) {
        if declarator.kind() != "variable_declarator" {
            continue;
        }
        // Check if the value is an arrow_function or function_expression (possibly wrapped in call_expression for HOCs)
        let value = declarator.child_by_field_name("value");
        let is_fn = value.is_some_and(|v| {
            matches!(
                v.kind(),
                "arrow_function" | "function_expression" | "function"
            )
        });
        if !is_fn {
            continue;
        }
        if let Some(name_node) = declarator.child_by_field_name("name") {
            let name = name_node
                .utf8_text(source.as_bytes())
                .unwrap_or("")
                .to_string();
            let fn_node = value.unwrap();
            let complexity = compute_complexity(fn_node);
            let deprecated = has_deprecated_jsdoc(node, source);
            let calls = extract_calls_from_body(fn_node, source);
            functions.push(FunctionInfo {
                name,
                line_start: node.start_position().row as u32 + 1 + line_offset,
                line_end: node.end_position().row as u32 + 1 + line_offset,
                complexity,
                calls,
                deprecated,
            });
        }
    }
}

/// Compute cyclomatic complexity for TypeScript/JavaScript code.
fn compute_complexity(node: tree_sitter::Node<'_>) -> u32 {
    let mut decisions = 0;
    fn walk(node: tree_sitter::Node<'_>, decisions: &mut u32) {
        match node.kind() {
            "if_statement" | "for_statement" | "for_in_statement" | "while_statement"
            | "do_statement" | "catch_clause" | "switch_case" => {
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
            "ternary_expression" => {
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
