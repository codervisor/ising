---
status: planned
created: 2026-03-04
priority: high
tags:
- theory
- core
- documentation
created_at: 2026-03-04T07:59:48.160825671Z
updated_at: 2026-03-04T07:59:48.160825671Z
---

# Spectral Health Thesis — Why λ_max and Modularity Q Measure Code Health

## Overview

Ising treats codebases as physical systems and applies spectral graph theory to quantify architectural health. This spec formalizes the thesis: **why** these metrics work, **what** they measure, and **how** they combine into a health report.

This is the conceptual foundation that every other spec builds on.

## The Analogy: Code as a Physical Lattice

In the Ising Model from statistical physics, a lattice of "spins" interact locally. When interaction strength exceeds a critical threshold, the system undergoes a **phase transition** — small perturbations suddenly propagate across the entire lattice instead of staying local.

Software dependency graphs behave the same way:

| Physics | Software |
|---------|----------|
| Spin | Code symbol (function, class, module) |
| Local interaction | Dependency edge (A calls/imports B) |
| Perturbation | Code change, bug, refactor |
| Phase transition | Tipping point where changes cascade system-wide |
| Ordered phase (stable) | Modular code — changes stay local |
| Disordered phase (critical) | "Big Ball of Mud" — changes ripple everywhere |

## Metric 1: Spectral Radius λ_max

### What it measures

The spectral radius of the adjacency matrix $A$ of the dependency graph:

$$\lambda_{max}(A) = \max \\{ |\lambda| : \det(A - \lambda I) = 0 \\}$$

λ_max captures the **maximum amplification factor** of the system. It answers: "If I perturb one symbol, how much does that perturbation grow as it propagates through dependencies?"

### Interpretation

- **λ < 1.0 (Stable)**: Perturbations decay exponentially. After $k$ hops through the dependency chain, impact scales as $\lambda^k \to 0$. Changes stay local. Architecture is healthy.
- **λ ≥ 1.0 (Critical)**: Perturbations sustain or amplify. Impact scales as $\lambda^k \to \infty$. A bug in one module statistically cascades into others. Architecture is fragile.

### Why 1.0 is the threshold

The threshold is not arbitrary — it's a mathematical phase transition. Below 1.0, the geometric series $\sum \lambda^k$ converges (finite impact). At or above 1.0, it diverges (unbounded impact). This is identical to the criticality condition in epidemic models ($R_0 = 1$) and reactor physics.

### What drives λ_max up

- Hub symbols with many dependents (high fan-in)
- Symbols that depend on many others (high fan-out)
- Dense clusters with bidirectional dependencies (cycles)
- Lack of module boundaries that would partition the graph

## Metric 2: Modularity Q

### What it measures

How well the dependency graph decomposes into self-contained communities. Computed via community detection (connected components / Louvain):

$$Q = \frac{1}{2m} \sum_{ij} \left[ A_{ij} - \frac{k_i k_j}{2m} \right] \delta(c_i, c_j)$$

Where $m$ = edge count, $k_i$ = degree of node $i$, $\delta$ = 1 if nodes share a community.

### Interpretation

- **Q → 1.0**: Strong modular structure. Symbols interact mostly within their module, rarely across.
- **Q → 0.0**: Random structure. No meaningful module boundaries — the "Big Ball of Mud."
- **Q < 0**: Anti-modular. Symbols interact more across boundaries than within — pathological.

### Relationship to λ_max

These metrics are complementary, not redundant:
- λ_max measures **propagation risk** (dynamic: what happens when something changes)
- Q measures **structural quality** (static: how well-organized is the code right now)

A codebase can have low λ_max but low Q (small but unstructured), or high λ_max but moderate Q (well-partitioned but with a few critical cross-cutting concerns).

## Combined Health Report

| λ_max | Q | Diagnosis |
|-------|---|-----------|
| < 1.0 | > 0.3 | Healthy — modular, changes stay local |
| < 1.0 | < 0.3 | Stable but unstructured — invest in module boundaries |
| ≥ 1.0 | > 0.3 | Fragile despite structure — find and decouple the hub symbols driving λ |
| ≥ 1.0 | < 0.3 | Critical — systemic coupling, prioritize architectural intervention |

## Validation Approach

The thesis makes testable predictions:
- A fully connected graph of N nodes should have λ = N-1 (critical for N ≥ 3)
- A chain graph (A→B→C) should have λ < 1 (stable)
- Adding hub nodes to a stable graph should push λ toward and past 1.0
- Removing edges between modules should decrease λ and increase Q

These are verified by unit tests in the physics module.

## Notes

- λ_max is computed via power iteration (no LAPACK dependency), which converges to the dominant eigenvalue for non-negative matrices
- The eigenvector associated with λ_max identifies which nodes contribute most to instability (see 008-actionable-diagnostics)
- Future work: spectral gap (λ_1 - λ_2) as a measure of how "close" the system is to the next phase transition
- The 1.0 threshold applies to unnormalized adjacency matrices; normalized variants (e.g., dividing by degree) would shift the threshold but preserve the phase transition semantics