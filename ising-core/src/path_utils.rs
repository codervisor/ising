//! Path classification utilities shared across crates.

/// Check if a path belongs to a test file based on common naming conventions.
pub fn is_test_file(path: &str) -> bool {
    let filename = path.rsplit('/').next().unwrap_or(path);
    filename.starts_with("test_")
        || filename.starts_with("tests_")
        || filename.ends_with("_test.py")
        || filename.ends_with("_test.rs")
        || filename.ends_with("_test.go")
        || filename.ends_with(".test.ts")
        || filename.ends_with(".test.js")
        || filename.ends_with(".test.tsx")
        || filename.ends_with(".test.jsx")
        || filename.ends_with(".spec.ts")
        || filename.ends_with(".spec.js")
        || path.contains("/tests/")
        || path.contains("/test/")
        || path.starts_with("tests/")
        || path.starts_with("test/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_detection() {
        assert!(is_test_file("test_basic.py"));
        assert!(is_test_file("tests/test_foo.py"));
        assert!(is_test_file("src/tests/helper.rs"));
        assert!(is_test_file("app.test.ts"));
        assert!(is_test_file("component.spec.js"));
        assert!(is_test_file("foo_test.py"));
        assert!(is_test_file("bar_test.go"));
        assert!(!is_test_file("src/main.py"));
        assert!(!is_test_file("src/utils.rs"));
    }
}
