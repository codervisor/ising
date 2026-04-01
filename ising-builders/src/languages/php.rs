//! PHP AST extraction via Tree-sitter.

use super::{ClassInfo, FunctionInfo, ImportInfo};
use std::path::Path;

/// Extract PHP functions, classes, and imports from a tree-sitter parse tree.
pub fn extract_nodes(
    node: tree_sitter::Node<'_>,
    source: &str,
    _relative_path: &str,
    repo_path: &Path,
    functions: &mut Vec<FunctionInfo>,
    classes: &mut Vec<ClassInfo>,
    imports: &mut Vec<ImportInfo>,
) {
    let psr4_map = read_composer_psr4(repo_path);
    walk_node(node, source, &psr4_map, None, functions, classes, imports);
}

/// Iterative AST walk using an explicit stack to avoid stack overflow
/// on deeply nested PHP ASTs.
fn walk_node(
    node: tree_sitter::Node<'_>,
    source: &str,
    psr4_map: &[(String, String)],
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
                    if let Some(name_node) = child.child_by_field_name("name") {
                        let raw_name = name_node
                            .utf8_text(source.as_bytes())
                            .unwrap_or("")
                            .to_string();
                        let name = if let Some(cls) = &cls_ctx {
                            format!("{}::{}", cls, raw_name)
                        } else {
                            raw_name
                        };
                        let complexity = compute_complexity(child);
                        functions.push(FunctionInfo {
                            name,
                            line_start: child.start_position().row as u32 + 1,
                            line_end: child.end_position().row as u32 + 1,
                            complexity,
                            calls: Vec::new(),
                            deprecated: false,
                        });
                    }
                }
                "method_declaration" => {
                    if let Some(name_node) = child.child_by_field_name("name") {
                        let raw_name = name_node
                            .utf8_text(source.as_bytes())
                            .unwrap_or("")
                            .to_string();
                        let name = if let Some(cls) = &cls_ctx {
                            format!("{}::{}", cls, raw_name)
                        } else {
                            raw_name
                        };
                        let complexity = compute_complexity(child);
                        functions.push(FunctionInfo {
                            name,
                            line_start: child.start_position().row as u32 + 1,
                            line_end: child.end_position().row as u32 + 1,
                            complexity,
                            calls: Vec::new(),
                            deprecated: false,
                        });
                    }
                }
                "class_declaration"
                | "interface_declaration"
                | "trait_declaration"
                | "enum_declaration" => {
                    if let Some(name_node) = child.child_by_field_name("name") {
                        let name = name_node
                            .utf8_text(source.as_bytes())
                            .unwrap_or("")
                            .to_string();
                        let complexity = compute_complexity(child);
                        classes.push(ClassInfo {
                            name: name.clone(),
                            line_start: child.start_position().row as u32 + 1,
                            line_end: child.end_position().row as u32 + 1,
                            complexity,
                            deprecated: false,
                        });
                        // Push class body with updated class context
                        if let Some(body) = child.child_by_field_name("body") {
                            stack.push((body, Some(name)));
                        }
                    }
                }
                "namespace_use_declaration" => {
                    extract_use_import(child, source, psr4_map, imports);
                }
                "namespace_definition" => {
                    // Push namespace body for iterative processing
                    if let Some(body) = child.child_by_field_name("body") {
                        stack.push((body, cls_ctx.clone()));
                    }
                }
                _ => {
                    // Push any container node that may hold declarations.
                    // The previous allowlist (program, declaration_list, compound_statement)
                    // missed nodes like expression_statement, if_statement, and other
                    // wrappers, causing 0 nodes extracted on repos like php-src.
                    if child.named_child_count() > 0 {
                        stack.push((child, cls_ctx.clone()));
                    }
                }
            }
        }
    }
}

/// Extract import from a `use` declaration.
fn extract_use_import(
    node: tree_sitter::Node<'_>,
    source: &str,
    psr4_map: &[(String, String)],
    imports: &mut Vec<ImportInfo>,
) {
    let text = node
        .utf8_text(source.as_bytes())
        .unwrap_or("")
        .trim()
        .to_string();

    // Strip "use " prefix and ";" suffix
    let stripped = text
        .strip_prefix("use ")
        .unwrap_or(&text)
        .trim_end_matches(';')
        .trim();

    if stripped.is_empty() {
        return;
    }

    // Handle grouped imports: use App\{Foo, Bar}
    // For now just take the base path
    let base = if let Some(brace_pos) = stripped.find('{') {
        stripped[..brace_pos].trim_end_matches('\\')
    } else {
        stripped
    };

    // Strip "function " or "const " prefixes
    let base = base
        .strip_prefix("function ")
        .or_else(|| base.strip_prefix("const "))
        .unwrap_or(base)
        .trim();

    if let Some(path) = resolve_php_namespace(base, psr4_map) {
        imports.push(ImportInfo { source: path });
    }
}

/// Resolve a PHP namespace to a file path using PSR-4 mappings.
fn resolve_php_namespace(namespace: &str, psr4_map: &[(String, String)]) -> Option<String> {
    // Skip built-in/vendor namespaces if no PSR-4 mapping matches
    let normalized = namespace.replace('\\', "/");

    for (prefix, dir) in psr4_map {
        let ns_prefix = prefix.replace('\\', "/").trim_end_matches('/').to_string();
        if let Some(rest) = normalized.strip_prefix(&ns_prefix) {
            let rest = rest.strip_prefix('/').unwrap_or(rest);
            let path = if rest.is_empty() {
                // Namespace itself
                return None;
            } else {
                let dir = dir.trim_end_matches('/');
                format!("{}/{}.php", dir, rest)
            };
            return Some(path);
        }
    }

    // Fallback: convert namespace directly to path
    Some(format!("{}.php", normalized))
}

/// Read PSR-4 autoload mappings from composer.json.
fn read_composer_psr4(repo_path: &Path) -> Vec<(String, String)> {
    let composer_path = repo_path.join("composer.json");
    let contents = match std::fs::read_to_string(composer_path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let parsed: serde_json::Value = match serde_json::from_str(&contents) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    let mut mappings = Vec::new();

    // Check autoload.psr-4 and autoload-dev.psr-4
    for section in &["autoload", "autoload-dev"] {
        if let Some(psr4) = parsed
            .get(*section)
            .and_then(|v| v.get("psr-4"))
            .and_then(|v| v.as_object())
        {
            for (prefix, dir) in psr4.iter() {
                if let Some(dir_str) = dir.as_str() {
                    mappings.push((prefix.to_string(), dir_str.to_string()));
                }
            }
        }
    }

    mappings
}

/// Compute cyclomatic complexity for PHP code.
/// Uses an iterative traversal with an explicit stack to avoid stack overflow
/// on deeply nested ASTs.
fn compute_complexity(node: tree_sitter::Node<'_>) -> u32 {
    let mut decisions = 0u32;
    let mut stack = vec![node];

    while let Some(current) = stack.pop() {
        match current.kind() {
            "if_statement" | "for_statement" | "foreach_statement" | "while_statement"
            | "do_statement" | "catch_clause" => {
                decisions += 1;
            }
            "case_statement" => {
                // default is a separate node kind (default_statement), so all
                // case_statement nodes are non-default cases worth counting.
                decisions += 1;
            }
            "match_conditional_expression" => {
                decisions += 1;
            }
            "binary_expression" => {
                if let Some(op) = current.child_by_field_name("operator") {
                    let op_text = op.kind();
                    if op_text == "&&" || op_text == "||" || op_text == "and" || op_text == "or" {
                        decisions += 1;
                    }
                }
            }
            "conditional_expression" => {
                decisions += 1;
            }
            _ => {}
        }
        let mut cursor = current.walk();
        for child in current.children(&mut cursor) {
            stack.push(child);
        }
    }

    1 + decisions
}
