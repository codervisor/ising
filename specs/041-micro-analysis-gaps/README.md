---
status: proposed
created: 2026-03-31
priority: high
tags:
- signals
- false-positives
- cross-crate
- call-resolution
- duplication
depends_on:
- 040-micro-level-analysis
---

# Micro-Analysis Gaps: Cross-Boundary Resolution, Duplication & Remaining False Positives

> **Status**: proposed · **Priority**: high · **Created**: 2026-03-31

## Context

Spec 040 delivered function-level analysis (Phase 1-2): call extraction, deprecation detection, 5 new signals, function-level risk computation, and hunk attribution. A self-analysis round exposed systematic false positives and coverage gaps that this spec addresses.

### Empirical findings from self-analysis (ising codebase)

| Round | Total signals | Orphan functions | Orphan modules | Notes |
|-------|--------------|------------------|----------------|-------|
| v1 (raw) | 197 | 128 | 35 | Massive noise from trait impls, struct methods, React components, config files |
| v2 (filtered) | 92 | 41 | 17 | Added heuristic filters: trait methods, qualified methods, PascalCase components, entry-point files, config/scripts |
| v2 accuracy | — | ~0 true positives | ~0 actionable | All remaining orphans are cross-crate public API or standalone viz app code |

**Key observation**: In the ising workspace (7 Rust crates + 1 TS viz app), **100% of remaining orphan function signals are false positives** caused by cross-crate boundary blindness. The call resolver only sees intra-file calls; functions like `build_structural_graph` called from `ising-cli` via `ising-builders::build_all` are invisible.

## Problems

### P1: Cross-Crate Call Resolution (Rust workspaces)

**Current state**: Call edges are created only for same-file calls and import-map-resolved calls within a single file's scope. In a Rust workspace, crate A calling `crate_b::function()` produces no `Calls` edge.

**Impact**: Every `pub fn` in a library crate shows as an orphan function. For ising: 27 false positive orphan signals.

**Root cause**: The structural builder processes files independently. It has no awareness of `use crate_b::something` imports that cross crate boundaries, because `Imports` edges are built from tree-sitter `use_declaration` nodes which reference crate names, not file paths.

### P2: Cross-File Call Resolution (all languages)

**Current state**: Within a single file, calls like `foo()` resolve to `module::foo` if that function exists in the same module. Qualified calls like `bar.method()` attempt import-map resolution. But many call patterns are missed:

- **Dispatch calls**: `match lang { Python => python::extract_nodes(...) }` — the callee is inside a match arm, not a direct `call_expression`
- **Callback/closure passing**: `stmt.query_map(params, map_signal_row)` — function passed as argument, never appears as a `call_expression`
- **Trait method dispatch**: `node.fmt(...)` — resolved at runtime based on the concrete type
- **Re-exports**: `pub use sub_module::func` — creates an alias that callers use

**Impact**: Even within a single crate, many real call relationships are invisible. Functions called via dispatch or as callbacks show as orphans.

### P3: JS/TS Import Resolution

**Current state**: TypeScript/JavaScript `import` statements create `Imports` edges at the module level, but the import map isn't used to resolve function calls across files. React component usage (`<Component />`) is not tracked as a call.

**Impact**: All `ising-viz/` components and utilities show as orphan modules (17) and orphan functions (14). These are a standalone React app — the import graph is entirely internal.

### P4: Code Duplication Detection (deferred from spec 040)

**Current state**: Not implemented. Code duplication (copy-paste code, near-clones) is a common maintainability issue invisible to the tool.

**Impact**: Duplicated code inflates maintenance cost, creates divergent bug fixes, and correlates with defects. Large codebases like Odoo or ha-core likely have significant duplication that the current analysis misses entirely.

### P5: Orphan Signal Confidence Calibration

**Current state**: All orphan signals have fixed severity (0.5 for functions, 0.3 for modules) regardless of evidence strength. A function with zero callers AND zero change history AND zero dependents is much more likely to be truly dead than a heavily-changed function that just happens to be called via dispatch.

**Impact**: Low-confidence orphans (public API, framework callbacks) drown out high-confidence orphans (zero history, zero dependents, low complexity).

## Design

### Phase 3A: Cross-Crate Resolution (Rust)

#### 3A.1 Workspace-Aware Import Map

Build a crate-level dependency graph from `Cargo.toml` workspace members:

```
ising-cli depends on: ising-builders, ising-analysis, ising-db, ising-core
ising-builders depends on: ising-core
ising-analysis depends on: ising-core
...
```

Then, when processing `use ising_builders::build_all` in `ising-cli/src/main.rs`:
1. Map `ising_builders` → crate path `ising-builders/`
2. Resolve `build_all` → `ising-builders/src/lib.rs::build_all` (follow `pub use` re-exports)
3. Create a `Calls` edge from `ising-cli/src/main.rs::cmd_build` → `ising-builders/src/lib.rs::build_all`

**Key data structure**:
```rust
struct WorkspaceMap {
    /// crate_name -> crate root path
    crate_roots: HashMap<String, PathBuf>,
    /// crate_name -> list of pub exports (function IDs)
    pub_exports: HashMap<String, Vec<String>>,
}
```

**Implementation**:
- Parse root `Cargo.toml` for `[workspace] members`
- For each member crate, parse its `Cargo.toml` `[dependencies]` for workspace deps
- Build `pub_exports` by scanning `lib.rs` / `mod.rs` for `pub fn` and `pub use` declarations
- During structural graph building, resolve cross-crate `use` statements against this map

#### 3A.2 `pub` Visibility Tracking

Add `pub visibility: Visibility` to `FunctionInfo` and `Node`:

```rust
#[derive(Default)]
enum Visibility {
    Public,       // pub fn, export function, def (Python top-level)
    CratePublic,  // pub(crate) fn
    #[default]
    Private,      // fn (no pub), private method
}
```

**Usage in orphan detection**: A `Private` function with zero callers within its own file is a much stronger orphan signal than a `Public` function with zero detected callers (which is likely called from another crate).

### Phase 3B: Enhanced JS/TS Resolution

#### 3B.1 Import-to-Function Resolution

Current: `import { foo } from './bar'` creates `Imports` edge `current_module → bar.ts`.

Enhanced: Also create `Calls` edge from the importing function to `bar.ts::foo`, by:
1. Tracking named imports: `{ foo, bar as baz }` → map `foo → bar.ts::foo`, `baz → bar.ts::bar`
2. When a call `foo()` appears in the importing file, resolve it via the named import map
3. Handle default imports: `import Comp from './Comp'` → `Comp → Comp.tsx::default_export`

#### 3B.2 JSX Component Resolution

React `<Component prop={value} />` should create a `Calls` edge:
- Walk JSX elements in the AST
- Extract the tag name (PascalCase = component, lowercase = HTML)
- Resolve against import map: `<BlastRadius />` → imported from `./views/BlastRadius` → `Calls` edge

### Phase 3C: Code Duplication Detection

#### 3C.1 External Tool Integration

Integrate with established duplication detection tools rather than building from scratch:

| Tool | Language support | Output format | License |
|------|-----------------|---------------|---------|
| **jscpd** | 150+ languages | JSON, XML, HTML | MIT |
| **PMD CPD** | Java, C/C++, Go, Python, JS, Ruby, and more | XML, CSV | BSD |
| **duplo** | Language-agnostic (token-based) | Text | - |

**Recommended**: `jscpd` — broadest language coverage, JSON output, npm install.

#### 3C.2 Integration Design

```
ising build --repo-path . --duplication
  1. Run normal graph + change + signal pipeline
  2. Shell out to jscpd: jscpd --reporters json --output /tmp/jscpd-report.json .
  3. Parse JSON output: extract clone pairs (file_a:lines, file_b:lines)
  4. Map clone pairs to graph nodes (file_a → module_id)
  5. Create new signal: CodeDuplication
  6. Optionally create CodeClone edges (Layer 1) for graph connectivity
```

**New signal**:
```rust
SignalType::CodeDuplication
// severity = clone_lines / total_lines (proportion of duplicated code)
// node_a = first file, node_b = second file
// description includes clone block size and location
```

**New edge type** (optional):
```rust
EdgeType::CodeClone  // Layer 1 - structural
// weight = proportion of shared code between the two files
```

#### 3C.3 Health Index Integration

Add duplication to the signal sub-score:

```rust
// In compute_health_index:
+ (signals.duplication_count as f64 / sqrt_n) * 2.0  // weight 2.0 — significant but not critical
```

**Caveat**: Duplication count is sensitive to thresholds (minimum clone size, minimum tokens). Document the jscpd configuration used and note that counts are not comparable across different threshold settings.

### Phase 3D: Orphan Confidence Calibration

Replace fixed severity with evidence-based scoring:

```rust
fn compute_orphan_confidence(graph: &UnifiedGraph, node_id: &str) -> f64 {
    let mut confidence = 0.0;

    // Evidence FOR being truly dead:
    if no_change_history(node_id)       { confidence += 0.3; }
    if zero_dependents(node_id)         { confidence += 0.2; }
    if low_complexity(node_id)          { confidence += 0.1; }  // trivial code more likely abandoned
    if not_in_public_api(node_id)       { confidence += 0.2; }  // private = less likely called externally

    // Evidence AGAINST being truly dead:
    if is_public_api(node_id)           { confidence -= 0.3; }  // likely called from external code
    if has_change_history(node_id)      { confidence -= 0.2; }  // someone actively maintains it
    if is_framework_callback(node_id)   { confidence -= 0.2; }  // called by framework, not user code
    if high_complexity(node_id)         { confidence -= 0.1; }  // significant investment, less likely dead

    confidence.clamp(0.1, 1.0)
}
```

This replaces the current flat 0.5/0.3 severity and surfaces the **most likely truly dead** code first.

## Prioritization

| Phase | Effort | Impact | Rationale |
|-------|--------|--------|-----------|
| **3D: Confidence calibration** | Small | High | Immediately improves signal quality with no new infrastructure |
| **3A: Cross-crate resolution** | Medium | High | Eliminates the dominant false positive source in Rust workspaces |
| **3B: JS/TS resolution** | Medium | Medium | Fixes viz app orphans; benefits all JS/TS projects |
| **3C: Duplication** | Medium | Medium | New capability, requires external tool dependency |

## Remaining Known Blind Spots (not addressed here)

These are documented for completeness — they require fundamentally different approaches and are out of scope for this spec:

1. **API stability**: Breaking changes, deprecation frequency, interface churn. Requires diffing public API surfaces across versions — a different tool category (e.g., cargo-semver-checks for Rust).

2. **Dynamic dispatch**: Languages with heavy runtime dispatch (Python `getattr`, Ruby `send`, Java reflection) will always have incomplete call graphs from static analysis. This is an inherent limitation of AST-based analysis.

3. **Generated code duplication**: Generated files (.pb.go, _pb2.py) may have high duplication by design. The duplication detector should either skip generated files (reuse `is_generated_code`) or flag them differently.

4. **Test coverage correlation**: Orphan functions that have test coverage are less risky than those without. Requires test coverage data (lcov, coverage.py) — a separate integration.

5. **Cross-repo dependencies**: Monorepo analysis sees internal call graphs but not external package callers. A `pub fn` in a published library may have thousands of callers in downstream repos that are invisible to us.

## Checklist

- [ ] Orphan confidence calibration (Phase 3D)
- [ ] Rust `Cargo.toml` workspace member parsing
- [ ] `pub` visibility tracking in function extraction
- [ ] Cross-crate `use` resolution for `Calls` edges
- [ ] JS/TS named import → function call resolution
- [ ] JSX component → `Calls` edge creation
- [ ] jscpd integration (or alternative tool)
- [ ] `CodeDuplication` signal type
- [ ] Health index duplication integration
- [ ] Tests for cross-crate resolution
- [ ] Tests for JS/TS import resolution
- [ ] Tests for duplication detection
- [ ] Validation against ising self-analysis (target: <10 orphan FPs)
- [ ] Validation against OSS test set (12 repos)
