---
status: planned
created: 2026-03-22
priority: high
tags:
- language-support
- tree-sitter
- php
depends_on:
- 001-rust-core
- 019-rust-language-support
---
# PHP Language Support

> **Status**: planned · **Priority**: high · **Created**: 2026-03-22

## Overview

PHP powers a massive share of the web — WordPress, Laravel, Symfony, Drupal. Many PHP codebases are long-lived, organically grown, and carry significant coupling debt. Ising's structural analysis can surface coupling problems that are hard to see in large PHP projects.

This spec adds PHP support via `tree-sitter-php`. PHP's class/function model maps to Ising's extraction pattern, but PHP has unique quirks around mixed HTML/PHP, autoloading conventions, and dual paradigms (procedural + OOP) that need careful handling.

## Design

### Layer 1: Structural Graph

#### PHP-Specific Node Types

| Tree-sitter node | Ising concept | Notes |
|---|---|---|
| `function_definition` | Function | Top-level `function foo()` declarations |
| `class_declaration` | Class | Class definitions |
| `interface_declaration` | Class | Interface definitions |
| `trait_declaration` | Class | Trait definitions (PHP mixins) |
| `enum_declaration` | Class | PHP 8.1+ enum types |
| `method_declaration` | Function | Methods inside classes, attributed as `ClassName::method` |
| `namespace_use_declaration` | Import | `use App\Models\User` — intra-project only |

#### Method Attribution

PHP methods live inside class/interface/trait bodies:

```
program
  class_declaration
    name                    (MyController)
    declaration_list
      method_declaration
        name                (index)
```

Attribution: `file.php::MyController::index`. Mirrors the Java/C# pattern.

#### Import Resolution

PHP uses `use` statements with fully qualified namespace paths:

1. **Built-in classes** — PHP has no standard library namespace prefix. Heuristic: skip imports starting with common vendor prefixes that don't match the project namespace.
2. **Composer autoloading** — Most modern PHP projects use Composer's PSR-4 autoloading, which maps namespace prefixes to directories via `composer.json`.

Resolution strategy:
- Read `composer.json` → `autoload.psr-4` to get namespace-to-directory mappings
- `"App\\": "src/"` means `App\Models\User` → `src/Models/User.php`
- If no `composer.json`, fall back to converting namespace separators to directory separators

#### Challenges

- **Mixed HTML/PHP**: Files like `template.php` contain `<?php ... ?>` blocks embedded in HTML. `tree-sitter-php` handles this via a `program` node containing `php_tag` and `text` children. The extractor should only process PHP code sections. The grammar handles this correctly — just walk the AST as usual.
- **No class requirement**: PHP allows top-level functions and procedural code (no class wrapper). Both class methods and standalone functions must be extracted.
- **Traits**: PHP traits (`trait Cacheable { ... }`) are reusable method sets mixed into classes via `use Cacheable;`. Extract traits as Class nodes. The `use TraitName;` inside a class body is a _trait use_, not a namespace import — do not create an import edge from it (it's intra-file composition).
- **Anonymous classes**: `new class { ... }` expressions should be skipped (no meaningful name to attribute).
- **`include`/`require` statements**: Legacy PHP uses `include "file.php"` instead of autoloading. These could be treated as imports, but they're increasingly rare in modern codebases. Defer to a follow-up if needed.

#### Complexity for PHP

| Tree-sitter node | Reason |
|---|---|
| `if_statement` | Conditional branch |
| `for_statement` | For loop |
| `foreach_statement` | Foreach loop |
| `while_statement` | While loop |
| `do_statement` | Do-while loop |
| `catch_clause` | Exception handler |
| `case_statement` | Each case in switch |
| `binary_expression` with `&&` / `\|\|` / `and` / `or` | Logical branching |
| `conditional_expression` | Ternary `?:` |
| `match_condition_list` | PHP 8.0 match expression arms |

Base complexity = 1 + count of above.

### Layer 2: Change Graph

Add `.php` to `Language::from_extension` and `supported_extensions()`.

## Plan

- [ ] Add `tree-sitter-php` to workspace and crate dependencies
- [ ] Add `Language::Php` variant to `Language` enum
  - `from_extension`: `"php"` → `Language::Php`
  - `name()`: returns `"php"`
- [ ] Create `ising-builders/src/languages/php.rs` with `extract_nodes()`
  - Walk for `function_definition`, `class_declaration`, `interface_declaration`, `trait_declaration`, `enum_declaration`
  - Walk class bodies for `method_declaration`
  - Resolve `namespace_use_declaration` via `composer.json` PSR-4 mappings
  - Skip trait `use` statements inside class bodies (not namespace imports)
- [ ] Implement `compute_complexity` for PHP
- [ ] Wire up in `languages/mod.rs`, `structural.rs` dispatch, and `get_tree_sitter_language`
- [ ] Unit tests: classes, traits, interfaces, standalone functions, methods, PSR-4 import resolution
- [ ] Integration test: run on a PHP project (e.g., a Laravel skeleton)

## Test

- [ ] `.php` files detected with `Language::Php`
- [ ] `function helper()` at file scope → `Function` node named `helper`
- [ ] `class UserController` → `Class` node named `UserController`
- [ ] `trait Cacheable` → `Class` node named `Cacheable`
- [ ] `public function index()` inside `UserController` → `Function` node `file.php::UserController::index`
- [ ] `use App\Models\User;` with PSR-4 mapping `App\ → src/` → `Imports` edge to `src/Models/User.php`
- [ ] `use Illuminate\Http\Request;` → no edge (vendor dependency)
- [ ] `use Cacheable;` inside a class body → no import edge (trait use, not namespace import)
- [ ] Complexity: method with `if`, `foreach`, `catch`, `&&` → 1 + 1 + 1 + 1 + 1 = 5
- [ ] No regression on existing language tests

## Notes

- **PSR-4 is critical**: Without `composer.json` PSR-4 mappings, PHP import resolution is guesswork. The spec requires reading `composer.json` — this is the first language that needs a config file for import resolution. If `composer.json` is missing, fall back to namespace-to-path conversion with no guarantee of correctness.
- **`tree-sitter-php` grammar variant**: The `tree-sitter-php` crate may expose two grammars: one for PHP-only files and one for PHP embedded in HTML. Use the PHP-only grammar by default. If a file starts with `<?php`, both grammars produce the same AST for the PHP sections.
- **WordPress/legacy PHP**: Older PHP codebases use procedural code with `include`/`require` instead of namespaces. These projects will get function/class extraction but limited import resolution. This is acceptable — the change graph (Layer 2) still provides coupling signals from git co-change history.
- **PHP 8.1+ enums**: Modern PHP enums are backed classes. The `tree-sitter-php` grammar represents them as `enum_declaration` — extract as Class nodes.
