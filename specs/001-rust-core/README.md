---
status: complete
created: 2026-03-04
priority: high
tags:
- phase-1
- core
created_at: 2026-03-04T01:48:18.777757472Z
updated_at: 2026-03-04T05:23:51.068902715Z
completed_at: 2026-03-04T05:23:51.068902715Z
transitions:
- status: complete
  at: 2026-03-04T05:23:51.068902715Z
---

# Rust Core — Spectral Engine

## Overview

The foundation of Ising: a Rust workspace (`ising-core`) implementing the spectral analysis engine. Constructs an adjacency matrix from a directed dependency graph and computes the spectral radius (λ_max) via power iteration to classify codebase health as Stable or Critical.

## Design

- **Graph model** (`graph` module): `IsingGraph` wraps `petgraph::DiGraph` with `Symbol` nodes and dependency edges. Provides `to_adjacency_matrix()` for the physics layer.
- **Physics engine** (`physics` module): `detect_phase_transition()` computes λ_max using power iteration on the adjacency matrix. Returns `HealthScore::Stable` or `HealthScore::Critical`.
- Dependencies: petgraph, ndarray, rayon, serde, thiserror.

## Plan

- [x] Initialize Rust workspace with `ising-core` crate
- [x] Define `Symbol`, `SymbolKind`, `IsingGraph` types
- [x] Implement adjacency matrix conversion
- [x] Implement power iteration for λ_max
- [x] Define `HealthScore` enum and `detect_phase_transition()`
- [x] Unit tests for graph and physics modules
- [x] Add error types with thiserror

## Test

- [x] Empty graph returns Stable with λ=0
- [x] Simple chain (2 nodes, 1 edge) returns Stable
- [x] Fully connected 4-node graph returns Critical

## Notes

- Power iteration chosen over LAPACK to avoid external C library dependency, keeping builds portable.
- ndarray-linalg omitted intentionally; can be added later if full eigendecomposition is needed.