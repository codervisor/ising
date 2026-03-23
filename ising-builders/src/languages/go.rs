//! Go AST extraction via Tree-sitter.

use super::{ClassInfo, FunctionInfo, ImportInfo};
use std::path::Path;

/// Extract Go functions, methods, structs, interfaces, and imports
/// from a tree-sitter parse tree.
pub fn extract_nodes(
    node: tree_sitter::Node<'_>,
    source: &str,
    relative_path: &str,
    repo_path: &Path,
    functions: &mut Vec<FunctionInfo>,
    classes: &mut Vec<ClassInfo>,
    imports: &mut Vec<ImportInfo>,
) {
    let module_path = read_go_mod_module(repo_path);
    let mut init_count: u32 = 0;

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "function_declaration" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let mut name = name_node
                        .utf8_text(source.as_bytes())
                        .unwrap_or("")
                        .to_string();
                    // Handle duplicate init() functions
                    if name == "init" {
                        init_count += 1;
                        if init_count > 1 {
                            name = format!("init_{}", init_count);
                        }
                    }
                    let complexity = compute_complexity(child);
                    functions.push(FunctionInfo {
                        name,
                        line_start: child.start_position().row as u32 + 1,
                        line_end: child.end_position().row as u32 + 1,
                        complexity,
                    });
                }
            }
            "method_declaration" => {
                let receiver_type = extract_receiver_type(child, source);
                if let Some(name_node) = child.child_by_field_name("name") {
                    let method_name = name_node
                        .utf8_text(source.as_bytes())
                        .unwrap_or("")
                        .to_string();
                    let name = if let Some(ref recv) = receiver_type {
                        format!("{}::{}", recv, method_name)
                    } else {
                        method_name
                    };
                    let complexity = compute_complexity(child);
                    functions.push(FunctionInfo {
                        name,
                        line_start: child.start_position().row as u32 + 1,
                        line_end: child.end_position().row as u32 + 1,
                        complexity,
                    });
                }
            }
            "type_declaration" => {
                extract_type_declaration(child, source, classes);
            }
            "import_declaration" => {
                extract_imports(
                    child,
                    source,
                    relative_path,
                    module_path.as_deref(),
                    imports,
                );
            }
            _ => {}
        }
    }
}

/// Extract the receiver type from a method declaration.
///
/// `func (s *MyStruct) Method()` → `MyStruct`
/// `func (s MyStruct) Method()` → `MyStruct`
fn extract_receiver_type(node: tree_sitter::Node<'_>, source: &str) -> Option<String> {
    // In tree-sitter-go, the receiver is in a field called "receiver"
    let receiver = node.child_by_field_name("receiver")?;
    find_type_identifier(receiver, source)
}

/// Recursively find the first type_identifier in a node tree.
fn find_type_identifier(node: tree_sitter::Node<'_>, source: &str) -> Option<String> {
    if node.kind() == "type_identifier" {
        return node.utf8_text(source.as_bytes()).ok().map(String::from);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(found) = find_type_identifier(child, source) {
            return Some(found);
        }
    }
    None
}

/// Extract struct and interface types from a type_declaration node.
fn extract_type_declaration(
    node: tree_sitter::Node<'_>,
    source: &str,
    classes: &mut Vec<ClassInfo>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "type_spec" {
            let name = child
                .child_by_field_name("name")
                .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                .unwrap_or("")
                .to_string();
            let type_node = child.child_by_field_name("type");
            let is_struct_or_interface = type_node
                .map(|t| t.kind() == "struct_type" || t.kind() == "interface_type")
                .unwrap_or(false);
            if is_struct_or_interface && !name.is_empty() {
                let complexity = type_node.map(|t| compute_complexity(t)).unwrap_or(1);
                classes.push(ClassInfo {
                    name,
                    line_start: child.start_position().row as u32 + 1,
                    line_end: child.end_position().row as u32 + 1,
                    complexity,
                });
            }
        }
    }
}

/// Extract imports from an import_declaration node.
fn extract_imports(
    node: tree_sitter::Node<'_>,
    source: &str,
    relative_path: &str,
    module_path: Option<&str>,
    imports: &mut Vec<ImportInfo>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "import_spec" => {
                if let Some(path) = resolve_import_spec(child, source, relative_path, module_path) {
                    imports.push(ImportInfo { source: path });
                }
            }
            "import_spec_list" => {
                let mut list_cursor = child.walk();
                for spec in child.children(&mut list_cursor) {
                    if spec.kind() == "import_spec"
                        && let Some(path) =
                            resolve_import_spec(spec, source, relative_path, module_path)
                    {
                        imports.push(ImportInfo { source: path });
                    }
                }
            }
            // Single import without parens: `import "fmt"`
            "interpreted_string_literal" => {
                let import_path = child
                    .utf8_text(source.as_bytes())
                    .unwrap_or("")
                    .trim_matches('"')
                    .to_string();
                if let Some(resolved) = resolve_go_import(&import_path, relative_path, module_path)
                {
                    imports.push(ImportInfo { source: resolved });
                }
            }
            _ => {}
        }
    }
}

/// Resolve a single import_spec node to a file path.
fn resolve_import_spec(
    node: tree_sitter::Node<'_>,
    source: &str,
    relative_path: &str,
    module_path: Option<&str>,
) -> Option<String> {
    let path_node = node.child_by_field_name("path")?;
    let import_path = path_node
        .utf8_text(source.as_bytes())
        .ok()?
        .trim_matches('"')
        .to_string();
    resolve_go_import(&import_path, relative_path, module_path)
}

/// Resolve a Go import path to a relative directory path within the repo.
///
/// Only resolves intra-module imports (paths starting with the module path from go.mod).
/// Standard library imports are ignored.
pub fn resolve_go_import(
    import_path: &str,
    _relative_path: &str,
    module_path: Option<&str>,
) -> Option<String> {
    let module = module_path?;
    let sub_path = import_path.strip_prefix(module)?.strip_prefix('/')?;
    if sub_path.is_empty() {
        return None;
    }
    // Return the directory path — structural.rs will resolve to actual .go files
    Some(sub_path.to_string())
}

/// Read the module path from go.mod in the repo root.
fn read_go_mod_module(repo_path: &Path) -> Option<String> {
    let go_mod_path = repo_path.join("go.mod");
    let contents = std::fs::read_to_string(go_mod_path).ok()?;
    for line in contents.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("module ") {
            return Some(rest.trim().to_string());
        }
    }
    None
}

/// Compute cyclomatic complexity for Go code.
fn compute_complexity(node: tree_sitter::Node<'_>) -> u32 {
    let mut decisions = 0;
    fn walk(node: tree_sitter::Node<'_>, decisions: &mut u32) {
        match node.kind() {
            "if_statement" | "for_statement" => {
                *decisions += 1;
            }
            "expression_case" | "type_case" | "communication_case" | "default_case" => {
                *decisions += 1;
            }
            "binary_expression" => {
                // In tree-sitter-go, logical operators are their own node kinds
                let mut op_cursor = node.walk();
                for op_child in node.children(&mut op_cursor) {
                    if op_child.kind() == "&&" || op_child.kind() == "||" {
                        *decisions += 1;
                    }
                }
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
