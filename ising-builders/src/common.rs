//! Shared types for the builder modules.
//!
//! Contains the `Language` enum and file extension utilities used by both
//! the structural and change builders.

use std::path::Path;

/// Supported languages for analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Python,
    TypeScript,
    JavaScript,
    Rust,
    Go,
}

impl Language {
    /// Detect language from a file extension.
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext {
            "py" => Some(Language::Python),
            "ts" | "tsx" => Some(Language::TypeScript),
            "js" | "jsx" => Some(Language::JavaScript),
            "rs" => Some(Language::Rust),
            "go" => Some(Language::Go),
            _ => None,
        }
    }

    /// Check if a file extension is a supported source language.
    pub fn is_supported_extension(ext: &str) -> bool {
        Self::from_extension(ext).is_some()
    }

    /// Check if a file path has a supported source code extension.
    pub fn is_supported_file(path: &str) -> bool {
        Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(Self::is_supported_extension)
    }

    /// Human-readable language name.
    pub fn name(&self) -> &'static str {
        match self {
            Language::Python => "python",
            Language::TypeScript => "typescript",
            Language::JavaScript => "javascript",
            Language::Rust => "rust",
            Language::Go => "go",
        }
    }

    /// All supported file extensions.
    pub fn supported_extensions() -> &'static [&'static str] {
        &["py", "ts", "tsx", "js", "jsx", "rs", "go"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_extension() {
        assert_eq!(Language::from_extension("py"), Some(Language::Python));
        assert_eq!(Language::from_extension("ts"), Some(Language::TypeScript));
        assert_eq!(Language::from_extension("tsx"), Some(Language::TypeScript));
        assert_eq!(Language::from_extension("rs"), Some(Language::Rust));
        assert_eq!(Language::from_extension("md"), None);
    }

    #[test]
    fn test_is_supported_file() {
        assert!(Language::is_supported_file("src/main.rs"));
        assert!(Language::is_supported_file("app.py"));
        assert!(Language::is_supported_file("index.ts"));
        assert!(!Language::is_supported_file("readme.md"));
        assert!(!Language::is_supported_file("Cargo.toml"));
    }

    #[test]
    fn test_supported_extensions() {
        let exts = Language::supported_extensions();
        assert!(exts.contains(&"py"));
        assert!(exts.contains(&"rs"));
        assert!(exts.contains(&"ts"));
    }
}
