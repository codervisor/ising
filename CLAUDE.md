# Ising -- AI Agent Guide

## What This Project Is

Ising is a Rust monorepo that analyzes codebases for maintainability risk. It builds a multi-layer graph (structural + change + defect), computes per-module risk scores and safety factors, detects cross-layer signals, and serves results via CLI and MCP server.

## Build and Test

```bash
cargo build                    # Build all crates
cargo test --workspace         # Run all 141 tests
cargo clippy --workspace       # Lint check
cargo fmt --check              # Format check
```

## Crate Map

| Crate | Path | Purpose |
|-------|------|---------|
| `ising-core` | `ising-core/` | Types: `UnifiedGraph`, `Config`, `SafetyZone`, `NodeRisk`, `RiskField`, `LoadCase` |
| `ising-builders` | `ising-builders/` | Graph construction: Tree-sitter parsing (`structural.rs`), git history (`change.rs`), language parsers (`languages/`) |
| `ising-analysis` | `ising-analysis/` | Risk computation (`stress.rs`), signal detection (`signals.rs`), hotspot ranking (`hotspots.rs`) |
| `ising-db` | `ising-db/` | SQLite storage: schema, queries, graph persistence, export |
| `ising-cli` | `ising-cli/` | CLI binary: `build`, `safety`, `simulate`, `signals`, `hotspots`, `serve` commands |
| `ising-server` | `ising-server/` | HTTP/MCP server: `/safety`, `/simulate`, `/signals`, `/hotspots` endpoints |
| `ising-scip` | `ising-scip/` | SCIP index loader (alternative to Tree-sitter for supported languages) |

## Key Files

When working on risk analysis, these are the critical files:

- **`ising-analysis/src/stress.rs`** -- Core risk engine. Contains `compute_risk_field()`, `propagate_risk()`, `simulate_load_case()`, `compare_risk_fields()`. This is where the math lives.
- **`ising-core/src/fea.rs`** -- Risk types: `SafetyZone`, `NodeRisk`, `RiskField`, `RiskDelta`, `LoadCase`.
- **`ising-core/src/config.rs`** -- `FeaConfig` with damping, epsilon, max_iterations.
- **`ising-core/src/graph.rs`** -- `UnifiedGraph`, `Node`, `EdgeType`, graph operations.
- **`ising-db/src/queries.rs`** -- `store_risk_field()`, `get_safety_ranking()`, `load_graph()`.
- **`ising-cli/src/main.rs`** -- CLI command implementations and output formatting.

## Risk Model (How It Works)

Each module gets:
1. **change_load** [0, 1+] -- `normalize(change_freq * churn_rate)` against graph max
2. **capacity** [0.05, 1.0] -- `1.0 - (complexity*0.4 + instability*0.3 + coupling*0.3)`
3. **propagated_risk** -- from neighbors via Jacobi iteration on co-change + import edges
4. **risk_score** -- `change_load + propagated_risk`
5. **safety_factor** -- `capacity / risk_score` (clamped to [0, 10])
6. **zone** -- Critical (<1.0), Danger (1.0-1.5), Warning (1.5-2.0), Healthy (2.0-3.0), Stable (>3.0)

Propagation normalizes per-node incoming weights to sum <= 0.95, ensuring convergence.

## Safety Zones (enum `SafetyZone`)

```
Critical  -- SF < 1.0  -- risk exceeds capacity
Danger    -- SF 1.0-1.5 -- thin margin
Warning   -- SF 1.5-2.0 -- caution
Healthy   -- SF 2.0-3.0 -- good
Stable    -- SF > 3.0   -- low risk, not a concern
```

## Signals

Cross-layer anomalies detected in `ising-analysis/src/signals.rs`:
- **GhostCoupling** -- files co-change but have no structural dependency
- **DependencyCycle** -- circular imports
- **GodModule** -- extreme complexity + fan-out
- **UnstableDependency** -- stable module depends on volatile one
- **StableCore** -- high fan-in, low change, protect it
- **UnnecessaryAbstraction** -- structural dep exists but files never co-change

## Conventions

- **Workspace**: shared deps in root `Cargo.toml` via `[workspace.dependencies]`
- **Testing**: unit tests inside each module, integration tests in `ising-db` and `ising-analysis`
- **Error handling**: `thiserror` for library crates, `anyhow` for CLI
- **DB**: SQLite via `rusqlite`. Schema in `ising-db/src/schema.rs`. FK enforcement via `PRAGMA foreign_keys`.
- **Graph**: `UnifiedGraph` is the in-memory model. Stored in SQLite for persistence. Both representations must stay in sync.

## Adding a New Language Parser

1. Add grammar dependency to `ising-builders/Cargo.toml`
2. Create `ising-builders/src/languages/<lang>.rs` implementing the `LanguageParser` trait
3. Register it in `ising-builders/src/languages/mod.rs`
4. Add file extension mapping in `ising-builders/src/structural.rs`
