# Implementation Plan: TECH_DESIGN_NEW.md — Three-Layer Code Graph Analysis Engine

## Overview

Pivot from current SCIP + spectral analysis approach to the three-layer graph architecture described in TECH_DESIGN_NEW.md. This implements the **MVP (Phase 1)** scope:

- **Layer 1**: Structural graph (Tree-sitter) for Python + TypeScript
- **Layer 2**: Change graph (git log via gix) with temporal coupling + hotspots
- **Cross-layer signals**: ghost_coupling, fragile_boundary, stable_core, ticking_bomb
- **CLI**: `build`, `impact`, `hotspots`, `signals`
- **SQLite storage**
- **MCP server**: `ising_impact` and `ising_signals` tools

## Architecture Decision

Keep the Cargo workspace structure but reorganize crates:

| Crate | Purpose |
|-------|---------|
| `ising-core` | Graph types, node/edge types, unified graph, metrics computation |
| `ising-db` | **NEW** — SQLite schema, migrations, queries (rusqlite) |
| `ising-builders` | **NEW** — Layer 1 (tree-sitter) + Layer 2 (gix) graph builders |
| `ising-analysis` | **NEW** — Cross-layer signal detection, hotspot ranking |
| `ising-cli` | Refactored CLI with new commands |
| `ising-server` | **NEW** — MCP server (axum + SSE) |
| `ising-scip` | Keep as-is (optional alternative to tree-sitter) |

---

## Step-by-Step Implementation

### Step 1: Update workspace dependencies in root `Cargo.toml`

Add new workspace dependencies:
- `tree-sitter`, `tree-sitter-python`, `tree-sitter-typescript`
- `gix` with `max-performance` feature
- `rusqlite` with `bundled` feature
- `axum`, `tokio` (full features)
- `toml` for config parsing
- `walkdir`, `regex`
- `tracing`, `tracing-subscriber`
- `anyhow`

### Step 2: Refactor `ising-core` — Graph types & metrics

Replace the current SCIP-oriented graph with the unified multi-layer graph:

- **`src/lib.rs`** — Re-export modules
- **`src/graph.rs`** — New unified graph types:
  - `NodeType` enum (Module, Class, Function, Import)
  - `EdgeType` enum (Calls, Imports, Inherits, Contains, CoChanges, ChangePropagates, FaultPropagates, CoFix)
  - `EdgeLayer` enum (Structural, Change, Defect)
  - `Node` struct with attributes (path, language, LOC, complexity, nesting_depth, line_start/end)
  - `Edge` struct with layer, edge_type, weight, metadata
  - `UnifiedGraph` wrapping petgraph with typed nodes/edges
  - Methods: `add_node`, `add_edge`, `has_edge`, `edge_weight`, `nodes`, `edges_of_type`, `node_attr`, `set_node_attr`
- **`src/metrics.rs`** — Node & graph metric computation:
  - Fan-in, Fan-out, CBO
  - Instability (fan-out / (fan-in + fan-out))
  - Modularity score
  - Cycle count detection
- **`src/config.rs`** — Configuration (serde + toml):
  - Time window, coupling thresholds, min co-changes, severity cutoffs
  - Default `ising.toml` values
- **`src/error.rs`** — Updated error types

Keep existing `IsingGraph`, `Symbol`, spectral analysis as legacy (behind feature flag or separate module) for backwards compatibility during transition.

### Step 3: Create `ising-db` — SQLite storage

New crate with:
- **`src/lib.rs`** — Database connection, initialization
- **`src/schema.rs`** — Create tables (nodes, edges, change_metrics, defect_metrics, signals, build_info) with indexes
- **`src/queries.rs`** — CRUD operations:
  - `insert_node`, `insert_edge`, `insert_signal`
  - `get_impact(node_id)` — returns neighbors + signals
  - `get_hotspots(top_n)` — ranked by hotspot score
  - `get_signals(type_filter, min_severity)`
  - `get_path(source, target)` — BFS shortest path
  - `store_build_info`, `get_build_info`

### Step 4: Create `ising-builders` — Layer 1 & 2 builders

New crate with:

**`src/structural.rs`** — Layer 1: Tree-sitter structural graph:
- Walk source files with `walkdir`
- Detect language, select tree-sitter parser
- Parse each file in parallel (rayon)
- Extract: modules, classes, functions, imports
- Build edges: calls, imports, inherits, contains
- Compute node metrics: LOC, cyclomatic complexity, nesting depth

**`src/change.rs`** — Layer 2: Git change graph:
- Open repo with `gix`
- Parse commit history within time window
- Build co-change matrix (file pairs changed in same commit)
- Compute temporal coupling scores
- Filter by min co-changes threshold (default: 5) and min coupling (default: 0.3)
- Compute per-file metrics: change_freq, churn, churn_rate, hotspot_score, sum_coupling

**`src/mod.rs`** — Orchestration:
- `build_all(repo_path, config)` — runs both builders, merges into UnifiedGraph

### Step 5: Create `ising-analysis` — Signal detection

New crate with:

**`src/signals.rs`** — Cross-layer signal detection:
- `SignalType` enum: GhostCoupling, FragileBoundary, UnnecessaryAbstraction, StableCore, TickingBomb
- `Signal` struct: type, node_a, node_b (optional), severity, details
- `detect_signals(structural, change)` — implements signal logic from Section 6
- Percentile computation helpers for node-level thresholds

**`src/hotspots.rs`** — Hotspot ranking:
- `rank_hotspots(graph)` — returns sorted list by hotspot score
- Combines change frequency × complexity (Tornhill model)

### Step 6: Refactor `ising-cli` — New commands

Replace current `index`/`analyze` commands with:

```
ising build [--repo-path .] [--since "6 months ago"] [--db ising.db]
ising impact <file_or_function>
ising hotspots [--top 20]
ising signals [--type ghost_coupling] [--min-severity 0.5]
ising stats
ising export --format json|dot|mermaid
```

Each command reads from SQLite (except `build` which writes to it).

Keep `index` and `analyze` as hidden/deprecated aliases for migration.

### Step 7: Create `ising-server` — MCP server

New crate with:
- **`src/mcp.rs`** — axum + SSE transport MCP server
- Tools: `ising_impact`, `ising_signals`
- Reads from SQLite database
- `ising serve [--port 3000] [--db ising.db]` CLI command

### Step 8: Create default `ising.toml` config

```toml
[build]
time_window = "6 months"

[thresholds]
min_co_changes = 5
min_coupling = 0.3
ghost_coupling_threshold = 0.5
fragile_boundary_coupling = 0.3
fragile_boundary_fault_prop = 0.1
unnecessary_abstraction_coupling = 0.05

[percentiles]
stable_core_freq = 10
stable_core_fan_in = 80
ticking_bomb_hotspot = 90
ticking_bomb_defect = 90
ticking_bomb_coupling = 80
```

### Step 9: Tests

- Unit tests in each crate (graph operations, signal detection, SQL queries)
- Integration test fixtures: small sample repos with known coupling patterns
- CLI integration tests using `assert_cmd`

### Step 10: Verify build & push

- `cargo build` — ensure everything compiles
- `cargo test` — run all tests
- Push to branch

---

## Implementation Order (Dependency Chain)

```
Step 1 (workspace deps)
  → Step 2 (ising-core graph types)
    → Step 3 (ising-db)
    → Step 4 (ising-builders) — depends on Step 2
      → Step 5 (ising-analysis) — depends on Step 2, Step 4
        → Step 6 (ising-cli) — depends on all above
          → Step 7 (ising-server) — depends on Step 3, Step 5
            → Step 8 (config)
              → Step 9 (tests)
                → Step 10 (build & push)
```

Steps 3 and 4 can be done in parallel after Step 2.
