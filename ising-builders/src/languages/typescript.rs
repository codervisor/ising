//! TypeScript/JavaScript AST extraction via Tree-sitter.

use super::{ClassInfo, FunctionInfo, ImportInfo};

/// Extract TypeScript/JavaScript functions, classes, and imports from a tree-sitter parse tree.
pub fn extract_nodes(
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
                    let complexity = compute_complexity(child);
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
                    let complexity = compute_complexity(child);
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
