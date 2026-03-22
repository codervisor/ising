---
status: planned
created: 2026-03-22
priority: medium
tags:
- language-support
- tree-sitter
- ruby
depends_on:
- 001-rust-core
- 019-rust-language-support
---
# Ruby Language Support

> **Status**: planned · **Priority**: medium · **Created**: 2026-03-22

## Overview

Ruby powers Rails — one of the most common frameworks for web applications. Rails codebases are often large, convention-heavy, and prone to hidden coupling through ActiveRecord associations, concerns, and metaprogramming. Ising can surface structural coupling that Rails conventions obscure.

This spec adds Ruby support via `tree-sitter-ruby`. Ruby's class/module/method structure maps to Ising's model, but Ruby's dynamic nature and heavy metaprogramming mean some coupling is invisible to static analysis.

## Design

### Layer 1: Structural Graph

#### Ruby-Specific Node Types

| Tree-sitter node | Ising concept | Notes |
|---|---|---|
| `method` | Function | `def method_name` at top level |
| `singleton_method` | Function | `def self.class_method` — class methods |
| `class` | Class | `class MyClass` definitions |
| `module` | Class | `module MyModule` — treated as class (similar to trait/interface) |
| `call` with `require`/`require_relative` | Import | Ruby's file-based imports |

#### Method Attribution

Ruby methods inside classes:

```
program
  class
    name: constant          (UserService)
    body_statement
      method
        name: identifier    (process)
```

Attribution: `file.rb::UserService::process`. For nested classes/modules: `file.rb::Outer::Inner::method`.

`singleton_method` (class methods via `def self.method_name`) are attributed the same way — the `self.` prefix is stripped.

#### Import Resolution

Ruby uses `require` and `require_relative` for file imports:

1. **`require_relative './helper'`** — resolved relative to the current file. Append `.rb` if missing.
2. **`require 'app/models/user'`** — resolved relative to load path. In Rails, `app/` directories are autoloaded.
3. **Gem requires** (`require 'json'`, `require 'rails'`) — skip, no nodes in graph.

Resolution strategy:
- `require_relative`: resolve relative to current file, append `.rb`
- `require`: check if the path (with `.rb` appended) exists as a module node. For Rails projects, check under common autoload paths (`app/models/`, `app/controllers/`, `app/services/`, `lib/`).
- Detect Rails by checking for `config/application.rb` or `Gemfile` containing `rails`.

#### Challenges

- **Metaprogramming**: Ruby's `define_method`, `method_missing`, `class_eval`, and `send` create methods/calls that are invisible to static analysis. This is a fundamental limitation — tree-sitter sees only the literal AST. Accept this gap; the change graph (Layer 2) catches coupling that static analysis misses.
- **Rails magic**: ActiveRecord associations (`has_many`, `belongs_to`), concerns (`include Cacheable`), and callbacks (`before_action`) create implicit coupling. These appear as `call` nodes in the AST but don't map to Ising's node types. Do not attempt to extract these — the change graph handles the coupling they create.
- **`module` as namespace vs mixin**: Ruby's `module` serves dual purpose — namespace grouping and mixin (like PHP traits). Both are extracted as Class nodes. `include ModuleName` inside a class is composition, not a file import — do not create an import edge.
- **Blocks and procs**: `do...end` blocks and lambda/proc objects are not extracted as function nodes — they're too granular and anonymous.
- **Open classes**: Ruby allows reopening classes across files (`class String; def custom; end; end`). Each file is analyzed independently, so methods added via open classes are attributed to the file's module node with the class prefix. This is correct — it shows coupling between the file that reopens the class and files that use the added methods.

#### Complexity for Ruby

| Tree-sitter node | Reason |
|---|---|
| `if` / `unless` | Conditional branch |
| `if_modifier` / `unless_modifier` | Inline conditional (`return if x`) |
| `for` | For loop (rare in Ruby) |
| `while` / `until` | Loops |
| `while_modifier` / `until_modifier` | Inline loops |
| `when` | Each `when` in a `case` statement |
| `in` (pattern matching) | Ruby 3.0+ pattern match arms |
| `rescue` | Exception handler |
| `binary` with `&&` / `\|\|` / `and` / `or` | Logical branching |
| `conditional` | Ternary `?:` |

Base complexity = 1 + count of above.

### Layer 2: Change Graph

Add `.rb` to `Language::from_extension` and `supported_extensions()`.

## Plan

- [ ] Add `tree-sitter-ruby` to workspace and crate dependencies
- [ ] Add `Language::Ruby` variant to `Language` enum
  - `from_extension`: `"rb"` → `Language::Ruby`
  - `name()`: returns `"ruby"`
- [ ] Create `ising-builders/src/languages/ruby.rs` with `extract_nodes()`
  - Walk for `method`, `singleton_method`, `class`, `module`
  - Walk class/module bodies for nested methods
  - Resolve `require_relative` and `require` calls to file paths
  - Detect Rails projects for autoload path resolution
- [ ] Implement `compute_complexity` for Ruby
- [ ] Wire up in `languages/mod.rs`, `structural.rs` dispatch, and `get_tree_sitter_language`
- [ ] Unit tests: classes, modules, methods, singleton methods, require resolution
- [ ] Integration test: run on a Ruby/Rails project

## Test

- [ ] `.rb` files detected with `Language::Ruby`
- [ ] `def helper` at file scope → `Function` node named `helper`
- [ ] `def self.class_method` → `Function` node named `class_method`
- [ ] `class UserService` → `Class` node named `UserService`
- [ ] `module Authentication` → `Class` node named `Authentication`
- [ ] `def process` inside `UserService` → `Function` node `file.rb::UserService::process`
- [ ] `require_relative './helper'` → `Imports` edge to `dir/helper.rb`
- [ ] `require 'json'` → no edge (gem/stdlib)
- [ ] `include Cacheable` inside a class → no import edge (mixin, not file import)
- [ ] Complexity: method with `if`, `unless`, `rescue`, `&&` → 1 + 1 + 1 + 1 + 1 = 5
- [ ] No regression on existing language tests

## Notes

- **Metaprogramming is the elephant in the room**: Ruby's dynamic nature means static analysis captures maybe 60-70% of the actual coupling in a typical Rails app. The change graph (Layer 2, git co-change analysis) is critical for Ruby — it catches the coupling that metaprogramming creates but tree-sitter can't see. This two-layer approach is exactly why Ising's architecture works well for Ruby despite its dynamic nature.
- **Rails autoloading**: Rails uses Zeitwerk for constant-based autoloading — `UserService` automatically loads `app/services/user_service.rb`. This means many Rails files have no explicit `require` statements at all. Import edges will be sparse for Rails projects. Again, the change graph compensates.
- **ERB templates**: `.erb` files contain embedded Ruby in HTML. These are not supported in this spec — they're primarily view templates with minimal coupling signal. Could be added later if needed.
- **RSpec/Minitest**: Test files (`*_spec.rb`, `*_test.rb`) should be included in analysis. Test-to-source coupling is a meaningful signal.
