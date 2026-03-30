//! Java AST extraction via Tree-sitter.

use super::{ClassInfo, FunctionInfo, ImportInfo};

/// Extract Java functions, classes, and imports from a tree-sitter parse tree.
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
            "class_declaration"
            | "interface_declaration"
            | "enum_declaration"
            | "record_declaration" => {
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
                // Recurse into class body for methods
                extract_class_members(child, source, functions);
            }
            "import_declaration" => {
                extract_import(child, source, relative_path, imports);
            }
            _ => {}
        }
    }
}

/// Extract methods from inside a class/interface declaration.
fn extract_class_members(
    class_node: tree_sitter::Node<'_>,
    source: &str,
    functions: &mut Vec<FunctionInfo>,
) {
    if let Some(body) = class_node.child_by_field_name("body") {
        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            if (child.kind() == "method_declaration" || child.kind() == "constructor_declaration")
                && let Some(name_node) = child.child_by_field_name("name")
            {
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
    }
}

/// Extract import path from a Java import declaration.
fn extract_import(
    node: tree_sitter::Node<'_>,
    source: &str,
    _relative_path: &str,
    imports: &mut Vec<ImportInfo>,
) {
    // Java imports look like: import com.example.Foo;
    // or: import static com.example.Foo.bar;
    // We extract the dotted path and convert to a file path.
    let text = node
        .utf8_text(source.as_bytes())
        .unwrap_or("")
        .trim()
        .to_string();

    // Strip "import " prefix and ";" suffix
    let stripped = text
        .strip_prefix("import ")
        .unwrap_or(&text)
        .strip_prefix("static ")
        .unwrap_or(text.strip_prefix("import ").unwrap_or(&text))
        .trim_end_matches(';')
        .trim();

    if stripped.is_empty() || stripped.ends_with('*') {
        return;
    }

    // Convert dotted path to file path: com.example.Foo -> com/example/Foo.java
    let path = stripped.replace('.', "/") + ".java";
    imports.push(ImportInfo { source: path });
}

/// Compute cyclomatic complexity for Java code.
fn compute_complexity(node: tree_sitter::Node<'_>) -> u32 {
    let mut decisions = 0;
    fn walk(node: tree_sitter::Node<'_>, decisions: &mut u32) {
        match node.kind() {
            "if_statement"
            | "for_statement"
            | "enhanced_for_statement"
            | "while_statement"
            | "do_statement"
            | "catch_clause"
            | "switch_expression_arm"
            | "ternary_expression" => {
                *decisions += 1;
            }
            "switch_label" => {
                // Count case labels (skip default)
                let mut c = node.walk();
                let is_default = node.children(&mut c).any(|ch| ch.kind() == "default");
                if !is_default {
                    *decisions += 1;
                }
            }
            "binary_expression" => {
                // Count && and ||
                if let Some(op) = node.child_by_field_name("operator") {
                    if op.kind() == "&&" || op.kind() == "||" {
                        *decisions += 1;
                    }
                }
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
