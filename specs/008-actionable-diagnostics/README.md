---
status: planned
created: 2026-03-04
priority: high
tags:
- cli
- analysis
- ux
depends_on:
- 001-rust-core
- 006-cli-mvp
created_at: 2026-03-04T07:46:00.558169334Z
updated_at: 2026-03-04T07:46:00.558169334Z
---

# Actionable Diagnostics — Explain Why & How to Fix

## Overview

`ising analyze` currently outputs aggregate scores (λ_max, modularity Q, symbol/dependency counts) but gives no insight into **why** the score is what it is or **what to do** about it. For a maintainability tool, the actionable output IS the product — raw numbers require expert interpretation that defeats the purpose.

### Problem

Given the current output:

    λ_max: 2.25, status: critical, modularity_q: ≈0, symbols: 211, dependencies: 769

A developer asks: "Which files should I refactor? Where is the coupling coming from? What would actually improve this?"

### Goal

Extend the analysis pipeline to surface **diagnostics** (explain the score) and **recommendations** (concrete next steps), turning Ising from a measurement instrument into a decision-support tool.

## Design

### 1. Diagnostics — "Why is my score X?"

Three tiers of insight, each building on existing data:

**Hotspot ranking** (already partially exists in text mode):
- Top N symbols by total degree (fan-in + fan-out), with file location and kind
- Expose in JSON output, not just text mode

**Eigenvector centrality** (new — leverages existing power iteration):
- The eigenvector corresponding to λ_max identifies which nodes *drive* the spectral radius
- Already computed as a side-effect of power iteration — just return it alongside λ_max
- Report top N nodes by eigenvector component — these are the symbols whose coupling amplifies change propagation

**File-level coupling summary** (new — aggregation of edge data):
- Group symbols by file, count inter-file vs intra-file edges
- Surface files with highest external coupling ratio
- Shows where module boundaries are leaking

### 2. Recommendations — "What should I do?"

Generate concrete, prioritized suggestions from diagnostics:

| Signal | Recommendation |
|--------|---------------|
| High eigenvector centrality | "Symbol X drives change propagation — consider extracting an interface or reducing its dependents" |
| High fan-in symbol | "Symbol Y has N dependents — changes here ripple widely. Consider stabilizing its API." |
| High fan-out symbol | "Symbol Z depends on N others — it knows too much. Consider splitting responsibilities." |
| File with high external coupling | "File F.rs has N% external dependencies — consider extracting a facade or splitting." |
| Low modularity Q | "Code lacks modular structure — consider grouping related symbols behind module boundaries." |

### 3. Output Structure

Extend `AnalyzeReport` with optional `diagnostics` section:

    {
      "health": { ... },
      "summary": { ... },
      "diagnostics": {
        "hotspots": [
          { "symbol": "pkg::Widget::render", "file": "src/widget.rs", "kind": "Function",
            "fan_in": 23, "fan_out": 5, "eigenvector_score": 0.87 }
        ],
        "file_coupling": [
          { "file": "src/widget.rs", "internal_edges": 12,
            "external_edges": 45, "coupling_ratio": 0.79 }
        ],
        "recommendations": [
          { "severity": "high", "category": "coupling",
            "message": "...", "target": "src/widget.rs" }
        ]
      }
    }

### Architecture

- **ising-core/src/physics**: Extend `detect_phase_transition` to return eigenvector alongside λ_max (or add a new function). Add `diagnostics` module.
- **ising-core/src/diagnostics** (new module): Hotspot analysis, file coupling, recommendation engine. Takes `&IsingGraph` + physics results, produces structured diagnostics.
- **ising-cli**: Integrate diagnostics into report. Render in both JSON and text formats.

## Plan

- [ ] Extend power iteration to return eigenvector alongside λ_max
- [ ] Create `ising-core/src/diagnostics/` module with hotspot and file-coupling analysis
- [ ] Build recommendation engine that maps signals to actionable messages
- [ ] Extend `AnalyzeReport` with diagnostics section (JSON + text rendering)
- [ ] Move `top_hotspots` and `estimate_modularity_q` from CLI into core
- [ ] Add unit tests for diagnostics with known graph topologies

## Test

- [ ] Eigenvector returned by power iteration matches expected values for small known graphs
- [ ] Hotspot ranking correctly identifies highest-degree nodes
- [ ] File coupling correctly counts inter vs intra-file edges
- [ ] Recommendations fire for expected signals (high centrality, high fan-in, low Q)
- [ ] JSON output includes diagnostics section with correct schema
- [ ] Text output renders diagnostics in human-readable form
- [ ] `cargo test` passes all existing + new tests

## Notes

- The eigenvector data is essentially free — power iteration already converges on it, we just discard it today
- File coupling analysis uses only `Symbol.file` and edge data, no new indexing needed
- Recommendation text should be opinionated but not prescriptive — suggest patterns, don't mandate them
- Keep recommendation count bounded (top 5-10) to avoid overwhelming output
- Future: could add `--diagnostics=off` flag for CI pipelines that only want the exit code