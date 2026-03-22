---
status: planned
created: 2026-03-22
priority: medium
tags:
- language-support
- tree-sitter
- kotlin
depends_on:
- 001-rust-core
- 019-rust-language-support
- 025-java-language-support
---
# Kotlin Language Support

> **Status**: planned · **Priority**: medium · **Created**: 2026-03-22

## Overview

Kotlin is the primary language for Android development and is increasingly used for backend services (Ktor, Spring Boot). Kotlin runs on the JVM and shares Java's package/import model, but adds its own syntax: data classes, sealed classes, extension functions, coroutines, and top-level functions.

This spec adds Kotlin support via `tree-sitter-kotlin`. The extraction model closely follows Java (spec 025) with additions for Kotlin-specific constructs.

## Design

### Layer 1: Structural Graph

#### Kotlin-Specific Node Types

| Tree-sitter node | Ising concept | Notes |
|---|---|---|
| `class_declaration` | Class | Regular classes, data classes, sealed classes |
| `object_declaration` | Class | Kotlin singletons (`object Foo {}`) |
| `interface_declaration` | Class | Interface definitions |
| `enum_class_body` parent: `class_declaration` | Class | Enum classes |
| `function_declaration` (top-level) | Function | Top-level functions (Kotlin allows this, unlike Java) |
| `function_declaration` (in class) | Function | Methods, attributed as `ClassName::method` |
| `import_header` | Import | `import com.foo.Bar` — intra-project only |

#### Key Differences from Java

1. **Top-level functions**: Kotlin allows `fun helper()` at file scope without a class. Extract as a standalone Function node (like Python).
2. **Extension functions**: `fun String.isEmail(): Boolean` — extract as a Function node named `String.isEmail`. The receiver type is part of the name for clarity.
3. **Object declarations**: Kotlin singletons (`object Config {}`) are extracted as Class nodes.
4. **Companion objects**: `companion object { fun create() }` — methods inside are attributed to the enclosing class: `MyClass::create`.
5. **Data classes / sealed classes**: Same AST node (`class_declaration`) with modifiers. Extract as Class nodes — the modifier doesn't affect coupling analysis.

#### Import Resolution

Kotlin imports follow the Java model. Same resolution strategy as spec 025:
- Convert qualified name to path: `com.example.service.UserService` → `src/main/kotlin/com/example/service/UserService.kt`
- Check source roots: `src/main/kotlin/`, `src/main/java/` (Kotlin can coexist), `src/`
- Skip standard library (`kotlin.*`, `java.*`) and third-party imports

#### Challenges

- **Kotlin Multiplatform (KMP)**: Projects may have `commonMain/`, `androidMain/`, `iosMain/` source sets. Each is a separate source root. The walker should find `.kt` files in all directories — no special handling needed beyond what the recursive walker already does.
- **Coroutines**: `suspend fun` are regular functions with a modifier. No special extraction needed.
- **DSL builders**: Kotlin's type-safe builders (e.g., Ktor routing, Compose UI) create deeply nested lambda structures that look complex but aren't traditional coupling. These lambdas are not extracted as function nodes — they contribute only to the enclosing function's complexity.
- **`typealias`**: Type aliases don't create nodes — they're just naming conveniences.
- **`when` expressions**: Kotlin's `when` is like Rust's `match`/Java's `switch`. Each branch is a decision point for complexity.

#### Complexity for Kotlin

| Tree-sitter node | Reason |
|---|---|
| `if_expression` | Conditional (also used as expression) |
| `for_statement` | For loop |
| `while_statement` | While loop |
| `do_while_statement` | Do-while loop |
| `when_entry` | Each branch in `when` expression |
| `catch_block` | Exception handler |
| `binary_expression` with `&&` / `\|\|` | Logical branching |
| `elvis_expression` (`?:`) | Null-coalescing (is a branch) |

Base complexity = 1 + count of above.

### Layer 2: Change Graph

Add `.kt` and `.kts` to `Language::from_extension` and `supported_extensions()`.

## Plan

- [ ] Add `tree-sitter-kotlin` to workspace and crate dependencies
- [ ] Add `Language::Kotlin` variant to `Language` enum
  - `from_extension`: `"kt"` / `"kts"` → `Language::Kotlin`
  - `name()`: returns `"kotlin"`
- [ ] Create `ising-builders/src/languages/kotlin.rs` with `extract_nodes()`
  - Walk for `class_declaration`, `object_declaration`, `interface_declaration`, `function_declaration`
  - Walk class bodies for methods, including companion object methods
  - Handle extension functions (include receiver type in name)
  - Resolve `import_header` directives via Java-style path resolution
- [ ] Implement `compute_complexity` for Kotlin (including `when` and `?:`)
- [ ] Wire up in `languages/mod.rs`, `structural.rs` dispatch, and `get_tree_sitter_language`
- [ ] Unit tests: classes, objects, top-level functions, extension functions, methods, when expressions
- [ ] Integration test: run on a Kotlin project

## Test

- [ ] `.kt` and `.kts` files detected with `Language::Kotlin`
- [ ] `fun helper()` at top level → `Function` node named `helper`
- [ ] `fun String.isEmail()` → `Function` node named `String.isEmail`
- [ ] `class UserService` → `Class` node named `UserService`
- [ ] `object Config` → `Class` node named `Config`
- [ ] `data class User(val name: String)` → `Class` node named `User`
- [ ] `fun process()` inside `UserService` → `Function` node `File.kt::UserService::process`
- [ ] `companion object { fun create() }` inside `MyClass` → `Function` node `File.kt::MyClass::create`
- [ ] `import com.example.model.User` with matching file → `Imports` edge
- [ ] `import kotlin.collections.List` → no edge (stdlib)
- [ ] Complexity: function with `if`, `when` (3 entries), `?:` → 1 + 1 + 3 + 1 = 6
- [ ] No regression on existing language tests

## Notes

- **Kotlin + Java coexistence**: Many projects mix Kotlin and Java files. Cross-language imports (Kotlin importing a Java class) should resolve correctly since both use the same package-to-path convention. A `.kt` file importing `com.example.Model` should create an edge to `Model.java` if it exists as a module node.
- **`tree-sitter-kotlin` maturity**: The grammar is maintained but less battle-tested than `tree-sitter-java`. Test with real-world projects (e.g., a Ktor app or Android project) to verify edge cases.
- **Elvis operator as complexity**: `?:` is debatable — it's a null check, not a traditional branch. Including it is conservative. Can be removed if it inflates complexity scores unreasonably in practice.
- **Script files (`.kts`)**: Gradle build scripts (`build.gradle.kts`) and Kotlin scripts are included. They contain top-level code and function declarations. Useful for detecting build-script coupling in large Gradle projects.
