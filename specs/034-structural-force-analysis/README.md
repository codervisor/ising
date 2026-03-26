---
status: draft
created: 2026-03-26
priority: high
tags:
- architecture
- visualization
- force-analysis
- simulation
- fea
depends_on:
- 033-self-analysis-round2-signal-improvements
created_at: 2026-03-26T10:00:00.000000000Z
updated_at: 2026-03-26T10:00:00.000000000Z
transitions: []
---

# Structural Force Analysis: FEA for Codebases

## Overview

Reframe Ising from a "signal detector" into a **structural force analysis and simulation engine** — the software equivalent of Finite Element Analysis (FEA) tools like ANSYS, COMSOL, and Abaqus. Instead of listing discrete signals, Ising computes continuous **stress fields** across the codebase, runs **load case simulations** ("what if we change module X?"), and renders results using **FEA-style visualization** with scientific colormaps, deformation overlays, and force flow diagrams.

### Core Analogy

| Mechanical Engineering | Code Engineering |
|---|---|
| Structure (bridge, hull, wing) | Codebase (modules, classes, functions) |
| Elements (beams, plates, shells) | Code units (files, classes, functions) |
| Material properties (stiffness, yield strength) | Code properties (complexity, test coverage, churn history) |
| External loads (forces, pressure, wind) | Change pressure (features, bugs, refactors, dep updates) |
| Internal stress (σ = F/A) | Coupling stress (change_pressure / capacity) |
| Deformation / strain | Architecture drift |
| Fatigue (cyclic loading) | Churn-induced code rot |
| Safety factor (σ_yield / σ_actual) | Architectural headroom |
| Modal analysis (vibration modes) | Change propagation eigenmodes |
| Buckling (sudden collapse) | Ising phase transition (λ_max ≥ 1.0) |

## Requirements

### 1. Material Property Model (`ising-core`)
- [ ] Define `MaterialProperties` per node: `stiffness` (complexity × coupling — resistance to change), `yield_strength` (test coverage × API stability — capacity before breaking), `fatigue_life` (inverse churn rate — remaining endurance), `cross_section` (API surface area — fan-in + fan-out)
- [ ] Compute material properties from existing metrics during graph build
- [ ] Nodes with no test coverage data get conservative defaults (low yield strength)

### 2. Stress Tensor Computation (`ising-analysis`)
- [ ] Compute per-node **stress** (σ): `change_pressure / (yield_strength × cross_section)` where `change_pressure` = change_freq × avg_churn_per_change
- [ ] Compute **tensile stress**: fan-out strain (module pulled in many directions by consumers)
- [ ] Compute **compressive stress**: responsibility overload (high LOC × high complexity × high CBO)
- [ ] Compute **Von Mises equivalent stress**: single scalar combining tensile + compressive for ranking
- [ ] Compute per-node **safety factor**: `yield_strength / von_mises_stress` — the primary health metric
- [ ] Stress propagates through coupling edges: neighbor stress contributes weighted by coupling strength

### 3. Load Case Simulation (`ising-analysis`)
- [ ] Define `LoadCase` struct: a set of `(node_id, pressure_magnitude)` pairs representing hypothetical changes
- [ ] `simulate_load_case(graph, load_case) -> StressField` — propagates load through coupling graph, returns per-node stress
- [ ] Built-in load case generators: `single_file_change(path)`, `module_change(module_prefix)`, `dependency_upgrade(dep_name)`
- [ ] Comparison API: `compare_stress_fields(before, after) -> StressDelta` for evaluating refactoring impact
- [ ] Integrate with existing `impact` CLI command: `ising impact <target>` now shows stress distribution, not just reachable nodes

### 4. Safety Factor Analysis (`ising-analysis`)
- [ ] Classify nodes into zones: **Critical** (SF < 1.0), **Danger** (1.0–1.5), **Warning** (1.5–2.0), **Healthy** (2.0–3.0), **Over-engineered** (> 3.0)
- [ ] Replace current signal severity (INFO/HIGH/CRITICAL) with safety factor ranges — signals become stress concentration explanations
- [ ] `ising safety` CLI command: ranked list of nodes by safety factor, lowest first
- [ ] `ising simulate <load-case-file>` CLI command: apply a load case JSON and output resulting stress field

### 5. FEA-Style Visualization (`ising-viz`)
- [ ] **Stress heatmap view**: Force-directed graph layout with nodes colored by Von Mises stress using a scientific colormap (viridis or plasma). Smooth color interpolation. Color scale legend with SF zones.
- [ ] **Deformation overlay**: Show "intended architecture" (module containment tree) as wireframe, overlay actual coupling graph with displacement proportional to architectural drift (ghost couplings = invisible deformation forces)
- [ ] **Force flow diagram**: Directed edges with thickness proportional to coupling strength, arrows showing change propagation direction. Reveals load-bearing paths.
- [ ] **Fatigue life map**: Nodes colored by estimated remaining endurance (churn trend extrapolation). Red = near fatigue failure.
- [ ] **Load case comparison**: Side-by-side stress fields for two scenarios (current vs. proposed refactoring)
- [ ] **Interactive probe**: Click any node to see full stress breakdown (σ_tensile, σ_compressive, σ_von_mises, SF, material properties, load contributions)
- [ ] Use WebGL (Three.js or Deck.gl) for GPU-accelerated rendering of large graphs

### 6. Export Formats (`ising-db`)
- [ ] `viz-json` export includes stress field data: per-node stress tensor, safety factor, material properties
- [ ] `load-case` JSON import/export format for defining and sharing simulation scenarios
- [ ] Stress field data persisted in SQLite for historical comparison across builds

## Non-Goals

- Real-time live analysis (batch build model is sufficient for now)
- 3D spatial layout of code (2D force-directed graph with color mapping suffices)
- Replacing existing signals — they become "failure mode explanations" attached to stress concentrations
- Physics-accurate FEA solver (we use graph-based stress propagation, not PDE solvers)

## Technical Notes

- **Stress propagation model**: Use iterative relaxation on the coupling graph. Each iteration, a node's stress is `local_stress + Σ(neighbor_stress × coupling_weight × damping)`. Converge when max delta < ε. This is analogous to Jacobi iteration in FEA but on a graph, not a mesh.
- **Material properties from existing data**: `stiffness` = normalized(complexity) × normalized(CBO). `yield_strength` = baseline × (1 + test_coverage_ratio). `cross_section` = fan_in + fan_out. `fatigue_life` = max_churn_rate / actual_churn_rate.
- **Colormap**: Use viridis (perceptually uniform, colorblind-safe) as default. Offer jet/plasma as alternatives. Map SF to color: SF=0 → dark red, SF=1 → red, SF=2 → yellow, SF=3 → green, SF>3 → blue.
- **Backward compatibility**: Existing `signals` command still works but now shows stress context. Existing `hotspots` maps to stress ranking. The `impact` command gains stress propagation.
- **Rendering**: Migrate `ising-viz` from simple React components to Three.js force-directed graph with WebGL shaders for smooth color interpolation. Use `three-forcegraph` or `force-graph` library.

## Acceptance Criteria

- [ ] `ising build` computes material properties and stress field for all nodes
- [ ] `ising safety` outputs ranked safety factor list with zone classification
- [ ] `ising simulate load-case.json` applies a load case and outputs stress delta
- [ ] `ising-viz` renders stress heatmap with viridis colormap and interactive probe
- [ ] Load case comparison view shows side-by-side stress distributions
- [ ] Self-analysis on ising repo: known hotspots (signals.rs, main.rs) show lowest safety factors
- [ ] Stress propagation converges in < 100 iterations for repos up to 10k nodes
