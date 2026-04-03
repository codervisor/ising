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
1. **change_load** [0, 1+] -- `normalize(defect_churn*3 + feature_churn)` against graph max (falls back to `change_freq * churn_rate` when no classification data)
2. **capacity** [0.05, 1.0] -- `1.0 - (complexity*0.4 + instability*0.3 + coupling*0.3)`
3. **propagated_risk** -- from neighbors via Jacobi iteration on co-change + import edges
4. **risk_score** -- `change_load + propagated_risk`
5. **safety_factor** -- `capacity / risk_score` (clamped to [0, 10])
6. **zone** -- Critical (<1.0), Danger (1.0-1.5), Warning (1.5-2.0), Healthy (2.0-3.0), Stable (>3.0)
7. **direct_score** -- `change_load / capacity` (local risk without propagation, basis for tier classification)
8. **risk_tier** -- Percentile-based: Critical (top 1%), High (top 5%), Medium (top 15%), Normal (rest)

Propagation normalizes per-node incoming weights to sum <= 0.95, ensuring convergence.

## Health Index

FEA-aligned scoring using safety factor zone fractions, structural coupling (λ_max), boundary containment, and signal penalty (see `compute_health_index` in `stress.rs`):

**Formula** (spec 047, fully multiplicative + tail risk cap):

```
base = zone_sub_score × coupling_modifier × containment_modifier × signal_factor
score = min(base, tail_risk_cap)
```

| Component | Formula | Range |
|-----------|---------|-------|
| Zone sub-score | Weighted avg: Stable×1.0 + Healthy×0.90 + Warning×0.65 + Danger×0.35 + Critical×0.15, with small-sample blend toward 0.75 prior for <50 active modules | [0, 1] |
| Coupling modifier | Uses normalized λ (λ/√N). norm<1: `1.0 + (1−norm)×0.05`, norm>1: `1.0 − log₂(norm)×0.05` (clamped [0.85, 1.05]) | [0.85, 1.05] |
| Containment modifier | `0.70 + 0.35 × avg_containment` (clamped [0.70, 1.05]). Only applied when boundary health is computed. | [0.70, 1.05] |
| Signal factor | `1.0 − signal_penalty`. Penalty uses adaptive piecewise curve: x≤5: `0.25×x/(x+3)`, x>5: `0.156 + 0.094×log₂(x/5)` (cap 0.30) | [0.70, 1.0] |
| Tail risk cap | Basel II / Moody's "minimum function": if any non-test node's Expected Loss (`direct_score × (1 + fan_in/max_fan_in)`) > 5.0, score capped at 0.84 (B ceiling). Computed on full risk field including function-level nodes. | floor at 0.84 |

**λ_max** (spectral radius of structural Import graph, unit weights):
- Raw λ_max is always >>1 for real codebases (hub modules with many imports)
- Normalized: λ/√N measures coupling density relative to codebase size
- norm < 1.0: loosely coupled → slight bonus
- norm > 1.0: tightly coupled → gentle penalty

Legacy sub-scores (risk, signal, structural) are still computed for backward compatibility but no longer drive the grade.

Grade thresholds: A ≥ 0.85, B ≥ 0.70, C ≥ 0.55, D ≥ 0.40, F < 0.40.

### Known detector blind spots

- **Odoo gets A** -- god module thresholds (complexity≥50, LOC≥500, CBO≥15) miss distributed complexity across thousands of moderately-complex files. `SystemicComplexity` signal detects elevated median/P75 complexity but fires 0 times for Odoo. The modules are moderately complex but individually below thresholds. This is a genuine detection gap — would require a "ratio of modules above moderate complexity" metric to fix. **Status**: known limitation, documented.
- **Go repos over-penalized** -- package-level imports inflate ghost coupling and unnecessary abstraction counts (GAP-13). **Addressed**: Go intra-package pairs now suppressed in both ghost coupling and unnecessary abstraction detection.
- **Cross-crate call resolution blind** -- In Rust workspaces, `pub fn` called from another crate shows as orphan function because call resolution is intra-file only. Same applies to JS/TS cross-file imports. **Tracked**: spec 041 Phase 3A/3B.
- **Dispatch/callback calls invisible** -- Functions called via match dispatch, passed as callbacks, or invoked through trait objects don't create `Calls` edges. Inherent limitation of AST-based static analysis.
- **Code duplication not measured** -- Copy-paste code, near-clones. External tool integration (jscpd) designed but not implemented. **Tracked**: spec 041 Phase 3C.
- **API stability not measured** -- breaking changes, deprecation frequency, interface churn are invisible to the tool.

## Analytical Discipline

**These rules apply when modifying the risk model, health index, signal detection, or any scoring formula.**

### Validate empirically, don't rationalize

- **Never cite research to justify a number you already picked.** That's rationalization. Instead: pick a number, run it against real repos, check for absurdities, adjust, document what broke.
- **Run every formula change against the benchmark test set** using `./scripts/bench-oss-repos.sh` (see the script for the current repo list and language coverage). Clone repos first with `--clone` flag. Check the output table. If gin gets below B or odoo gets A with no caveats, something is wrong. See `specs/042-oss-validation-round5-expanded/README.md` for the full validation report and calibration targets.
- **Look for the cases that break your theory**, not the ones that confirm it. Selection bias means you'll always find supporting examples.

### Statistical pitfalls to avoid

- **Mean vs median**: Always use median for per-module aggregates. Mean is dominated by outliers, especially in small repos (gin: 98 modules, one file inflated mean from 0.05 to 0.52).
- **Density (count/N) is NOT scale-invariant**: It lets large repos hide problems behind a big denominator. Use **sqrt(N)** normalization (same principle as TF-IDF). `count/sqrt(N)` is sub-linear: penalizes large repos more than density, small repos less.
- **Amplification factors shift bias, they don't remove it**: The 5x amplification on avg_direct_score that helps large repos discriminate will destroy small repo scores. Test at both ends of the scale range.
- **Percentile-based metrics are self-referential by construction**: Top 1% is always critical regardless of absolute quality. This is a feature (bounded false positive rate) and a limitation (can't compare across repos by tier counts).

### Be honest about what we measure

- **Grade A means**: "Low median change-risk, few detectable architectural signals relative to codebase size."
- **Grade A does NOT mean**: "Good codebase" or "low defect rate." Those claims require defect histories, maintenance cost data, or developer surveys we don't have.
- **Always emit caveats** when data is insufficient (<5% active modules, no defect data, no signals in large repo).
- **Document unfixed biases** alongside fixed ones. The spec should say what's still wrong, not just what was improved.

### When adding or changing signal weights

- All signal type weights (cycles=4x, god_modules=3x, ghost_coupling=1x, etc.) are **engineering judgment, not empirically validated**. The ordering is defensible; the magnitudes are arbitrary.
- To validate: correlate signal presence with actual defect rates across repos. Until that study is done, treat weights as tunable assumptions, not ground truth.
- Never adjust weights to make a specific repo "look right." That's overfitting to one data point.

## Safety Zones (legacy) and Risk Tiers (current)

Legacy zones (hard-coded thresholds, kept for backward compatibility):
```
Critical  -- SF < 1.0  -- risk exceeds capacity
Danger    -- SF 1.0-1.5 -- thin margin
Warning   -- SF 1.5-2.0 -- caution
Healthy   -- SF 2.0-3.0 -- good
Stable    -- SF > 3.0   -- low risk, not a concern
```

Risk tiers (auto-calibrated, primary classification):
```
Critical  -- Top 1% by direct_score  -- immediate attention
High      -- Top 1-5%                -- elevated risk
Medium    -- Top 5-15%               -- moderate risk
Normal    -- Bottom 85%              -- normal
```

## Signals

Cross-layer anomalies detected in `ising-analysis/src/signals.rs`:
- **GhostCoupling** -- files co-change but have no structural dependency
- **DependencyCycle** -- circular imports
- **GodModule** -- extreme complexity + fan-out
- **UnstableDependency** -- stable module depends on volatile one
- **StableCore** -- high fan-in, low change, protect it
- **UnnecessaryAbstraction** -- structural dep exists but files never co-change
- **SystemicComplexity** -- median/P75 complexity elevated across codebase (catches Odoo-like distributed complexity that GodModule misses)

## Conventions

- **Workspace**: shared deps in root `Cargo.toml` via `[workspace.dependencies]`
- **Testing**: unit tests inside each module, integration tests in `ising-db` and `ising-analysis`
- **Error handling**: `thiserror` for library crates, `anyhow` for CLI
- **DB**: SQLite via `rusqlite`. Schema in `ising-db/src/schema.rs`. FK enforcement via `PRAGMA foreign_keys`.
- **Graph**: `UnifiedGraph` is the in-memory model. Stored in SQLite for persistence. Both representations must stay in sync.

## Adding a New Language Parser

1. Add grammar dependency to `ising-builders/Cargo.toml`
2. Create `ising-builders/src/languages/<lang>.rs` with `extract_nodes()` function
3. Register it in `ising-builders/src/languages/mod.rs`
4. Add `Language` variant and extension mapping in `ising-builders/src/common.rs`
5. Add tree-sitter dispatch in `ising-builders/src/structural.rs` (both `get_tree_sitter_language` and extract match)

**Supported languages**: Python, TypeScript, JavaScript, Rust, Go, Vue, Java, C#, PHP, Ruby, Kotlin, C, C++

## Post-Fix Verification SOP

After fixing bugs in parsers, the risk model, health index, or signal detection:

1. **Run the full CI check locally** before committing:
   ```bash
   cargo test --workspace         # All tests must pass
   cargo clippy --workspace -- -D warnings  # No warnings
   cargo fmt --check              # Clean formatting
   ```

2. **Run the full OSS benchmark** to check for regressions and verify the fix's impact:
   ```bash
   ./scripts/bench-oss-repos.sh --repos-dir /tmp/oss-repos --output /tmp/oss-bench-results
   ```
   **MANDATORY: Always run against ALL 28 repos. Never run a subset.**
   The script auto-clones missing repos. All 28 must pass. There is no "minimum set" fallback —
   partial benchmarks miss cross-repo regressions (e.g., a fix that helps flask but breaks gin).
   - Compare results against `specs/042-oss-validation-round5-expanded/README.md`.
   - If a fix targets a specific repo (e.g., flask small-repo bias), verify that repo's grade changed as expected without regressing others.

3. **Document what changed** in the spec or commit message:
   - Which repos were affected and how grades shifted.
   - Any new caveats or known limitations introduced.

4. **This step is mandatory, not optional.** Unit tests alone cannot catch scoring regressions,
   grade inflation/deflation, or parser failures on real-world code. Always run at least the
   minimum validation set before pushing fixes to analysis code.
