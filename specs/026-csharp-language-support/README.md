---
status: planned
created: 2026-03-22
priority: high
tags:
- language-support
- tree-sitter
- csharp
depends_on:
- 001-rust-core
- 019-rust-language-support
- 025-java-language-support
---
# C# Language Support

> **Status**: planned · **Priority**: high · **Created**: 2026-03-22

## Overview

C# is the dominant language in the .NET ecosystem, powering enterprise backends, game development (Unity), and cloud services. C# codebases are often large and long-lived — exactly the profile where coupling analysis provides the most value.

This spec adds C# support via `tree-sitter-c-sharp`. C#'s class-centric model is structurally similar to Java (spec 025), so most design decisions carry over directly.

## Design

### Layer 1: Structural Graph

#### C#-Specific Node Types

| Tree-sitter node | Ising concept | Notes |
|---|---|---|
| `class_declaration` | Class | Standard classes |
| `interface_declaration` | Class | Interface definitions |
| `struct_declaration` | Class | Value types (C# structs carry methods, unlike C) |
| `enum_declaration` | Class | Enum type definitions |
| `record_declaration` | Class | C# 9+ record types |
| `method_declaration` | Function | Methods inside classes, attributed as `ClassName::Method` |
| `constructor_declaration` | Function | Constructors, attributed as `ClassName::ClassName` |
| `using_directive` | Import | `using Namespace.SubNamespace` — intra-project only |

#### Method Attribution

C# methods always live inside a type declaration. Attribution follows the Java pattern:

- `MyClass.Process()` → `File.cs::MyClass::Process`
- Nested class: `Outer.Inner.Method()` → `File.cs::Outer::Inner::Method`
- Constructors: `File.cs::MyClass::MyClass`

#### Import Resolution

C# uses `using` directives for namespace imports:

1. **Framework namespaces** (`System.*`, `Microsoft.*`) — skip.
2. **Third-party** (`Newtonsoft.Json.*`, etc.) — skip unless matches project namespace.
3. **Intra-project** — namespaces matching the project's root namespace.

Resolution strategy:
- C# namespaces don't map 1:1 to file paths (unlike Java). A namespace `MyApp.Services` could live in any directory.
- **Heuristic**: Convert namespace to path (`MyApp.Services.UserService` → `MyApp/Services/UserService.cs`) and check common source roots. This works for projects following the convention of matching namespace to directory structure (which most do).
- Fall back: if the heuristic fails, scan existing module nodes for files declaring a matching namespace.

#### Challenges

- **Partial classes**: C# allows splitting a class across multiple files (`partial class Foo`). Each file should be analyzed independently — they'll produce separate module nodes with methods attributed to the same class name prefix. This is correct for coupling analysis: a change in one partial file doesn't necessarily affect the other.
- **Top-level statements** (C# 9+): Files can contain statements without a class wrapper. These appear as direct children of `compilation_unit`. Extract as functions named after the file (e.g., `Program::Main`).
- **Properties**: `property_declaration` nodes contain `get`/`set` accessors. These are _not_ extracted as separate function nodes — properties are too granular. They contribute to the enclosing class's complexity score only.
- **`global using`** (C# 10+): `global using` directives apply project-wide. These are ignored — they create implicit edges everywhere, which would add noise without signal.

#### Complexity for C#

| Tree-sitter node | Reason |
|---|---|
| `if_statement` | Conditional branch |
| `for_statement` | For loop |
| `for_each_statement` | Foreach loop |
| `while_statement` | While loop |
| `do_statement` | Do-while loop |
| `catch_clause` | Exception handler |
| `case_switch_label` / `case_pattern_switch_label` | Each case in switch |
| `binary_expression` with `&&` / `\|\|` | Logical branching |
| `conditional_expression` | Ternary `?:` |
| `switch_expression_arm` | C# 8+ switch expression arms |

Base complexity = 1 + count of above.

### Layer 2: Change Graph

Add `.cs` to `Language::from_extension` and `supported_extensions()`.

## Plan

- [ ] Add `tree-sitter-c-sharp` to workspace and crate dependencies
- [ ] Add `Language::CSharp` variant to `Language` enum
  - `from_extension`: `"cs"` → `Language::CSharp`
  - `name()`: returns `"csharp"`
- [ ] Create `ising-builders/src/languages/csharp.rs` with `extract_nodes()`
  - Walk for class/interface/struct/enum/record declarations
  - Walk class bodies for method and constructor declarations
  - Handle nested types with full path attribution
  - Handle top-level statements (C# 9+)
  - Resolve `using` directives to file paths via namespace-to-path heuristic
- [ ] Implement `compute_complexity` for C#
- [ ] Wire up in `languages/mod.rs`, `structural.rs` dispatch, and `get_tree_sitter_language`
- [ ] Unit tests: classes, interfaces, methods, constructors, nested types, partial classes
- [ ] Integration test: run on a C# project

## Test

- [ ] `.cs` files detected with `Language::CSharp`
- [ ] `class UserService {}` → `Class` node named `UserService`
- [ ] `interface IRepository {}` → `Class` node named `IRepository`
- [ ] `struct Point {}` → `Class` node named `Point`
- [ ] `void Process()` inside `UserService` → `Function` node `File.cs::UserService::Process`
- [ ] Nested `class Inner` inside `Outer` → `Class` node `File.cs::Outer::Inner`
- [ ] `using MyApp.Models;` with matching project file → `Imports` edge
- [ ] `using System.Linq;` → no edge (framework namespace)
- [ ] Complexity: method with `if`, `foreach`, `catch`, `&&` → 1 + 1 + 1 + 1 + 1 = 5
- [ ] No regression on existing language tests

## Notes

- **Similarity to Java**: The extraction logic is ~80% shared with spec 025. Consider extracting a shared `class_based_extractor` helper if both are implemented — but only after both work independently (no premature abstraction).
- **Namespace vs directory**: Unlike Java, C# does not enforce namespace-to-directory mapping. The heuristic works for ~90% of real projects. For the remaining 10%, missing import edges are acceptable — coupling analysis is probabilistic, not exact.
- **Unity projects**: Unity C# files often live in `Assets/Scripts/`. The walker already handles arbitrary directory structures, so no special case needed.
- **`.csx` script files**: C# script files are rare and have different semantics. Not supported in this spec.
