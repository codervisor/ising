---
status: complete
created: 2026-03-31
priority: high
tags:
- signals
- granularity
- function-level
- orphan-detection
depends_on:
- 001-rust-core
- 012-intra-file-coupling
---

# Micro-Level Analysis: Function-Level Risk, Orphans, Deprecation & Staleness

> **Status**: complete · **Priority**: high · **Created**: 2026-03-31
>
> **Follow-up**: [041-micro-analysis-gaps](../041-micro-analysis-gaps/README.md) — addresses remaining false positives, cross-crate resolution, and code duplication.

## Problem

All current analysis operates at **file (module) granularity**. Function and class nodes are extracted by tree-sitter but sit dormant — never used for risk scoring, signal detection, or health assessment. This creates blind spots:

1. **Function health is invisible** — A 500-line file might have one 300-line god function and ten healthy helpers. Current analysis treats them identically.
2. **Orphan code goes undetected** — Functions/modules with zero callers/importers are classified as "Stable" rather than "potentially dead."
3. **Deprecated code is invisible** — `@deprecated`, `#[deprecated]`, `[Obsolete]` markers are never parsed or surfaced.
4. **Stale vs. stable is ambiguous** — A file untouched for 2 years could be a rock-solid utility or abandoned code. Without `last_changed` timestamps, we can't distinguish them.
5. **Intra-file coupling is hidden** — Which functions change together? Which function is the real hotspot inside a large file?

## Design

### Phase 1: Function-Level Foundation

#### 1.1 Call Extraction

Enhance language parsers to extract function/method calls from AST.

**New type in `languages/mod.rs`:**
```rust
pub struct CallInfo {
    pub callee: String,        // Raw callee name as written: "foo", "self.bar", "pkg.Func"
    pub line: u32,             // Line of the call site
}
```

**Added to `FunctionInfo`:**
```rust
pub struct FunctionInfo {
    pub name: String,
    pub line_start: u32,
    pub line_end: u32,
    pub complexity: u32,
    pub calls: Vec<CallInfo>,       // NEW
    pub deprecated: bool,           // NEW
}
```

**Per-language AST patterns:**

| Language | Call node type | Callee extraction |
|----------|---------------|-------------------|
| Python | `call` | `function` child → identifier or attribute |
| TypeScript/JS | `call_expression` | `function` child → identifier or member_expression |
| Rust | `call_expression`, `macro_invocation` | `function` child |
| Go | `call_expression` | `function` child → identifier or selector_expression |
| Java | `method_invocation` | `name` + optional `object` |
| C# | `invocation_expression` | `member_access_expression` or `identifier` |

**Resolution strategy (in `structural.rs`):**
1. **Same-file calls**: `foo()` → look up `module_id::foo` in known functions
2. **Qualified calls**: `self.method()` / `this.method()` → look up methods in same class
3. **Cross-file calls**: `import bar; bar.func()` → resolve via import map to `bar_module::func`
4. **Unresolved**: Skip gracefully — partial call graphs are still useful

#### 1.2 Deprecation Detection

**Per-language patterns:**

| Language | Pattern | AST node |
|----------|---------|----------|
| Python | `@deprecated` decorator, `warnings.warn("deprecated")` | `decorator` |
| Rust | `#[deprecated]` | `attribute_item` |
| TypeScript/JS | `@deprecated` JSDoc tag, `/** @deprecated */` | `comment` |
| Go | `// Deprecated:` comment (godoc convention) | `comment` |
| Java | `@Deprecated` annotation | `annotation` / `marker_annotation` |
| C# | `[Obsolete]` attribute | `attribute` |
| PHP | `@deprecated` docblock tag | `comment` |
| Ruby | Custom (no standard), check for `warn "deprecated"` | - |
| Kotlin | `@Deprecated` annotation | `annotation` |
| C/C++ | `[[deprecated]]`, `__attribute__((deprecated))` | `attribute_declaration` |

**Added to `ClassInfo`:**
```rust
pub struct ClassInfo {
    pub name: String,
    pub line_start: u32,
    pub line_end: u32,
    pub complexity: u32,
    pub deprecated: bool,    // NEW
}
```

**Added to `Node`:**
```rust
pub struct Node {
    // ... existing fields ...
    pub deprecated: bool,    // NEW — propagated from parser output
}
```

#### 1.3 Orphan Detection

Two levels of orphan detection:

**Function-level orphans:**
- Zero incoming `Calls` edges
- Not a module entry point (not `main`, `__init__`, test function, exported)
- Not a public API method (heuristic: not in `__all__`, not `pub` in Rust, not `export` in TS)

**Module-level orphans:**
- Zero incoming `Imports` edges
- Not a root entry point (main.py, index.ts, lib.rs, main.rs, etc.)
- Not a test file
- Has been unchanged for > threshold period (avoids flagging new files)

#### 1.4 Staleness Detection

Populate `last_changed` field from git history:
- During change analysis, track the most recent commit timestamp per file
- For function-level: use hunk attribution (Phase 2) to get per-function last_changed
- **Stale threshold**: Configurable, default 12 months with zero change activity

### Phase 2: Function-Level Risk

#### 2.1 Hunk-to-Symbol Attribution

(Implements core of spec 012)

Map git diff hunks to function/class nodes using line range overlap:

```
Commit diff: file.py lines 42-58 changed
Symbol map:  file.py::func_a = lines 30-55
             file.py::func_b = lines 57-80

Attribution: func_a gets 14 lines (42-55)
             func_b gets 2 lines (57-58)
```

**Output**: Per-function `ChangeMetrics` (change_freq, churn_lines, churn_rate, hotspot_score).

#### 2.2 Function-Level Risk Scores

Extend `compute_risk_field` to optionally compute risk for Function nodes:
- `change_load` from attributed change metrics
- `capacity` from function complexity and nesting
- `propagated_risk` from `Calls` edges (functions this one calls/is called by)
- `direct_score`, `safety_factor`, `risk_tier` — same formulas, function-scoped

#### 2.3 New Signals

| Signal | Type | Severity | Description |
|--------|------|----------|-------------|
| `OrphanFunction` | Function | info-warning | Function with zero callers, not an entry point |
| `OrphanModule` | Module | info-warning | Module with zero importers, not an entry point |
| `DeprecatedUsage` | Function/Module | warning | Deprecated symbol still being called/imported |
| `StaleCode` | Function/Module | info | No changes in > threshold period, low connectivity |
| `IntraFileHotspot` | Function | warning | Function churns far more than siblings in same file |

### Configuration

```toml
[thresholds]
# Orphan detection
orphan_min_age_days = 90           # Don't flag files newer than this
orphan_entry_patterns = ["main", "index", "lib", "__init__", "mod"]

# Staleness
stale_threshold_days = 365         # Consider stale after this period
stale_min_modules = 10             # Don't flag in tiny repos

# Intra-file hotspot
intra_hotspot_ratio = 3.0          # Function churn / sibling median churn
```

## Schema Changes

```sql
-- Add to nodes table
ALTER TABLE nodes ADD COLUMN deprecated BOOLEAN DEFAULT 0;

-- Add to change_metrics (support function-level)
-- No schema change needed: change_metrics.node_id already supports any node type
```

## Checklist

- [x] Add `CallInfo`, `deprecated` to parser types (`languages/mod.rs`)
- [x] Add `deprecated` to `Node` struct (`graph.rs`)
- [x] Extract function calls in Python parser
- [x] Extract function calls in TypeScript/JS parser
- [x] Extract function calls in Rust parser
- [x] Extract function calls in Go parser
- [x] Extract function calls in Java parser
- [x] Extract function calls in C# parser
- [x] Extract deprecation markers in all parsers
- [x] Create `Calls` edges in structural builder (`structural.rs`)
- [x] Resolve same-file and cross-file call targets
- [x] Populate `last_changed` from git history (`change.rs`)
- [x] Implement hunk-to-symbol attribution (`change.rs`)
- [x] Function-level `ChangeMetrics` from attribution
- [x] New signals: `OrphanFunction`, `OrphanModule`
- [x] New signals: `DeprecatedUsage`
- [x] New signals: `StaleCode`
- [x] New signals: `IntraFileHotspot`
- [x] Function-level risk computation in `stress.rs`
- [x] DB schema update for `deprecated` column
- [x] Update `store_graph` / `load_graph` for new fields
- [x] CLI output for function-level data
- [x] Tests for call extraction (per language)
- [x] Tests for orphan/deprecation/stale signals
- [x] Tests for function-level risk
- [x] Integration tests
