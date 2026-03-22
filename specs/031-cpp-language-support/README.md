---
status: planned
created: 2026-03-22
priority: low
tags:
- language-support
- tree-sitter
- cpp
- research-needed
depends_on:
- 001-rust-core
- 019-rust-language-support
---
# C/C++ Language Support

> **Status**: planned · **Priority**: low · **Created**: 2026-03-22

## Overview

C and C++ power operating systems, embedded systems, game engines, databases, and performance-critical infrastructure. These codebases are often the largest and longest-lived in existence — exactly where coupling analysis would provide immense value. However, C/C++ presents the hardest extraction challenge of any language due to the preprocessor, header file model, and template metaprogramming.

This spec adds C/C++ support via `tree-sitter-c` and `tree-sitter-cpp`. The approach is deliberately conservative: extract what tree-sitter can see, accept the gaps, and rely on the change graph to fill them.

## Design

### Layer 1: Structural Graph

#### C-Specific Node Types

| Tree-sitter node | Ising concept | Notes |
|---|---|---|
| `function_definition` | Function | `void foo() {}` — only definitions, not declarations |
| `struct_specifier` (with body) | Class | `struct Foo { ... };` |
| `enum_specifier` (with body) | Class | `enum Color { ... };` |
| `preproc_include` | Import | `#include "local.h"` — quoted includes only |

#### C++-Specific Node Types (additional)

| Tree-sitter node | Ising concept | Notes |
|---|---|---|
| `class_specifier` | Class | `class Foo { ... };` |
| `function_definition` inside class | Function | Methods, attributed as `ClassName::method` |
| `namespace_definition` | — | Grouping only, not a node. Used for name qualification. |
| `template_declaration` | — | The template itself is not a node; the function/class inside it is |
| `using_declaration` | Import | `using namespace std;` — skip stdlib |

#### Header File Problem

**This is the central challenge.** C/C++ splits declarations (`.h`/`.hpp`) from definitions (`.c`/`.cpp`). The coupling between a header and its implementation file is implicit — there's no `import` statement in the `.cpp` file that says "I implement the interface declared in this `.h` file."

How `#include` works:
- `#include "foo.h"` — project-local header, quoted. Creates a coupling edge.
- `#include <vector>` — system/library header, angle brackets. Skip.

Strategy for `#include` resolution:
- Only resolve quoted includes (`"..."`) — these are project-local.
- Resolve relative to the including file's directory first, then check common include paths (`include/`, `src/`, project root).
- Create an `Imports` edge from the `.c`/`.cpp` file to the resolved `.h`/`.hpp` file.
- The `.h` file is also analyzed as a module node — it may contain inline function definitions, class declarations with methods, etc.

**What we miss**: The preprocessor (`#define`, `#ifdef`, macro expansion) rewrites source code before compilation. Tree-sitter parses the unexpanded source. This means:
- Macros that generate functions/classes are invisible
- Conditional compilation (`#ifdef DEBUG`) creates paths tree-sitter can't evaluate
- Macro-heavy code (common in C) will have incomplete extraction

**Accept this gap.** The change graph provides coupling signals regardless of preprocessor complexity.

#### Method Attribution (C++)

C++ methods can be defined inside the class body or outside:

```cpp
// Inside class body — tree-sitter sees this directly
class Foo {
    void method() { ... }
};

// Outside class body — tree-sitter sees function_definition with qualified name
void Foo::method() { ... }
```

For out-of-class definitions, the function name includes the class qualifier (`Foo::method`). Parse the `::` in the function name to determine attribution.

For in-class definitions, walk the `class_specifier`'s `field_declaration_list` for `function_definition` nodes.

#### Challenges

- **Preprocessor macros**: Cannot be resolved without running the preprocessor. Macros like `DEFINE_TEST(name)` that generate functions are invisible. Tree-sitter represents `#define` as `preproc_def` but doesn't expand macros.
- **Templates**: `template<typename T> class Container { ... }` — the class inside the template is extractable, but template specializations and instantiations create coupling that tree-sitter can't trace.
- **Multiple definitions**: C++ allows the same class to have methods defined across multiple `.cpp` files (each including the header). This is handled naturally — each `.cpp` file becomes a module with methods attributed to the class.
- **Build system complexity**: C/C++ projects use CMake, Make, Bazel, Meson, etc. Each has its own way of specifying include paths. Without parsing the build system, include path resolution is heuristic-based.
- **Include path resolution**: A `#include "util/helper.h"` could resolve differently depending on `-I` flags passed to the compiler. Heuristic: search for the path relative to the including file, then project root, then `include/`, `src/`.
- **`.h` files — C or C++?**: Header files with `.h` extension could be C or C++. Use `tree-sitter-cpp` for `.h` files by default (it's a superset of C). Use `tree-sitter-c` only for `.c` files.
- **Forward declarations**: `class Foo;` (no body) should not create a Class node. Only extract class/struct/enum specifiers that have a body.

#### Complexity for C

| Tree-sitter node | Reason |
|---|---|
| `if_statement` | Conditional branch |
| `for_statement` | For loop |
| `while_statement` | While loop |
| `do_statement` | Do-while loop |
| `case_statement` | Each case in switch |
| `binary_expression` with `&&` / `\|\|` | Logical branching |
| `conditional_expression` | Ternary `?:` |

#### Additional Complexity for C++

| Tree-sitter node | Reason |
|---|---|
| `catch_clause` | Exception handler |
| `for_range_loop` | Range-based for |
| `try_statement` | (counted via catch clauses, not try itself) |

Base complexity = 1 + count of above.

### Layer 2: Change Graph

Add `.c`, `.h`, `.cpp`, `.hpp`, `.cc`, `.cxx`, `.hh`, `.hxx` to `Language::from_extension` and `supported_extensions()`.

Use two language variants: `Language::C` for `.c` files and `Language::Cpp` for `.cpp`/`.cc`/`.cxx`/`.hpp`/`.hxx`/`.h` files.

## Plan

- [ ] Add `tree-sitter-c` and `tree-sitter-cpp` to workspace and crate dependencies
- [ ] Add `Language::C` and `Language::Cpp` variants to `Language` enum
  - `from_extension`: `"c"` → C; `"cpp"` / `"cc"` / `"cxx"` / `"hpp"` / `"hxx"` / `"h"` / `"hh"` → Cpp
  - `name()`: returns `"c"` or `"cpp"`
- [ ] Create `ising-builders/src/languages/c_cpp.rs` with `extract_nodes()`
  - Walk for `function_definition`, `struct_specifier`/`class_specifier`/`enum_specifier` (with body only)
  - Walk C++ class bodies for method definitions
  - Handle out-of-class C++ method definitions (parse `::` in qualified names)
  - Resolve `#include "..."` (quoted only) to file paths via heuristic search
  - Skip `#include <...>` (system headers)
- [ ] Implement `compute_complexity` for C and C++ (shared with C++ additions)
- [ ] Wire up in `languages/mod.rs`, `structural.rs` dispatch, and `get_tree_sitter_language`
- [ ] Unit tests: functions, structs, classes (C++), methods (in-class and out-of-class), include resolution
- [ ] Integration test: run on a C or C++ project, verify reasonable node extraction despite preprocessor gaps

## Test

- [ ] `.c` files detected with `Language::C`, `.cpp`/`.h` with `Language::Cpp`
- [ ] `void helper() {}` → `Function` node named `helper`
- [ ] `void helper();` (declaration only, no body) → no Function node
- [ ] `struct Foo { int x; };` → `Class` node named `Foo`
- [ ] `class MyClass { void method() {} };` → `Class` node + `Function` node `File.cpp::MyClass::method`
- [ ] `void MyClass::method() {}` outside class → `Function` node `File.cpp::MyClass::method`
- [ ] `#include "util/helper.h"` → `Imports` edge to resolved `util/helper.h`
- [ ] `#include <vector>` → no edge (system header)
- [ ] Forward declaration `class Foo;` → no node
- [ ] Complexity: function with `if`, `for`, `switch` (3 cases), `&&` → 1 + 1 + 1 + 3 + 1 = 7
- [ ] No regression on existing language tests

## Notes

- **This is the hardest language to support well.** The preprocessor, header model, and build system complexity mean tree-sitter extraction will miss more coupling than any other language. Set expectations accordingly — C/C++ support provides ~50-60% of structural coupling, with the change graph filling the gap. This is still valuable for large codebases that have no analysis at all today.
- **Priority is low** precisely because of this complexity. The implementation effort is higher and the extraction fidelity is lower than any other language. Ship Go, Java, C#, PHP, Ruby, Kotlin, and Swift first.
- **Two grammars, shared extractor**: `tree-sitter-c` and `tree-sitter-cpp` have overlapping node types. Use a single extractor module (`c_cpp.rs`) that handles both, with C++ additions gated on the language variant.
- **`.h` defaults to C++**: Parsing `.h` files with the C++ grammar is safe — C is (mostly) a subset of C++. The reverse is not true. This avoids misclassification for the common case of C++ projects using `.h` headers.
- **Include guards and pragma once**: `#ifndef FOO_H` / `#pragma once` are not meaningful for coupling analysis. Ignore them.
- **CMake integration**: A future enhancement could read `CMakeLists.txt` to extract `include_directories()` and `target_link_libraries()` for better include path resolution. Not in scope for this spec — the heuristic approach is sufficient as a starting point.
- **Objective-C**: `.m` and `.mm` files (Objective-C/C++) are not covered. They would require `tree-sitter-objc`. Deferred — Objective-C is declining in favor of Swift.
