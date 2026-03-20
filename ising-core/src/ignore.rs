//! `.isingignore` file support.
//!
//! Syntax (subset of `.gitignore`):
//! - Blank lines and lines starting with `#` are ignored
//! - Patterns are matched against relative file paths
//! - `*` matches anything except `/`
//! - `**` matches any number of directories
//! - Trailing `/` matches directories (treated as prefix match)
//! - `!` prefix negates a pattern (re-includes a previously excluded path)

use std::path::Path;

/// A compiled set of ignore patterns from a `.isingignore` file.
#[derive(Debug, Clone)]
pub struct IgnoreRules {
    patterns: Vec<IgnorePattern>,
}

#[derive(Debug, Clone)]
struct IgnorePattern {
    /// The glob pattern converted to a regex.
    regex: regex::Regex,
    /// Whether this is a negation pattern (starts with `!`).
    negated: bool,
}

impl IgnoreRules {
    /// Load ignore rules from a `.isingignore` file in the given directory.
    /// Returns empty rules if the file doesn't exist.
    pub fn load(repo_path: &Path) -> Self {
        let ignore_path = repo_path.join(".isingignore");
        match std::fs::read_to_string(&ignore_path) {
            Ok(content) => Self::parse(&content),
            Err(_) => Self { patterns: vec![] },
        }
    }

    /// Parse ignore rules from a string.
    pub fn parse(content: &str) -> Self {
        let patterns = content
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .filter_map(|line| {
                let (pattern, negated) = if let Some(rest) = line.strip_prefix('!') {
                    (rest.trim(), true)
                } else {
                    (line, false)
                };

                let regex = glob_to_regex(pattern)?;
                Some(IgnorePattern { regex, negated })
            })
            .collect();

        Self { patterns }
    }

    /// Check if a relative path should be ignored.
    pub fn is_ignored(&self, path: &str) -> bool {
        let mut ignored = false;
        for pat in &self.patterns {
            if pat.regex.is_match(path) {
                ignored = !pat.negated;
            }
        }
        ignored
    }

    /// Returns true if no rules are loaded.
    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }
}

/// Convert a glob pattern to a regex.
fn glob_to_regex(pattern: &str) -> Option<regex::Regex> {
    let mut regex = String::from("(?:^|/)");
    let pattern = pattern.strip_prefix('/').unwrap_or(pattern);

    // Trailing slash means directory prefix match
    let pattern = if let Some(p) = pattern.strip_suffix('/') {
        regex = String::from("(?:^|/)");
        let result = translate_glob(p, &mut regex);
        regex.push_str("(?:/|$)");
        return result.and_then(|_| regex::Regex::new(&regex).ok());
    } else {
        pattern
    };

    translate_glob(pattern, &mut regex)?;
    regex.push('$');
    regex::Regex::new(&regex).ok()
}

fn translate_glob(pattern: &str, regex: &mut String) -> Option<()> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '*' => {
                if i + 1 < chars.len() && chars[i + 1] == '*' {
                    // ** matches any path segments
                    regex.push_str(".*");
                    i += 2;
                    // Skip trailing /
                    if i < chars.len() && chars[i] == '/' {
                        i += 1;
                    }
                } else {
                    // * matches within a single segment
                    regex.push_str("[^/]*");
                    i += 1;
                }
            }
            '?' => {
                regex.push_str("[^/]");
                i += 1;
            }
            '.' | '(' | ')' | '{' | '}' | '+' | '|' | '^' | '$' | '[' | ']' => {
                regex.push('\\');
                regex.push(chars[i]);
                i += 1;
            }
            c => {
                regex.push(c);
                i += 1;
            }
        }
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_directory() {
        let rules = IgnoreRules::parse("docs_src/");
        assert!(rules.is_ignored("docs_src/tutorial001.py"));
        assert!(rules.is_ignored("docs_src/sub/file.py"));
        assert!(!rules.is_ignored("src/docs_src.py"));
    }

    #[test]
    fn test_glob_pattern() {
        let rules = IgnoreRules::parse("tests/test_tutorial/**");
        assert!(rules.is_ignored("tests/test_tutorial/test_body/test_tutorial001.py"));
        assert!(!rules.is_ignored("tests/test_basic.py"));
    }

    #[test]
    fn test_wildcard() {
        let rules = IgnoreRules::parse("*_py310.py");
        assert!(rules.is_ignored("docs_src/tutorial001_py310.py"));
        assert!(rules.is_ignored("deep/path/file_py310.py"));
        assert!(!rules.is_ignored("docs_src/tutorial001.py"));
    }

    #[test]
    fn test_negation() {
        let rules = IgnoreRules::parse("tests/\n!tests/test_basic.py");
        assert!(rules.is_ignored("tests/test_tutorial/foo.py"));
        assert!(!rules.is_ignored("tests/test_basic.py"));
    }

    #[test]
    fn test_comments_and_blanks() {
        let rules = IgnoreRules::parse("# comment\n\ndocs_src/\n");
        assert_eq!(rules.patterns.len(), 1);
        assert!(rules.is_ignored("docs_src/foo.py"));
    }

    #[test]
    fn test_empty() {
        let rules = IgnoreRules::parse("");
        assert!(rules.is_empty());
        assert!(!rules.is_ignored("anything.py"));
    }
}
