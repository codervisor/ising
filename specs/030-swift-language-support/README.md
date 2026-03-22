---
status: planned
created: 2026-03-22
priority: medium
tags:
- language-support
- tree-sitter
- swift
depends_on:
- 001-rust-core
- 019-rust-language-support
---
# Swift Language Support

> **Status**: planned · **Priority**: medium · **Created**: 2026-03-22

## Overview

Swift is the primary language for Apple platform development (iOS, macOS, watchOS, tvOS) and is growing as a server-side language (Vapor). iOS codebases are often large, with UIKit/SwiftUI layers creating complex dependency graphs.

This spec adds Swift support via `tree-sitter-swift`. Swift's type system (classes, structs, enums, protocols, extensions) maps to Ising's model, with extensions requiring special attention.

## Design

### Layer 1: Structural Graph

#### Swift-Specific Node Types

| Tree-sitter node | Ising concept | Notes |
|---|---|---|
| `class_declaration` | Class | Class definitions |
| `struct_declaration` | Class | Structs (primary value type, carries methods) |
| `enum_declaration` | Class | Enums (can carry associated values and methods) |
| `protocol_declaration` | Class | Protocols (like interfaces/traits) |
| `function_declaration` (top-level) | Function | Free functions |
| `function_declaration` (in type) | Function | Methods, attributed as `TypeName::method` |
| `init_declaration` | Function | Initializers, attributed as `TypeName::init` |
| `import_declaration` | Import | `import Module` — framework/package imports |

#### Extensions

Swift extensions are a core challenge:

```swift
extension UserService {
    func validate() { ... }
}

extension UserService: Codable {
    // conformance methods
}
```

Extensions add methods to types defined elsewhere (possibly in other files or even other modules). The tree-sitter node is `extension_declaration`.

Strategy:
- Extract methods inside extensions and attribute them to the extended type: `File.swift::UserService::validate`
- The extension block itself is not a separate node — it's a grouping mechanism (same as Rust `impl` blocks)
- If the extended type is defined in the same project, coupling edges form naturally through method attribution
- If the extended type is external (e.g., `extension String`), the methods still belong to the file's module node — coupling is tracked at the file level

#### Import Resolution

Swift imports are module-level, not file-level:

- `import UIKit`, `import Foundation`, `import SwiftUI` — framework imports, skip
- `import MyAppCore` — could be an internal module in a multi-target project

Swift Package Manager (SPM) projects define targets in `Package.swift`. Each target is a module. However, within a single target, there are no explicit imports between files — all files in a target are implicitly visible to each other.

**This means Swift has no intra-module import edges.** Files within the same target don't import each other. Cross-target imports (`import MyAppCore`) could be resolved by reading `Package.swift`, but this is low value — the change graph (Layer 2) captures the real coupling.

Resolution: Skip import edge resolution entirely for Swift. Rely on Layer 2 (change graph) for coupling detection. This is honest — Swift's visibility model makes static import analysis largely useless within a single module.

#### Challenges

- **No intra-module imports**: Unlike Python/Java/Go, Swift files don't import each other within a module. All coupling is implicit. This is the biggest gap — Layer 1 structural graph will have module→function/class edges but almost no import edges. **The change graph is essential for Swift.**
- **`@objc` and dynamic dispatch**: Objective-C interop attributes create coupling invisible to tree-sitter. Accept this gap.
- **Property wrappers** (`@State`, `@Published`, `@EnvironmentObject`): These are SwiftUI-specific and create data flow coupling. Not extractable via tree-sitter AST alone.
- **Xcode project structure**: iOS projects use `.xcodeproj` or `.xcworkspace` to organize targets. Ising doesn't parse these — it walks the file system. This works because source files are still `.swift` files in directories.
- **Multiple `init` declarations**: Swift types often have multiple initializers. Deduplicate: `init`, `init_2`, etc. (same approach as Go's `init()`).

#### Complexity for Swift

| Tree-sitter node | Reason |
|---|---|
| `if_statement` | Conditional branch |
| `guard_statement` | Early exit guard (is a branch) |
| `for_statement` | For-in loop |
| `while_statement` | While loop |
| `repeat_while_statement` | Repeat-while loop |
| `switch_case` | Each case in switch |
| `catch_keyword` / `catch_pattern` | Each catch clause in do-catch |
| `binary_expression` with `&&` / `\|\|` | Logical branching |
| `ternary_expression` | Conditional `?:` |
| `optional_chaining_expression` | `?.` chains (debatable — see notes) |

Base complexity = 1 + count of above. **Exclude** `?.` optional chaining initially — revisit if complexity scores seem low for Swift code.

### Layer 2: Change Graph

Add `.swift` to `Language::from_extension` and `supported_extensions()`.

## Plan

- [ ] Add `tree-sitter-swift` to workspace and crate dependencies
- [ ] Add `Language::Swift` variant to `Language` enum
  - `from_extension`: `"swift"` → `Language::Swift`
  - `name()`: returns `"swift"`
- [ ] Create `ising-builders/src/languages/swift.rs` with `extract_nodes()`
  - Walk for `class_declaration`, `struct_declaration`, `enum_declaration`, `protocol_declaration`, `extension_declaration`, `function_declaration`
  - Walk type bodies for `function_declaration` and `init_declaration`
  - Handle extensions: attribute methods to the extended type name
  - Skip import resolution (no intra-module imports)
- [ ] Implement `compute_complexity` for Swift (including `guard`)
- [ ] Wire up in `languages/mod.rs`, `structural.rs` dispatch, and `get_tree_sitter_language`
- [ ] Unit tests: classes, structs, enums, protocols, extensions, free functions, guard complexity
- [ ] Integration test: run on a Swift project

## Test

- [ ] `.swift` files detected with `Language::Swift`
- [ ] `func helper()` at top level → `Function` node named `helper`
- [ ] `class ViewController` → `Class` node named `ViewController`
- [ ] `struct User` → `Class` node named `User`
- [ ] `enum State` → `Class` node named `State`
- [ ] `protocol Repository` → `Class` node named `Repository`
- [ ] `func process()` inside `UserService` → `Function` node `File.swift::UserService::process`
- [ ] `extension UserService { func validate() }` → `Function` node `File.swift::UserService::validate`
- [ ] `init(name: String)` inside `User` → `Function` node `File.swift::User::init`
- [ ] Import edges: none expected within a module (Swift has no intra-module imports)
- [ ] Complexity: function with `guard`, `if`, `switch` (3 cases) → 1 + 1 + 1 + 3 = 6
- [ ] No regression on existing language tests

## Notes

- **Layer 2 is critical for Swift**: More than any other supported language, Swift relies on the change graph for coupling detection. The structural graph provides node extraction (functions, classes) and containment edges, but import edges are nearly absent. This is a known limitation, not a bug — Swift's module visibility model simply doesn't have file-level imports to extract.
- **`guard` as complexity**: `guard` is syntactically an early-exit check, but it is a branch (the `else` block runs on failure). Counting it is correct.
- **`tree-sitter-swift` maturity**: The grammar is community-maintained and covers Swift 5.x. Swift 6 concurrency features (`actor`, `async`/`await`, `Sendable`) may not be fully represented. Test with modern Swift codebases and file issues upstream if gaps are found.
- **Optional chaining exclusion**: `?.` is extremely common in Swift (more so than `&&`/`||`). Counting it as complexity would inflate scores for idiomatic Swift code. Exclude initially; add only if Swift projects consistently score lower than expected.
- **Actors**: Swift's `actor` type (concurrency primitive) should be extracted as a Class node if `tree-sitter-swift` represents it as a distinct node type. If it falls under `class_declaration` with a modifier, no additional handling is needed.
