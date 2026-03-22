//! Vue Single File Component (.vue) extraction via two-pass parsing.
//!
//! Strategy: Parse the `.vue` file to locate the `<script>` or `<script setup>` block,
//! extract its raw text content, then re-parse with tree-sitter-typescript and apply
//! the existing TypeScript extractor with a line offset adjustment.
//!
//! This avoids depending on a tree-sitter-vue grammar (which is incompatible with
//! tree-sitter 0.24) and only requires finding well-structured `<script>` tags.

use super::{ClassInfo, FunctionInfo, ImportInfo};
use regex::Regex;
use std::sync::LazyLock;

/// Regex to match `<script>`, `<script setup>`, `<script lang="ts">`, etc.
/// Captures the attributes (group 1) and the content (group 2).
static SCRIPT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?si)<script([^>]*)>(.*?)</script>").unwrap()
});

/// Regex to detect `lang="ts"` or `lang='ts'` in script tag attributes.
static LANG_TS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)lang\s*=\s*["']ts["']"#).unwrap()
});

/// Information about an extracted `<script>` block.
struct ScriptBlock {
    /// The raw text content of the script block.
    content: String,
    /// 0-based line number where the script content starts in the .vue file.
    start_line: u32,
    /// Whether the script uses TypeScript (`lang="ts"`).
    is_typescript: bool,
}

/// Extract the `<script>` block from a Vue SFC source.
fn extract_script_block(source: &str) -> Option<ScriptBlock> {
    let captures = SCRIPT_RE.captures(source)?;

    let attrs = captures.get(1).map(|m| m.as_str()).unwrap_or("");
    let content_match = captures.get(2)?;
    let content = content_match.as_str().to_string();

    // Calculate the line number where the content starts
    let byte_offset = content_match.start();
    let start_line = source[..byte_offset].matches('\n').count() as u32;

    let is_typescript = LANG_TS_RE.is_match(attrs);

    Some(ScriptBlock {
        content,
        start_line,
        is_typescript,
    })
}

/// Extract functions, classes, and imports from a Vue SFC file.
///
/// Two-pass approach:
/// 1. Find the `<script>` block and extract its content + line offset
/// 2. Re-parse the content with tree-sitter-typescript and apply the TS extractor
/// 3. Additionally extract `@/` alias imports (which the TS extractor skips)
pub fn extract_nodes(
    source: &str,
    functions: &mut Vec<FunctionInfo>,
    classes: &mut Vec<ClassInfo>,
    imports: &mut Vec<ImportInfo>,
) {
    let block = match extract_script_block(source) {
        Some(b) => b,
        None => return, // No script block — nothing to extract
    };

    // Choose the appropriate tree-sitter language
    let ts_lang: tree_sitter::Language = if block.is_typescript {
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
    } else {
        // Default to TypeScript parser even for JS — it's a superset
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
    };

    let mut parser = tree_sitter::Parser::new();
    if parser.set_language(&ts_lang).is_err() {
        return;
    }

    let tree = match parser.parse(&block.content, None) {
        Some(t) => t,
        None => return,
    };

    // Apply the TypeScript extractor with line offset
    super::typescript::extract_nodes_with_offset(
        tree.root_node(),
        &block.content,
        functions,
        classes,
        imports,
        block.start_line,
    );

    // The TS extractor only captures relative imports (starting with '.').
    // For Vue files, also extract @/ alias imports by walking the AST again.
    extract_at_alias_imports(tree.root_node(), &block.content, imports);
}

/// Extract `@/` alias imports from a parsed script block.
fn extract_at_alias_imports(
    node: tree_sitter::Node<'_>,
    source: &str,
    imports: &mut Vec<ImportInfo>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "import_statement" {
            if let Some(source_node) = child.child_by_field_name("source") {
                let import_path = source_node
                    .utf8_text(source.as_bytes())
                    .unwrap_or("")
                    .trim_matches(|c| c == '\'' || c == '"')
                    .to_string();
                if import_path.starts_with("@/") {
                    imports.push(ImportInfo {
                        source: import_path,
                    });
                }
            }
        }
    }
}

/// Resolve a Vue import path.
///
/// - Relative imports (starting with `.`): strip `./` prefix, keep `.vue` extension
/// - `@/` alias: resolve to `src/` if a Vue/Vite config file exists
pub fn resolve_vue_import(import_path: &str, has_vue_config: bool) -> Option<String> {
    if import_path.starts_with('.') {
        let normalized = import_path.trim_start_matches("./").to_string();
        Some(normalized)
    } else if import_path.starts_with("@/") && has_vue_config {
        let resolved = import_path.replacen("@/", "src/", 1);
        Some(resolved)
    } else {
        None // External package
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_script_block_ts() {
        let source = r#"<template>
  <div>{{ msg }}</div>
</template>

<script setup lang="ts">
import { ref } from 'vue'
const msg = ref('Hello')
</script>

<style scoped>
div { color: red; }
</style>"#;

        let block = extract_script_block(source).unwrap();
        assert!(block.is_typescript);
        assert_eq!(block.start_line, 4);
        assert!(block.content.contains("import { ref } from 'vue'"));
        assert!(block.content.contains("const msg = ref('Hello')"));
    }

    #[test]
    fn test_extract_script_block_js_default() {
        let source = r#"<template><div /></template>
<script>
export default {
  data() { return {} }
}
</script>"#;

        let block = extract_script_block(source).unwrap();
        assert!(!block.is_typescript);
        assert_eq!(block.start_line, 1);
    }

    #[test]
    fn test_extract_script_block_none() {
        let source = "<template><div /></template>\n<style>.x {}</style>";
        assert!(extract_script_block(source).is_none());
    }

    #[test]
    fn test_extract_vue_nodes_arrow_functions() {
        let source = r#"<template>
  <div>{{ count }}</div>
</template>

<script setup lang="ts">
import { ref } from 'vue'
import MyChild from './MyChild.vue'

const count = ref(0)
const handleClick = () => count.value++
</script>"#;

        let mut functions = Vec::new();
        let mut classes = Vec::new();
        let mut imports = Vec::new();
        extract_nodes(source, &mut functions, &mut classes, &mut imports);

        // handleClick is an arrow function
        let names: Vec<&str> = functions.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"handleClick"), "Expected handleClick, got {:?}", names);

        // Relative import should be extracted
        let import_sources: Vec<&str> = imports.iter().map(|i| i.source.as_str()).collect();
        assert!(import_sources.contains(&"MyChild.vue"), "Expected MyChild.vue import, got {:?}", import_sources);

        // Line numbers should be offset by the script block start
        let handle_click = functions.iter().find(|f| f.name == "handleClick").unwrap();
        assert_eq!(handle_click.line_start, 10, "handleClick should be at line 10 of the .vue file");
    }

    #[test]
    fn test_extract_vue_nodes_options_api() {
        let source = r#"<template><div /></template>
<script>
export default function setup() {
  return {}
}
</script>"#;

        let mut functions = Vec::new();
        let mut classes = Vec::new();
        let mut imports = Vec::new();
        extract_nodes(source, &mut functions, &mut classes, &mut imports);

        let names: Vec<&str> = functions.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"setup"), "Expected setup function, got {:?}", names);
    }

    #[test]
    fn test_resolve_vue_import_relative() {
        assert_eq!(
            resolve_vue_import("./Foo.vue", false),
            Some("Foo.vue".to_string())
        );
        assert_eq!(
            resolve_vue_import("../components/Bar.vue", false),
            Some("../components/Bar.vue".to_string())
        );
    }

    #[test]
    fn test_resolve_vue_import_alias() {
        assert_eq!(
            resolve_vue_import("@/components/Button.vue", true),
            Some("src/components/Button.vue".to_string())
        );
        // Without config, @/ is not resolved
        assert_eq!(resolve_vue_import("@/components/Button.vue", false), None);
    }

    #[test]
    fn test_resolve_vue_import_external() {
        assert_eq!(resolve_vue_import("vue", false), None);
        assert_eq!(resolve_vue_import("pinia", false), None);
    }
}
