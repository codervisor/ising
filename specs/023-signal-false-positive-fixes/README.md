---
status: draft
created: 2026-03-22
priority: medium
tags:
- signal-quality
- false-positives
- self-analysis
depends_on:
- '021'
- '022'
created_at: 2026-03-22T13:40:00Z
updated_at: 2026-03-22T13:40:00Z
---

# Fix Signal False Positives Found in Self-Analysis

> **Status**: draft · **Priority**: medium · **Created**: 2026-03-22

## Overview

After implementing spec 022 (builder decomposition), re-running Ising on itself produces 10 signals — 3 of which are false positives. These reveal structural gaps in the signal detection logic that will affect all users, not just self-analysis.

### Current signal output (post-spec-022)

```
  [1.00] ghost_coupling: ising-builders/src/change.rs <-> ising-builders/src/structural.rs
  [0.83] ghost_coupling: ising-builders/src/structural.rs <-> ising-db/src/lib.rs
  [0.40] over_engineering: ising-builders/src/lib.rs <-> ising-builders/src/languages/mod.rs
  [0.10] stable_core: ising-db/src/export.rs
  [0.10] stable_core: ising-builders/src/common.rs
  [0.10] stable_core: ising-builders/src/languages/mod.rs
  [0.10] stable_core: ising-builders/src/languages/python.rs
  [0.10] stable_core: ising-core/src/ignore.rs
  [0.10] stable_core: ising-builders/src/languages/typescript.rs
  [0.10] stable_core: ising-db/src/queries.rs
```

## Analysis of False Positives

### FP1: GhostCoupling — `change.rs` ↔ `structural.rs` (severity 1.00)

**Why it fires:** These files have 100% co-change rate and no direct structural edge between them (neither imports the other).

**Why it's a false positive:** Both files are **sibling modules** orchestrated by a common parent (`ising-builders/src/lib.rs`). The `build_all()` function in `lib.rs` calls `structural::build_structural_graph()` then `change::build_change_graph()` and merges the results. They co-change because they share the same parent module and both depend on `common.rs` — not because of a hidden dependency. Neither file needs to know about the other.

**Root cause in detection logic:** Ghost coupling only checks for a direct structural edge between A and B (`has_structural_edge(a, b)`). It does not consider **common-parent relationships** — when A and B are both imported by the same orchestrator C, their co-change is explained by the shared parent, not by a hidden dependency.

**Pattern:** Sibling modules in a fan-out architecture (A←C→B) naturally co-change when C is modified, but A and B have no reason to import each other. This is correct architecture, not a missing abstraction.

### FP2: GhostCoupling — `structural.rs` ↔ `ising-db/src/lib.rs` (severity 0.83)

**Why it fires:** These files have 83% co-change rate and no structural edge between them.

**Why it's a false positive:** Same pattern as FP1 but across crate boundaries. `ising-cli/src/main.rs` (or the build pipeline) uses both `ising-builders` and `ising-db`. They co-change because feature work touches both the graph building and storage layers — this is expected in a layered architecture where the CLI orchestrates both. There is no hidden dependency; they communicate only through the `UnifiedGraph` type from `ising-core`.

**Root cause in detection logic:** Same as FP1 — no common-parent check. Additionally, the detection doesn't account for **shared data type coupling**: when two modules both depend on the same core type (`UnifiedGraph`), changes to the type's shape naturally cause co-changes in both consumers.

### FP3: OverEngineering — `lib.rs` ↔ `languages/mod.rs` (severity 0.40)

**Why it fires:** `languages/mod.rs` has fan-in=1 (only `lib.rs` imports it directly), complexity ≤ 5, and change_freq ≤ 1. This matches the "single-consumer wrapper" pattern.

**Why it's a false positive:** `languages/mod.rs` is a **Rust module barrel file** — it exists solely to declare submodules (`pub mod python; pub mod rust_lang; pub mod typescript;`) and define shared types (`FileAnalysis`, `FunctionInfo`, etc.). This is the idiomatic Rust module pattern, equivalent to `__init__.py` or `index.ts`. The actual consumers are the submodules and `structural.rs` (which uses `crate::languages::python` etc.).

**Root cause in detection logic:** The `is_reexport_module()` filter only checks for `__init__.py`, `index.ts`, and `index.js`. It does not recognize `mod.rs` or barrel-file `lib.rs` patterns in Rust. These are structurally identical to `__init__.py` — they re-export submodules without containing significant logic.

### Stable Core signals — NOT false positives

The 6 `stable_core` signals are **true positives** and correctly identify foundational modules:
- `common.rs`, `languages/mod.rs`, `python.rs`, `typescript.rs` — new stable modules from spec 022
- `export.rs`, `queries.rs` — newly extracted DB modules
- `ignore.rs` — core utility

These have high fan-in and low change frequency, which is exactly the profile of stable foundations.

## Design

### Fix 1: Common-parent suppression for GhostCoupling

Add a check to the ghost coupling detector: if both A and B are imported by a common parent C (i.e., C→A and C→B structural edges exist), suppress the ghost coupling signal.

**Location:** `ising-analysis/src/signals.rs`, ghost coupling detection block (lines 81-99).

**Algorithm:**
1. Precompute a map of `node → set of structural importers` from import edges.
2. Before emitting a ghost coupling signal for (A, B), check if `importers(A) ∩ importers(B)` is non-empty.
3. If they share a common parent, skip the signal.

**Edge case:** If A and B share a parent but ALSO have very high coupling (≥ 0.9), still emit the signal but at reduced severity (× 0.3) with an amended description: "Co-change likely explained by shared parent {C}, but coupling is very high — verify no hidden dependency."

### Fix 2: Rust barrel-file recognition for OverEngineering

Extend `is_reexport_module()` to recognize Rust module patterns.

**Location:** `ising-analysis/src/signals.rs`, `is_reexport_module()` function (line 392-395).

**Changes:**
- Add `mod.rs` to the recognized filenames (Rust's equivalent of `__init__.py`).
- Add a heuristic for `lib.rs` files that are barrel modules: if the file's only structural edges are `Contains` and re-exports (no complex logic), treat it as a re-export module.
- Simplest correct fix: just add `"mod.rs"` to the filename check, since `mod.rs` files in Rust are almost always barrel files.

### Fix 3: (Optional) Shared-type co-change annotation

When two modules both import from a common core type crate (e.g., both depend on `ising-core`), annotate ghost coupling signals to note that co-change may be explained by shared type evolution. This is lower priority — Fix 1 (common-parent) handles the most common case.

## Implementation Plan

### Part 1: Precompute importer index
- In `detect_signals()`, build a `HashMap<&str, HashSet<&str>>` mapping each node to the set of nodes that import it.
- This reuses the existing `import_edges` iteration and adds ~5 lines.

### Part 2: Common-parent check in ghost coupling
- Before pushing a `GhostCoupling` signal, check if `importers[a] ∩ importers[b]` is non-empty.
- If shared parent exists and coupling < 0.9: skip signal entirely.
- If shared parent exists and coupling ≥ 0.9: emit at severity × 0.3 with amended description.
- If no shared parent: emit as before.

### Part 3: Extend `is_reexport_module()`
- Add `"mod.rs"` to the filename check.
- This is a one-line change.

### Part 4: Add tests
- Test: sibling modules with common parent should NOT trigger ghost coupling.
- Test: sibling modules with common parent and coupling ≥ 0.9 should trigger at reduced severity.
- Test: unrelated modules with no common parent should still trigger ghost coupling (no regression).
- Test: `mod.rs` should be recognized as a reexport module.
- Test: `lib.rs` should NOT be blanket-recognized (it may contain real logic).

### Part 5: Re-run self-analysis
- Verify that the 3 false positives are eliminated.
- Verify that the 6 stable_core signals remain unchanged.
- Verify that no new false negatives are introduced.

## Expected outcome after fixes

```
  [0.10] stable_core: ising-db/src/export.rs
  [0.10] stable_core: ising-builders/src/common.rs
  [0.10] stable_core: ising-builders/src/languages/mod.rs
  [0.10] stable_core: ising-builders/src/languages/python.rs
  [0.10] stable_core: ising-core/src/ignore.rs
  [0.10] stable_core: ising-builders/src/languages/typescript.rs
  [0.10] stable_core: ising-db/src/queries.rs
```

7 true signals, 0 false positives.

## Acceptance criteria

- [ ] Self-analysis produces 0 ghost_coupling signals for sibling modules with a common parent
- [ ] Self-analysis produces 0 over_engineering signals for `mod.rs` barrel files
- [ ] All existing signal detection tests continue to pass
- [ ] New tests cover common-parent suppression and `mod.rs` recognition
- [ ] No regression: ghost coupling still fires for genuinely unrelated co-changing modules
