# Ising Self-Analysis: Analyzing Ising with Ising

> Full analysis and spec: [specs/021-self-analysis/](specs/021-self-analysis/README.md)
> Raw data report: [specs/021-self-analysis/SELF_ANALYSIS.md](specs/021-self-analysis/SELF_ANALYSIS.md)

## Quick Summary (Post-Rust Support)

| Metric | Pre-Rust | Post-Rust |
|--------|----------|-----------|
| Nodes | 51 | **246** |
| Structural edges | 16 | **201** |
| Change edges | 0 | **1** |
| Signals | 0 | **2** |
| Cycles | 0 | 0 |

### Signals Detected

| Signal | Severity | Details |
|--------|----------|---------|
| Ghost Coupling | **1.00** | `change.rs` <-> `structural.rs` — co-change 100%, zero structural dependency |
| Stable Core | 0.10 | `ignore.rs` — well-defined, low-churn foundation module |

### Top 5 Hotspots

| File | Score | Complexity | Freq |
|------|-------|------------|------|
| `ising-builders/src/structural.rs` | **0.85** | 150 | 7 |
| `ising-db/src/lib.rs` | 0.57 | 176 | 4 |
| `ising-cli/src/main.rs` | 0.55 | 113 | 6 |
| `ising-builders/src/change.rs` | 0.28 | 57 | 6 |
| `ising-analysis/src/signals.rs` | 0.23 | 70 | 4 |

### Key Findings

1. **Ghost coupling in the builder pipeline** — `change.rs` and `structural.rs` evolve in lockstep due to an implicit shared schema. Extract shared builder types to make the coupling explicit.
2. **Zero cycles** — Clean DAG architecture enforced by Rust's crate system.
3. **Complexity concentration** — 76% of complexity in 3 files (`structural.rs`, `db/lib.rs`, `main.rs`).
4. **Stable core validated** — `ising-core` modules are low-churn, low-hotspot foundations.

*Analysis performed 2026-03-22. Ising v0.1.0 analyzing itself — turtles all the way down.*
