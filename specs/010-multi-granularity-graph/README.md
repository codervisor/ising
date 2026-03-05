---
status: planned
created: 2026-03-05
priority: medium
tags:
- graph
- architecture
- spectral
created_at: 2026-03-05T02:45:25.265313242Z
updated_at: 2026-03-05T02:45:25.265313242Z
---

# Multi-Granularity Graph — File, Module & Package Aggregation

## Problem

The current `IsingGraph` operates at individual symbol granularity only. Each node is a `Symbol` (function, class, variable, etc.) and the adjacency matrix rows/columns map 1:1 to symbols.

Real-world architectural analysis requires multiple levels of granularity:
- **Symbol-level**: Current behavior — fine-grained coupling detection
- **File-level**: Which files are coupled? Useful for change-impact analysis
- **Directory/Module-level**: Are modules well-separated? Direct input to Modularity Q
- **Package-level**: Cross-package dependency health

Without aggregation, spectral metrics (λ_max, Modularity Q) only reflect symbol-level structure, missing higher-level architectural patterns.

## Solution

Add a graph aggregation layer that collapses symbol nodes into coarser groupings based on their `file` field and path hierarchy.

### Granularity Levels

| Level | Grouping Key | Use Case |
|---|---|---|
| Symbol | `symbol.name` (current) | Fine-grained coupling |
| File | `symbol.file` | Change-impact, co-change |
| Directory | Parent directory of `symbol.file` | Module boundary detection |
| Package | Top-level directory or manifest boundary | Cross-package health |

### Design

1. **`GranularityLevel` enum** — `Symbol`, `File`, `Directory(depth)`, `Package`
2. **`aggregate(&self, level: GranularityLevel) -> IsingGraph`** method on `IsingGraph`
   - Groups nodes by the selected key
   - Creates one aggregate node per group
   - Merges edges: if any symbol in group A depends on any symbol in group B, add edge A→B
   - Edge weight = count of collapsed symbol-level edges (for weighted analysis)
3. **Weighted adjacency matrix** — `to_weighted_adjacency_matrix()` returning edge counts after aggregation

### Key Decisions

- Aggregation produces a **new** `IsingGraph`, not a view — keeps the API simple
- Directory depth is configurable: depth=1 groups by top-level dir, depth=2 by two levels, etc.
- Self-loops (intra-group edges) are excluded by default but optionally includable

## Checklist

- [ ] Add `GranularityLevel` enum to `graph` module
- [ ] Implement `aggregate()` on `IsingGraph`
- [ ] Add weighted edge support (edge data `f64` instead of `()`)
- [ ] Add `to_weighted_adjacency_matrix()` method
- [ ] Unit tests: symbol→file aggregation, symbol→directory aggregation
- [ ] Integration test: aggregate → spectral analysis pipeline

## Dependencies

- Builds on the existing `IsingGraph` and `Symbol` types from 001-rust-core
- Spectral analysis in `physics` module should work on aggregated graphs without changes (already uses adjacency matrix)