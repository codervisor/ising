//! Path classification utilities shared across crates.

/// Check if a path belongs to a test file based on common naming conventions.
pub fn is_test_file(path: &str) -> bool {
    let filename = path.rsplit('/').next().unwrap_or(path);
    filename.starts_with("test_")
        || filename.starts_with("tests_")
        // Python
        || filename.ends_with("_test.py")
        // Rust
        || filename.ends_with("_test.rs")
        // Go
        || filename.ends_with("_test.go")
        // TypeScript / JavaScript
        || filename.ends_with(".test.ts")
        || filename.ends_with(".test.js")
        || filename.ends_with(".test.tsx")
        || filename.ends_with(".test.jsx")
        || filename.ends_with(".spec.ts")
        || filename.ends_with(".spec.js")
        // Java / Kotlin (JUnit convention: *Test.java, *Tests.java, *Spec.kt)
        || filename.ends_with("Test.java")
        || filename.ends_with("Tests.java")
        || filename.ends_with("IT.java")
        || filename.ends_with("Test.kt")
        || filename.ends_with("Tests.kt")
        || filename.ends_with("Spec.kt")
        // C# (NUnit/xUnit convention: *Tests.cs, *Test.cs)
        || filename.ends_with("Test.cs")
        || filename.ends_with("Tests.cs")
        // PHP (PHPUnit convention: *Test.php)
        || filename.ends_with("Test.php")
        // Ruby (RSpec: *_spec.rb, Minitest: *_test.rb)
        || filename.ends_with("_spec.rb")
        || filename.ends_with("_test.rb")
        // Directory-based test locations
        || path.contains("/tests/")
        || path.contains("/test/")
        || path.starts_with("tests/")
        || path.starts_with("test/")
        || path.contains("/spec/")
        || path.starts_with("spec/")
        || path.contains("/src/test/")
        || path.contains("/__tests__/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_detection() {
        // Python
        assert!(is_test_file("test_basic.py"));
        assert!(is_test_file("tests/test_foo.py"));
        assert!(is_test_file("foo_test.py"));
        // Rust
        assert!(is_test_file("src/tests/helper.rs"));
        // TypeScript / JavaScript
        assert!(is_test_file("app.test.ts"));
        assert!(is_test_file("component.spec.js"));
        // Go
        assert!(is_test_file("bar_test.go"));
        // Java / Kotlin
        assert!(is_test_file("src/test/java/com/example/FooTest.java"));
        assert!(is_test_file("UserServiceTests.java"));
        assert!(is_test_file("IntegrationIT.java"));
        assert!(is_test_file("FooTest.kt"));
        assert!(is_test_file("FooSpec.kt"));
        // C#
        assert!(is_test_file("UserServiceTest.cs"));
        assert!(is_test_file("UserServiceTests.cs"));
        // PHP
        assert!(is_test_file("UserServiceTest.php"));
        // Ruby
        assert!(is_test_file("user_spec.rb"));
        assert!(is_test_file("user_test.rb"));
        assert!(is_test_file("spec/models/user_spec.rb"));
        // Directory patterns
        assert!(is_test_file("src/test/java/Foo.java"));
        assert!(is_test_file("src/__tests__/App.tsx"));
        // Negative cases
        assert!(!is_test_file("src/main.py"));
        assert!(!is_test_file("src/utils.rs"));
        assert!(!is_test_file("src/Contest.java"));
    }
}
