# Ising Self-Analysis: Analyzing Ising with Ising

> This report has been run twice. The **Post-Rust** section reflects the current state after spec 019 (Rust language support).

## Post-Rust Analysis (Current)

**Date:** 2026-03-22
**Commits analyzed:** 16
**Command:** `ising build --repo-path .`

### Graph Summary

| Metric | Value |
|--------|-------|
| Nodes | 246 |
| Structural edges | 201 |
| Change edges | 1 |
| Defect edges | 0 |
| Cycles | 0 |
| Signals | 2 |

### Signals

```
[1.00] ghost_coupling: ising-builders/src/change.rs <-> ising-builders/src/structural.rs
[0.10] stable_core: ising-core/src/ignore.rs
```

### Hotspot Rankings (Top 10)

| Rank | File | Score | Complexity | Freq |
|------|------|-------|------------|------|
| 1 | `ising-builders/src/structural.rs` | **0.85** | 150 | 7 |
| 2 | `ising-db/src/lib.rs` | 0.57 | 176 | 4 |
| 3 | `ising-cli/src/main.rs` | 0.55 | 113 | 6 |
| 4 | `ising-builders/src/change.rs` | 0.28 | 57 | 6 |
| 5 | `ising-analysis/src/signals.rs` | 0.23 | 70 | 4 |
| 6 | `ising-scip/src/lib.rs` | 0.12 | 38 | 4 |
| 7 | `ising-core/src/graph.rs` | 0.07 | 42 | 2 |
| 8 | `ising-core/src/config.rs` | 0.06 | 24 | 3 |
| 9 | `ising-server/src/lib.rs` | 0.04 | 22 | 2 |
| 10 | `ising-core/src/ignore.rs` | 0.02 | 26 | 1 |

### Impact Analysis

#### `ising-builders/src/structural.rs` (Blast Radius)

- **Fan-out:** 19 (all contained functions/types)
- **Temporal coupling:** 100% with `change.rs`
- **Active signal:** Ghost coupling (severity 1.00)
- **Key functions:** `build_structural_graph`, `extract_python_nodes`, `extract_ts_nodes`, `extract_rust_nodes`, `compute_complexity`

This is the highest-risk file to modify. Changes propagate temporally to `change.rs` and structurally to the entire builder pipeline.

#### `ising-core/src/graph.rs`

- **Fan-out:** 27 (largest structural footprint in the codebase)
- **Change frequency:** 2 (low — stabilized)
- **Hotspot score:** 0.07
- **Key types:** `UnifiedGraph`, `Node`, `Edge`, `NodeType`, `EdgeType`, `EdgeLayer`

The core graph module has the largest symbol count but stabilized early — a sign of solid upfront design.

#### `ising-db/src/lib.rs`

- **Fan-out:** 26
- **Change frequency:** 4
- **Hotspot score:** 0.57
- **Key types:** `Database`, `StoredSignal`, `ImpactResult`, `VizExport`

The database layer has the highest raw complexity (176), driven by SQL query construction and serialization logic. A candidate for splitting into submodules.

#### `ising-cli/src/main.rs`

- **Fan-out:** 22
- **Change frequency:** 6
- **Hotspot score:** 0.55
- **Key functions:** `cmd_build`, `cmd_impact`, `cmd_hotspots`, `cmd_signals`, `cmd_stats`, `cmd_export`, `cmd_serve`

The CLI entry point orchestrates all commands. Its complexity is inherent to its role as the user-facing interface.

### Mermaid Graph

```mermaid
graph LR
  structural["structural.rs<br/>🔥 0.85"]:::hot
  db["db/lib.rs<br/>0.57"]
  cli["cli/main.rs<br/>0.55"]
  change["change.rs<br/>0.28"]
  signals["signals.rs<br/>0.23"]
  scip["scip/lib.rs<br/>0.12"]
  graph["graph.rs<br/>0.07"]
  config["config.rs<br/>0.06"]
  server["server/lib.rs<br/>0.04"]
  ignore["ignore.rs<br/>🛡️ 0.02"]:::guard

  change -.->|ghost_coupling 1.00| structural

  classDef hot fill:#f96,stroke:#333
  classDef guard fill:#6f9,stroke:#333
```

### Architecture Observations

1. **Zero cycles** — The dependency graph is a clean DAG. The crate structure (`core` → `builders` → `analysis` → `db` → `cli`) enforces layering at compile time.

2. **Ghost coupling as self-diagnosis** — Ising detected its own architectural smell: the implicit coupling between its two builder modules. This validates the signal engine on a real codebase.

3. **Complexity concentration** — 76% of total complexity lives in just 3 files (`structural.rs`, `db/lib.rs`, `cli/main.rs`). This is typical for a young codebase but flags future refactoring targets.

4. **Stable core** — `ising-core` modules (`graph.rs`, `config.rs`, `ignore.rs`, `metrics.rs`) are all low-churn, low-hotspot. The foundational layer is solid.

### Recommendations

| Priority | Action | Rationale |
|----------|--------|-----------|
| **High** | Extract shared builder types to resolve ghost coupling | Make the `change.rs` ↔ `structural.rs` relationship explicit |
| **Medium** | Split `ising-db/src/lib.rs` into `schema.rs`, `queries.rs`, `export.rs` | Reduce complexity concentration (176 → ~60 each) |
| **Low** | Split `structural.rs` by language | Per-language extractors (`python.rs`, `typescript.rs`, `rust.rs`) would reduce per-file churn |

---

## Pre-Rust Analysis (Original — Before Spec 019)

**Date:** 2026-03-22
**Commit:** 2fcd797
**Command:** `ising build --repo-path . --db ising-self.db --since "3 years ago"`

### Graph Summary

| Metric | Value |
|--------|-------|
| Nodes | 51 |
| Structural edges | 16 |
| Change edges | 0 |
| Defect edges | 0 |
| Cycles | 0 |
| Signals | 0 |

**Node breakdown:** 35 modules, 16 functions

All 16 edges were `structural/contains` (module → function containment).
No `imports`, `calls`, or `inherits` edges were detected — the core Rust codebase
was invisible to the analyzer.

### Hotspot Rankings (Top 10)

| Rank | File | Score | Complexity | Freq |
|------|------|-------|------------|------|
| 1 | `scripts/publish-platform-packages.ts` | 0.50 | 15 | 1 |
| 2 | `scripts/validate-platform-binaries.ts` | 0.47 | 14 | 1 |
| 3 | `ising-viz/src/state/context.tsx` | 0.43 | 13 | 1 |
| 4 | `scripts/publish-main-packages.ts` | 0.40 | 12 | 1 |
| 5 | `scripts/prepare-publish.ts` | 0.37 | 11 | 1 |
| 6 | `ising-viz/src/App.tsx` | 0.33 | 10 | 1 |
| 7 | `scripts/validate-no-workspace-protocol.ts` | 0.27 | 8 | 1 |
| 8 | `scripts/generate-platform-manifests.ts` | 0.23 | 7 | 1 |
| 9 | `scripts/sync-versions.ts` | 0.23 | 7 | 1 |
| 10 | `scripts/add-platform-deps.ts` | 0.17 | 5 | 1 |

### Key Finding

**Ising could not analyze its own core logic.** The Rust backend containing all
graph algorithms, signal detection, and persistence was invisible. The tool only
saw its periphery (visualization SPA, publishing scripts, npm wrapper).

---

*Analysis performed on 2026-03-22. Tool: Ising v0.1.0 analyzing itself — turtles all the way down.*
