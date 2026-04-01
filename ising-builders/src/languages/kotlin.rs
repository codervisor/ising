//! Kotlin AST extraction via Tree-sitter.

use super::{ClassInfo, FunctionInfo, ImportInfo};

/// Extract Kotlin functions, classes, and imports from a tree-sitter parse tree.
pub fn extract_nodes(
    node: tree_sitter::Node<'_>,
    source: &str,
    _relative_path: &str,
    functions: &mut Vec<FunctionInfo>,
    classes: &mut Vec<ClassInfo>,
    imports: &mut Vec<ImportInfo>,
) {
    walk_node(node, source, None, functions, classes, imports);
}

/// Iterative AST walk using an explicit stack to avoid stack overflow
/// on deeply nested Kotlin ASTs.
fn walk_node(
    node: tree_sitter::Node<'_>,
    source: &str,
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
                "class_declaration" | "object_declaration" | "interface_declaration" => {
                    extract_class_iterative(child, source, &mut stack, classes);
                }
                "function_declaration" => {
                    extract_function(child, source, cls_ctx.as_deref(), functions);
                }
                "import_header" | "import_list" => {
                    extract_import(child, source, imports);
                }
                _ => {
                    // Push container nodes for iterative processing
                    stack.push((child, cls_ctx.clone()));
                }
            }
        }
    }
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
        .or_else(|| {
            // object_declaration might use a different field
            let mut c = node.walk();
            node.children(&mut c)
                .find(|ch| ch.kind() == "type_identifier" || ch.kind() == "simple_identifier")
        })
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
    } else {
        // Try to find class_body child directly
        let mut c = node.walk();
        for ch in node.children(&mut c) {
            if ch.kind() == "class_body" || ch.kind() == "enum_class_body" {
                stack.push((ch, Some(name.clone())));
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
    let name_node = node.child_by_field_name("name").or_else(|| {
        let mut c = node.walk();
        node.children(&mut c)
            .find(|ch| ch.kind() == "simple_identifier")
    });

    let raw_name = match name_node {
        Some(n) => n.utf8_text(source.as_bytes()).unwrap_or("").to_string(),
        None => return,
    };

    if raw_name.is_empty() {
        return;
    }

    // Check for extension function: fun String.isEmail()
    // The receiver type appears before the function name
    let full_name = if let Some(receiver) = extract_receiver_type(node, source) {
        if let Some(cls) = current_class {
            format!("{}::{}.{}", cls, receiver, raw_name)
        } else {
            format!("{}.{}", receiver, raw_name)
        }
    } else if let Some(cls) = current_class {
        format!("{}::{}", cls, raw_name)
    } else {
        raw_name
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

/// Try to extract a receiver type for extension functions.
fn extract_receiver_type(node: tree_sitter::Node<'_>, source: &str) -> Option<String> {
    // In tree-sitter-kotlin, extension functions have a receiver type
    // Look for a type_identifier before the function name
    let mut c = node.walk();
    let children: Vec<_> = node.children(&mut c).collect();

    for (i, child) in children.iter().enumerate() {
        if child.kind() == "simple_identifier" {
            // Check if previous sibling is a "." and before that is a type
            if i >= 2
                && children[i - 1].kind() == "."
                && let Ok(text) = children[i - 2].utf8_text(source.as_bytes())
            {
                return Some(text.to_string());
            }
        }
    }
    None
}

fn extract_import(node: tree_sitter::Node<'_>, source: &str, imports: &mut Vec<ImportInfo>) {
    if node.kind() == "import_list" {
        let mut c = node.walk();
        for child in node.children(&mut c) {
            if child.kind() == "import_header" {
                extract_import(child, source, imports);
            }
        }
        return;
    }

    let text = node
        .utf8_text(source.as_bytes())
        .unwrap_or("")
        .trim()
        .to_string();

    // Strip "import " prefix
    let stripped = text.strip_prefix("import ").unwrap_or(&text).trim();

    if stripped.is_empty() {
        return;
    }

    // Skip standard library imports
    if stripped.starts_with("kotlin.")
        || stripped.starts_with("java.")
        || stripped.starts_with("javax.")
        || stripped.starts_with("android.")
        || stripped.starts_with("kotlinx.")
    {
        return;
    }

    // Strip wildcard and member imports
    let class_path = if let Some(pos) = stripped.rfind('.') {
        // Check if last segment starts with lowercase (member import)
        let last = &stripped[pos + 1..];
        if last == "*" || last.starts_with(|c: char| c.is_lowercase()) {
            &stripped[..pos]
        } else {
            stripped
        }
    } else {
        stripped
    };

    // Convert dotted path to file path
    let path = class_path.replace('.', "/") + ".kt";
    imports.push(ImportInfo { source: path });
}

/// Compute cyclomatic complexity for Kotlin code.
fn compute_complexity(node: tree_sitter::Node<'_>) -> u32 {
    let mut decisions = 0;
    fn walk(node: tree_sitter::Node<'_>, decisions: &mut u32) {
        match node.kind() {
            "if_expression" | "for_statement" | "while_statement" | "do_while_statement"
            | "catch_block" => {
                *decisions += 1;
            }
            "when_entry" => {
                // Count when entries (skip else)
                let mut c = node.walk();
                let is_else = node.children(&mut c).any(|ch| ch.kind() == "else");
                if !is_else {
                    *decisions += 1;
                }
            }
            "conjunction_expression" | "disjunction_expression" => {
                // && and ||
                *decisions += 1;
            }
            "elvis_expression" => {
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
