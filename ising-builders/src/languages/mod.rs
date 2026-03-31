//! Per-language AST extraction via Tree-sitter.
//!
//! Each language implements extraction of functions, classes, and imports
//! from its Tree-sitter parse tree.

pub mod csharp;
pub mod go;
pub mod java;
pub mod python;
pub mod rust_lang;
pub mod typescript;
pub mod vue;

pub mod c_lang;
pub mod cpp;
pub mod kotlin;
pub mod php;
pub mod ruby;

/// Result of analyzing a single file.
#[derive(Debug)]
pub struct FileAnalysis {
    /// Module node ID (file path).
    pub module_id: String,
    /// File path.
    pub file_path: String,
    /// Language detected.
    pub language: String,
    /// Lines of code in the file.
    pub loc: u32,
    /// Functions found in the file.
    pub functions: Vec<FunctionInfo>,
    /// Classes found in the file.
    pub classes: Vec<ClassInfo>,
    /// Imports found in the file.
    pub imports: Vec<ImportInfo>,
}

/// A function call found inside a function body.
#[derive(Debug, Clone)]
pub struct CallInfo {
    /// Raw callee name as written in source: "foo", "self.bar", "pkg.Func".
    pub callee: String,
    /// Line number of the call site.
    pub line: u32,
}

#[derive(Debug)]
pub struct FunctionInfo {
    pub name: String,
    pub line_start: u32,
    pub line_end: u32,
    pub complexity: u32,
    /// Function calls made from within this function body.
    pub calls: Vec<CallInfo>,
    /// Whether this function is marked as deprecated.
    pub deprecated: bool,
}

#[derive(Debug)]
pub struct ClassInfo {
    pub name: String,
    pub line_start: u32,
    pub line_end: u32,
    pub complexity: u32,
    /// Whether this class is marked as deprecated.
    pub deprecated: bool,
}

#[derive(Debug)]
pub struct ImportInfo {
    /// The module being imported from (resolved to a relative path if possible).
    pub source: String,
}
