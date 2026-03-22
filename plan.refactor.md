# Refactoring Plan: Self-Analysis Recommendations

Based on the self-analysis report (`specs/021-self-analysis/SELF_ANALYSIS.md`), this plan addresses the three identified issues in priority order.

---

## 1. [HIGH] Extract shared builder types — Resolve ghost coupling

**Problem:** `structural.rs` and `change.rs` have ghost coupling (severity 1.00) — they always change together but share no explicit dependency. Both independently define supported file extensions and rely on the same `ising-core` graph types with similar patterns.

**Root cause:** Both modules duplicate the concept of "supported source file" — `structural.rs` has `Language::from_extension()` and `change.rs` has `is_source_file()`. When a new language is added (e.g. Rust in spec 019), both files must change.

**Changes:**

1. **Create `ising-builders/src/common.rs`** — shared builder types:
   - Move `Language` enum from `structural.rs` into `common.rs`
   - Move `is_source_file()` from `change.rs` and reimplement as `Language::is_supported_extension(ext)`
   - Add `Language::supported_extensions() -> &[&str]` for the change builder to use
   - Re-export from `ising-builders/src/lib.rs`

2. **Update `structural.rs`**:
   - Remove `Language` enum definition
   - Import `Language` from `super::common`

3. **Update `change.rs`**:
   - Remove `is_source_file()` function
   - Import `Language` from `super::common`, use `Language::is_supported_extension()`

**Result:** Adding a new language only requires changing `common.rs` (single point of change). The ghost coupling signal should disappear on re-analysis.

---

## 2. [MEDIUM] Split `ising-db/src/lib.rs` into submodules

**Problem:** `ising-db/src/lib.rs` is 886 lines with complexity 176 (rank #2 hotspot at 0.57). It mixes schema DDL, CRUD queries, export logic, and type definitions in one file.

**Changes:**

1. **Create `ising-db/src/schema.rs`** — DDL and initialization:
   - Move `init_schema()` method
   - Move `clear()` method
   - Keep as inherent methods on `Database` via a trait or impl block

2. **Create `ising-db/src/queries.rs`** — read/write queries:
   - Move `store_graph()`, `store_signals()`, `store_build_info()`
   - Move `get_signals()`, `get_hotspots()`, `get_impact()`, `get_stats()`
   - Move `get_build_info()`

3. **Create `ising-db/src/export.rs`** — export/visualization:
   - Move `VizExport`, `VizNode`, `VizEdge`, `VizSignal` structs
   - Move `export_viz()` method
   - Move any DOT/Mermaid export logic

4. **Refactor `ising-db/src/lib.rs`** — thin module root:
   - Keep `Database` struct definition + `open()` / `open_in_memory()` constructors
   - Keep `DbError` and `StoredSignal` type definitions
   - Add `mod schema; mod queries; mod export;`
   - Re-export public types

**Target:** Each file ~150-250 lines instead of one 886-line file. Complexity drops from 176 to ~60 per file.

---

## 3. [LOW] Split `structural.rs` by language

**Problem:** `structural.rs` is the #1 hotspot (1088 lines, score 0.85, complexity 150). It contains per-language extractors that are independent of each other. Adding Rust support (spec 019) required touching this file even though it was unrelated to Python/TypeScript logic.

**Changes:**

1. **Create `ising-builders/src/languages/mod.rs`** — language extraction framework:
   - Define a `LanguageExtractor` trait with method `extract(source: &[u8], path: &str) -> FileAnalysis`
   - Move `FileAnalysis`, `FunctionInfo`, `ClassInfo`, `ImportInfo` structs here

2. **Create `ising-builders/src/languages/python.rs`**:
   - Move `extract_python_nodes()` and `compute_python_complexity()`
   - Implement `LanguageExtractor` for Python

3. **Create `ising-builders/src/languages/typescript.rs`**:
   - Move `extract_ts_nodes()` and `compute_ts_complexity()`
   - Implement `LanguageExtractor` for TypeScript/JavaScript

4. **Create `ising-builders/src/languages/rust.rs`**:
   - Move `extract_rust_nodes()` and `compute_rust_complexity()`
   - Implement `LanguageExtractor` for Rust

5. **Simplify `structural.rs`**:
   - Keep `build_structural_graph()` orchestration and `walk_source_files()`
   - Dispatch to language-specific extractors via the trait
   - Should shrink from 1088 lines to ~200 lines

**Target:** Per-language files ~200-300 lines each. Adding a new language = adding one file + registering it in `mod.rs`. No existing language files need to change.

---

## Implementation Order

```
Step 1: Extract shared builder types (common.rs)     ~30 min
Step 2: Split ising-db/src/lib.rs into submodules     ~45 min
Step 3: Split structural.rs by language                ~60 min
Step 4: Run `cargo build && cargo test` to verify      ~5 min
Step 5: Re-run `ising build --repo-path .` to verify   ~5 min
        ghost coupling signal is resolved
```

Steps 1 and 2 are independent and can be done in parallel.
Step 3 depends on Step 1 (since `Language` moves to `common.rs`).

## Expected Outcomes After Refactoring

| Metric | Before | After |
|--------|--------|-------|
| Ghost coupling signals | 1 | 0 |
| `structural.rs` lines | 1088 | ~200 |
| `ising-db/lib.rs` lines | 886 | ~80 |
| Max file complexity | 176 | ~60 |
| Hotspot #1 score | 0.85 | Distributed across language files |
| Files changed to add a language | 2 (structural + change) | 1 (new language file + common.rs enum) |
