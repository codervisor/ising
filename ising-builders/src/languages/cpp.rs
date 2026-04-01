//! C++ AST extraction via Tree-sitter.

use super::{ClassInfo, FunctionInfo, ImportInfo};
use std::path::Path;

/// Extract C++ functions, classes, structs, and includes from a tree-sitter parse tree.
pub fn extract_nodes(
    node: tree_sitter::Node<'_>,
    source: &str,
    relative_path: &str,
    repo_path: &Path,
    functions: &mut Vec<FunctionInfo>,
    classes: &mut Vec<ClassInfo>,
    imports: &mut Vec<ImportInfo>,
) {
    walk_node(
        node,
        source,
        relative_path,
        repo_path,
        None,
        functions,
        classes,
        imports,
    );
}

/// Iterative AST walk using an explicit stack to avoid stack overflow
/// on deeply nested C++ ASTs.
#[allow(clippy::too_many_arguments)]
fn walk_node(
    node: tree_sitter::Node<'_>,
    source: &str,
    relative_path: &str,
    repo_path: &Path,
    current_class: Option<&str>,
    functions: &mut Vec<FunctionInfo>,
    classes: &mut Vec<ClassInfo>,
    imports: &mut Vec<ImportInfo>,
) {
    // Stack of (node_to_iterate_children_of, current_class_context)
    let mut stack: Vec<(tree_sitter::Node<'_>, Option<String>)> =
        vec![(node, current_class.map(|s| s.to_string()))];

    while let Some((current, cls_ctx)) = stack.pop() {
        let mut cursor = current.walk();
        for child in current.children(&mut cursor) {
            match child.kind() {
                "function_definition" => {
                    extract_function(child, source, cls_ctx.as_deref(), functions);
                }
                "class_specifier" | "struct_specifier" => {
                    if has_body(child) {
                        extract_class_iterative(child, source, &mut stack, classes);
                    }
                }
                "enum_specifier" => {
                    if has_body(child) {
                        extract_enum(child, source, classes);
                    }
                }
                "namespace_definition" => {
                    if let Some(body) = child.child_by_field_name("body") {
                        stack.push((body, cls_ctx.clone()));
                    }
                }
                "preproc_include" => {
                    super::c_lang::extract_include_from(
                        child,
                        source,
                        relative_path,
                        repo_path,
                        imports,
                    );
                }
                "template_declaration" => {
                    // Push template content for iterative processing
                    stack.push((child, cls_ctx.clone()));
                }
                "declaration" | "access_specifier" | "field_declaration" | "friend_declaration" => {
                    // Skip forward declarations and class-body-only nodes
                }
                _ => {}
            }
        }
    }
}

fn extract_function(
    node: tree_sitter::Node<'_>,
    source: &str,
    current_class: Option<&str>,
    functions: &mut Vec<FunctionInfo>,
) {
    let declarator = match node.child_by_field_name("declarator") {
        Some(d) => d,
        None => return,
    };

    let name = find_function_name(declarator, source);
    if name.is_empty() {
        return;
    }

    // Check for out-of-class method: ClassName::method
    let full_name = if name.contains("::") {
        // Already qualified
        name
    } else if let Some(cls) = current_class {
        format!("{}::{}", cls, name)
    } else {
        name
    };

    let complexity = compute_complexity(node);
    functions.push(FunctionInfo {
        name: full_name,
        line_start: node.start_position().row as u32 + 1,
        line_end: node.end_position().row as u32 + 1,
        complexity,
        calls: Vec::new(),
        deprecated: false,
    });
}

/// Extract class info and push body onto the iterative walk stack.
fn extract_class_iterative<'a>(
    node: tree_sitter::Node<'a>,
    source: &str,
    stack: &mut Vec<(tree_sitter::Node<'a>, Option<String>)>,
    classes: &mut Vec<ClassInfo>,
) {
    let name = node
        .child_by_field_name("name")
        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
        .unwrap_or("")
        .to_string();

    if name.is_empty() {
        return;
    }

    let complexity = compute_complexity(node);
    classes.push(ClassInfo {
        name: name.clone(),
        line_start: node.start_position().row as u32 + 1,
        line_end: node.end_position().row as u32 + 1,
        complexity,
        deprecated: false,
    });

    // Push class body onto stack with class context
    if let Some(body) = node.child_by_field_name("body") {
        stack.push((body, Some(name)));
    }
}

fn extract_enum(node: tree_sitter::Node<'_>, source: &str, classes: &mut Vec<ClassInfo>) {
    let name = node
        .child_by_field_name("name")
        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
        .unwrap_or("")
        .to_string();

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

/// Find function name, handling qualified names (ClassName::method).
fn find_function_name(node: tree_sitter::Node<'_>, source: &str) -> String {
    // Handle qualified_identifier: ClassName::method_name
    if node.kind() == "qualified_identifier" {
        return node.utf8_text(source.as_bytes()).unwrap_or("").to_string();
    }

    if node.kind() == "identifier" {
        return node.utf8_text(source.as_bytes()).unwrap_or("").to_string();
    }

    // For function_declarator, get the declarator child
    if let Some(declarator) = node.child_by_field_name("declarator") {
        return find_function_name(declarator, source);
    }

    // Check for qualified name in children
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "qualified_identifier" | "identifier" => {
                return child.utf8_text(source.as_bytes()).unwrap_or("").to_string();
            }
            "destructor_name" => {
                return child.utf8_text(source.as_bytes()).unwrap_or("").to_string();
            }
            _ => {}
        }
    }

    // Recurse
    let mut cursor2 = node.walk();
    for child in node.children(&mut cursor2) {
        let found = find_function_name(child, source);
        if !found.is_empty() {
            return found;
        }
    }

    String::new()
}

/// Check if a type specifier has a body (field_declaration_list for struct/class, enumerator_list for enum).
fn has_body(node: tree_sitter::Node<'_>) -> bool {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .any(|child| child.kind() == "field_declaration_list" || child.kind() == "enumerator_list")
}

/// Compute cyclomatic complexity for C++ code.
fn compute_complexity(node: tree_sitter::Node<'_>) -> u32 {
    let mut decisions = 0;
    fn walk(node: tree_sitter::Node<'_>, decisions: &mut u32) {
        match node.kind() {
            "if_statement" | "for_statement" | "while_statement" | "do_statement"
            | "for_range_loop" => {
                *decisions += 1;
            }
            "case_statement" => {
                *decisions += 1;
            }
            "catch_clause" => {
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
