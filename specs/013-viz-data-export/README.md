---
status: planned
created: 2026-03-20
priority: high
tags:
- visualization
- cli
- data
depends_on:
- 001-rust-core
- 006-cli-mvp
---

# Visualization Data Export

> **Status**: planned · **Priority**: high · **Created**: 2026-03-20

## Overview

The visualization layer needs a data pipeline. Before any frontend work can begin, the Ising CLI must export the unified graph (nodes, edges, signals) as a single JSON file that the SPA can load statically — no server required. This spec defines the `ising export --format viz-json` command and the JSON schema it produces.

This is the **offline-first** data path. The SPA can also connect to the MCP server for live data (via existing `ising_impact`, `ising_signals`, `ising_hotspots` tools), but the static export is the primary delivery mechanism and the only one required for MVP.

## Design

### CLI Command

```bash
ising export --format viz-json --output ising-viz-data.json
```

Reads from `ising.db` (SQLite) and serializes the full graph state into a single JSON file suitable for the visualization SPA.

### JSON Schema

```jsonc
{
  "meta": {
    "repo": "my-project",              // repository name
    "commit": "abc123",                // HEAD commit at build time
    "built_at": "2025-07-01T12:00:00Z",
    "time_window": "6 months",
    "file_count": 342,
    "signal_count": 28
  },
  "nodes": [
    {
      "id": "src/auth/login.py",       // qualified path (unique key)
      "type": "module",                // module | class | function
      "module": "auth",                // top-level directory group
      "language": "python",
      "loc": 380,
      "complexity": 42,
      "nesting_depth": 5,
      "fan_in": 3,
      "fan_out": 5,
      "change_freq": 34,
      "churn_rate": 2.3,
      "hotspot": 0.87,
      "defect_density": 0.08,
      "bug_count": 4,
      "fix_inducing_rate": 0.15,
      "sum_coupling": 3.2,
      "last_changed": "2025-06-28"
    }
  ],
  "edges": [
    {
      "source": "src/auth/login.py",
      "target": "src/auth/token.py",
      "layer": "structural",           // structural | change | defect
      "type": "imports",               // imports | calls | co_changes | fault_propagates
      "weight": 1.0,
      "metadata": {                    // layer-specific
        "symbols": ["generate_jwt"]
      }
    }
  ],
  "signals": [
    {
      "type": "fragile_boundary",
      "node_a": "src/auth/login.py",
      "node_b": "src/auth/token.py",   // null for node-level signals
      "severity": 0.92,
      "detail": "Structural dep + co-change 0.82 + fault propagation 0.18.",
      "evidence": {
        "structural_edge": true,
        "temporal_coupling": 0.82,
        "fault_propagation": 0.18
      }
    }
  ]
}
```

### Node Fields

All fields are required. Values come from the unified graph in `ising.db`:

- `id` — file path relative to repo root (unique key)
- `type` — node granularity (`module` for file-level nodes in MVP)
- `module` — first path component (e.g., `src/auth/login.py` → `auth`). Used for treemap grouping and color assignment
- `language` — detected language
- `loc`, `complexity`, `nesting_depth` — from Layer 1 (structural)
- `fan_in`, `fan_out` — structural edge counts
- `change_freq`, `churn_rate`, `hotspot`, `bug_count`, `fix_inducing_rate` — from Layer 2 (change)
- `defect_density` — from Layer 3 (defect)
- `sum_coupling` — sum of temporal coupling weights for all edges involving this node
- `last_changed` — date of most recent commit touching this file

### Edge Fields

- `layer` — which analysis layer produced this edge
- `type` — specific relationship type within the layer
- `weight` — edge weight (1.0 for structural, coupling coefficient for change, probability for defect)
- `metadata` — layer-specific details (symbol names for structural, co-change counts for change)

### Module Derivation

The `module` field is derived from the file path using a heuristic:

1. Strip common prefix (e.g., `src/`)
2. Take the first directory component
3. Files at root level get module `"root"`

This produces the top-level grouping for the treemap. The heuristic should be configurable in `ising.toml` for monorepos.

### Performance Constraints

- Export must complete in <5s for repos up to 5,000 files
- JSON output must be <10MB uncompressed for 5,000-file repos (gzip to <1MB for serving)
- All data must be serialized in a single pass from `ising.db`

## Plan

- [ ] Define `VizExport` struct in `ising-core/src/export.rs` (or `ising-cli/src/commands/`) matching the JSON schema
- [ ] Implement `module` derivation from file paths with configurable prefix stripping
- [ ] Query `ising.db` for all file-level nodes with computed metrics
- [ ] Query `ising.db` for all edges across three layers
- [ ] Query `ising.db` for all signals with evidence
- [ ] Assemble meta section from repo info + counts
- [ ] Add `ising export --format viz-json --output <path>` CLI command
- [ ] Add serde serialization with `#[serde(rename_all = "snake_case")]`
- [ ] Validate output against schema for a test repo

## Test

- [ ] Export produces valid JSON parseable by `serde_json::from_str`
- [ ] All node fields are present and non-null for every file in the graph
- [ ] Edge `source` and `target` reference valid node IDs
- [ ] Signal `node_a` references a valid node ID; `node_b` is null for node-level signals
- [ ] `meta.file_count` matches `nodes.len()`
- [ ] `meta.signal_count` matches `signals.len()`
- [ ] Module derivation: `src/auth/login.py` → `auth`, `lib/utils.py` → `utils`, `main.py` → `root`
- [ ] Export completes in <5s for FastAPI-sized repo
- [ ] Round-trip: export then re-import produces same node/edge/signal counts

## Notes

- The `type` field on nodes is `module` for file-level in MVP. When spec 010 (multi-granularity) ships, this will include `class` and `function` nodes.
- Edge `metadata` is intentionally unstructured (`serde_json::Value`) to accommodate layer-specific fields without schema rigidity.
- The static JSON export is the primary data path for CI/CD artifact embedding — teams can generate the viz data in CI and serve the SPA + JSON from a static file host.
- Consider adding `--pretty` flag for human-readable output and compact output by default.
