//! C# AST extraction via Tree-sitter.

use super::{ClassInfo, FunctionInfo, ImportInfo};

/// Extract C# functions, classes, and imports from a tree-sitter parse tree.
pub fn extract_nodes(
    node: tree_sitter::Node<'_>,
    source: &str,
    _relative_path: &str,
    functions: &mut Vec<FunctionInfo>,
    classes: &mut Vec<ClassInfo>,
    imports: &mut Vec<ImportInfo>,
) {
    walk_node(node, source, functions, classes, imports);
}

/// Recursively walk the C# AST to find declarations at any nesting level.
fn walk_node(
    node: tree_sitter::Node<'_>,
    source: &str,
    functions: &mut Vec<FunctionInfo>,
    classes: &mut Vec<ClassInfo>,
    imports: &mut Vec<ImportInfo>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "class_declaration"
            | "interface_declaration"
            | "struct_declaration"
            | "enum_declaration"
            | "record_declaration"
            | "record_struct_declaration" => {
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
                // Recurse into type body for methods
                walk_node(child, source, functions, classes, imports);
            }
            "method_declaration"
            | "constructor_declaration"
            | "destructor_declaration"
            | "operator_declaration"
            | "conversion_operator_declaration" => {
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
            "property_declaration" => {
                // Properties with accessors can have significant logic
                if let Some(name_node) = child.child_by_field_name("name") {
                    let name = name_node
                        .utf8_text(source.as_bytes())
                        .unwrap_or("")
                        .to_string();
                    let complexity = compute_complexity(child);
                    if complexity > 1 {
                        functions.push(FunctionInfo {
                            name,
                            line_start: child.start_position().row as u32 + 1,
                            line_end: child.end_position().row as u32 + 1,
                            complexity,
                        });
                    }
                }
            }
            "using_directive" => {
                extract_using(child, source, imports);
            }
            "namespace_declaration" | "file_scoped_namespace_declaration" => {
                // Recurse into namespace body
                walk_node(child, source, functions, classes, imports);
            }
            "global_statement" => {
                walk_node(child, source, functions, classes, imports);
            }
            _ => {}
        }
    }
}

/// Extract using directive to an import path.
fn extract_using(node: tree_sitter::Node<'_>, source: &str, imports: &mut Vec<ImportInfo>) {
    // C# using: using System.Collections.Generic;
    // or: using Alias = Namespace.Type;
    let text = node
        .utf8_text(source.as_bytes())
        .unwrap_or("")
        .trim()
        .to_string();

    let stripped = text
        .strip_prefix("using ")
        .unwrap_or(&text)
        .strip_prefix("static ")
        .unwrap_or(text.strip_prefix("using ").unwrap_or(&text))
        .strip_prefix("global ")
        .unwrap_or(text.strip_prefix("using ").unwrap_or(&text))
        .trim_end_matches(';')
        .trim();

    if stripped.is_empty() {
        return;
    }

    // Handle alias: using Alias = Namespace.Type
    let namespace = if let Some((_alias, ns)) = stripped.split_once('=') {
        ns.trim()
    } else {
        stripped
    };

    // Convert dotted namespace to path: System.Collections -> System/Collections.cs
    let path = namespace.replace('.', "/") + ".cs";
    imports.push(ImportInfo { source: path });
}

/// Compute cyclomatic complexity for C# code.
fn compute_complexity(node: tree_sitter::Node<'_>) -> u32 {
    let mut decisions = 0;
    fn walk(node: tree_sitter::Node<'_>, decisions: &mut u32) {
        match node.kind() {
            "if_statement"
            | "for_statement"
            | "for_each_statement"
            | "while_statement"
            | "do_statement"
            | "catch_clause"
            | "case_switch_label"
            | "case_pattern_switch_label"
            | "conditional_expression" => {
                *decisions += 1;
            }
            "binary_expression" => {
                if let Some(op) = node.child_by_field_name("operator")
                    && (op.kind() == "&&" || op.kind() == "||" || op.kind() == "??")
                {
                    *decisions += 1;
                }
            }
            "switch_expression_arm" => {
                *decisions += 1;
            }
            "lambda_expression" => {
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
