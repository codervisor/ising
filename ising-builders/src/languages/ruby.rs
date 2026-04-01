//! Ruby AST extraction via Tree-sitter.

use super::{ClassInfo, FunctionInfo, ImportInfo};
use std::path::Path;

/// Extract Ruby functions, classes, modules, and imports from a tree-sitter parse tree.
pub fn extract_nodes(
    node: tree_sitter::Node<'_>,
    source: &str,
    relative_path: &str,
    repo_path: &Path,
    functions: &mut Vec<FunctionInfo>,
    classes: &mut Vec<ClassInfo>,
    imports: &mut Vec<ImportInfo>,
) {
    let is_rails = detect_rails(repo_path);
    walk_node(
        node,
        source,
        relative_path,
        repo_path,
        is_rails,
        None,
        functions,
        classes,
        imports,
    );
}

/// Iterative AST walk using an explicit stack to avoid stack overflow
/// on deeply nested Ruby ASTs (e.g., large Rails codebases like spring-boot).
#[allow(clippy::too_many_arguments)]
fn walk_node(
    node: tree_sitter::Node<'_>,
    source: &str,
    relative_path: &str,
    repo_path: &Path,
    is_rails: bool,
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
                "method" => {
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
                "singleton_method" => {
                    if let Some(name_node) = child.child_by_field_name("name") {
                        let raw_name = name_node
                            .utf8_text(source.as_bytes())
                            .unwrap_or("")
                            .to_string();
                        let name = if let Some(cls) = &cls_ctx {
                            format!("{}::self.{}", cls, raw_name)
                        } else {
                            format!("self.{}", raw_name)
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
                "class" | "module" => {
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
                        // Push class/module body with updated class context
                        if let Some(body) = child.child_by_field_name("body") {
                            stack.push((body, Some(name)));
                        }
                    }
                }
                "call" => {
                    extract_require(child, source, relative_path, repo_path, is_rails, imports);
                }
                _ => {
                    // Push container nodes for iterative processing
                    if child.named_child_count() > 0
                        && child.kind() != "method"
                        && child.kind() != "singleton_method"
                    {
                        stack.push((child, cls_ctx.clone()));
                    }
                }
            }
        }
    }
}

/// Extract require/require_relative calls.
fn extract_require(
    node: tree_sitter::Node<'_>,
    source: &str,
    relative_path: &str,
    repo_path: &Path,
    is_rails: bool,
    imports: &mut Vec<ImportInfo>,
) {
    let method_node = match node.child_by_field_name("method") {
        Some(m) => m,
        None => return,
    };
    let method_name = method_node.utf8_text(source.as_bytes()).unwrap_or("");

    if method_name != "require" && method_name != "require_relative" {
        return;
    }

    // Get the argument (first argument)
    let args = match node.child_by_field_name("arguments") {
        Some(a) => a,
        None => return,
    };

    let mut arg_cursor = args.walk();
    for arg in args.children(&mut arg_cursor) {
        if arg.kind() == "string" || arg.kind() == "string_content" {
            let text = extract_string_content(arg, source);
            if text.is_empty() {
                continue;
            }

            if let Some(resolved) =
                resolve_require(&text, method_name, relative_path, repo_path, is_rails)
            {
                imports.push(ImportInfo { source: resolved });
            }
        }
    }
}

/// Extract the text content from a string node.
fn extract_string_content(node: tree_sitter::Node<'_>, source: &str) -> String {
    let text = node.utf8_text(source.as_bytes()).unwrap_or("").to_string();
    // Strip quotes
    text.trim_matches('\'').trim_matches('"').to_string()
}

/// Resolve a require/require_relative path to a file path.
fn resolve_require(
    path: &str,
    method: &str,
    relative_path: &str,
    repo_path: &Path,
    is_rails: bool,
) -> Option<String> {
    // Skip standard library / gem requires
    if !path.starts_with('.') && !path.starts_with('/') && method == "require" {
        // Check if it's a project-relative require
        let with_ext = if path.ends_with(".rb") {
            path.to_string()
        } else {
            format!("{}.rb", path)
        };

        // Check common autoload paths
        let search_paths = if is_rails {
            vec![
                "app/models/",
                "app/controllers/",
                "app/services/",
                "app/helpers/",
                "app/jobs/",
                "lib/",
            ]
        } else {
            vec!["lib/", ""]
        };

        for prefix in search_paths {
            let candidate = format!("{}{}", prefix, with_ext);
            if repo_path.join(&candidate).exists() {
                return Some(candidate);
            }
        }

        // Try as-is
        if repo_path.join(&with_ext).exists() {
            return Some(with_ext);
        }

        return None;
    }

    if method == "require_relative" {
        // Resolve relative to current file's directory
        let current_dir = Path::new(relative_path).parent().unwrap_or(Path::new(""));
        let with_ext = if path.ends_with(".rb") {
            path.to_string()
        } else {
            format!("{}.rb", path)
        };
        let resolved = current_dir.join(&with_ext);
        // Normalize the path (handle ../)
        let normalized = normalize_path(&resolved.to_string_lossy());
        return Some(normalized);
    }

    None
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

/// Detect if the project is a Rails application.
fn detect_rails(repo_path: &Path) -> bool {
    repo_path.join("config/application.rb").exists()
        || repo_path.join("Gemfile").is_file()
            && std::fs::read_to_string(repo_path.join("Gemfile"))
                .unwrap_or_default()
                .contains("rails")
}

/// Compute cyclomatic complexity for Ruby code.
/// Uses an iterative traversal with an explicit stack to avoid stack overflow
/// on deeply nested ASTs (e.g., large Rails files).
fn compute_complexity(node: tree_sitter::Node<'_>) -> u32 {
    let mut decisions = 0u32;
    let mut stack = vec![node];

    while let Some(current) = stack.pop() {
        match current.kind() {
            "if" | "unless" | "if_modifier" | "unless_modifier" => {
                decisions += 1;
            }
            "for" | "while" | "until" | "while_modifier" | "until_modifier" => {
                decisions += 1;
            }
            "when" | "in_pattern" => {
                decisions += 1;
            }
            "rescue" => {
                decisions += 1;
            }
            "binary" => {
                if let Some(op) = current.child_by_field_name("operator") {
                    let op_kind = op.kind();
                    if op_kind == "&&" || op_kind == "||" || op_kind == "and" || op_kind == "or" {
                        decisions += 1;
                    }
                }
            }
            "conditional" => {
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
