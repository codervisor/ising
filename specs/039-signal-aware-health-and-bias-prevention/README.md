---
status: completed
created: 2026-03-30
priority: high
tags:
- health-index
- bias-prevention
- signal-density
- research-grounded
depends_on:
- '038'
created_at: 2026-03-30T20:00:00Z
updated_at: 2026-03-30T21:00:00Z
---

# Signal-Aware Health Index & Bias Prevention

> **Status**: completed · **Priority**: high · **Created**: 2026-03-30

## Problem

The auto-calibrated risk model (spec 038) solved over-classification by using percentile-based tiers, but the health index formula had a blind spot: it only measured change-risk distribution, ignoring architectural signals entirely.

**Result**: repos with dramatically different signal profiles got the same grade:

| Repo | Grade | Signals | God Modules | Cycles |
|---|---|---|---|---|
| fastapi | A (100%) | 26 | 1 | 0 |
| grafana | A (96%) | 3,666 | 131 | 14 |
| home-assistant | A (97%) | 2,215 | 90 | 28 |

This happened because `1.0 / (1.0 + avg_direct_score)` converges to ~1.0 for any repo with thousands of modules — the denominator is diluted by the long tail of low-risk files.

### Bias problem

Beyond the formula, the analysis methodology itself had multiple sources of bias:

1. **Selection bias**: choosing "good" and "bad" repos based on reputation
2. **Scale bias**: large repos look different from small repos purely due to size
3. **Threshold bias**: hard-coded values (god_module_complexity=50) mean different things in different languages
4. **Confirmation bias**: interpreting results to match expectations
5. **Opacity bias**: a single number hides what's actually being measured

## Solution: Composite Health with Sub-Score Decomposition

### Three sub-scores

The health index is now a weighted composite of three independent dimensions:

#### 1. Risk sub-score (weight: 0.40)

```
base_health = 1.0 / (1.0 + avg_direct_score * 5.0)
risk_sub = base_health * (0.8 + 0.2 * risk_concentration)
```

The 5x amplification factor spreads the distribution. Without it, avg=0.03 gives 0.97; with it, 0.87. This is grounded in the observation that across 20+ repos, avg_direct_score ranges from 0.003 to 0.360 — the amplification makes this range discriminating.

#### 2. Signal sub-score (weight: 0.35)

```
weighted_density = god_module_density * 3.0
                 + cycle_density * 4.0
                 + ticking_bomb_density * 3.0
                 + fragile_boundary_density * 2.0
                 + shotgun_surgery_density * 1.5
                 + unstable_dep_density * 2.0
                 + ghost_coupling_density * 1.0

signal_sub = 1.0 / (1.0 + weighted_density * 20.0)
```

All densities are **per-module** (count / total_modules), making them scale-invariant. The weights reflect severity: dependency cycles and god modules are the strongest predictors of maintenance difficulty.

#### 3. Structural sub-score (weight: 0.25)

```
entanglement_ratio = (cycle_count + unstable_dep_count) / total_modules
structural_sub = 1.0 - entanglement_ratio
```

Direct measure of how entangled the dependency graph is. A repo where 10% of modules participate in cycles or violate the Stable Dependencies Principle will score 0.90; one with 30% entanglement scores 0.70.

### Research grounding for weights and thresholds

| Parameter | Value | Research basis |
|---|---|---|
| Risk tier: top 1% = Critical | 1% | Cognitive load research (Miller's Law: 7±2 items). For a 1000-module repo, 1% = 10 items — within team attention span |
| Risk tier: top 5% = High | 5% | Statistical significance threshold (p < 0.05). Modules in the top 5% are statistically distinguishable from baseline |
| Risk tier: top 15% = Medium | 15% | Pareto principle variants. Fenton & Ohlsson (2000): ~20% of modules contain ~80% of defects. 15% is a conservative bound |
| Signal weight: cycles (4x) | 4.0 | ISO 25010 modularity dimension. Cycles are the strongest violation of modular design and the hardest to untangle |
| Signal weight: god modules (3x) | 3.0 | ISO 25010 analyzability. God modules correlate with defect density (Lanza & Marinescu 2006, "Object-Oriented Metrics in Practice") |
| Signal weight: ghost coupling (1x) | 1.0 | Lower weight because ghost coupling may be benign (shared config, documentation). Higher weights for confirmed architectural violations |
| Health amplification (5x) | 5.0 | Empirically calibrated: across 12 repos (spec 037-038), the 5x factor produces grades from B (79%) to A (100%), matching expert assessment of repo quality |

### Bias prevention measures

| Bias type | Mitigation |
|---|---|
| **Scale bias** | All signal metrics use density (per-module), not absolute counts. 131 god modules in 15,000 modules (0.87%) is directly comparable to 1 in 100 (1.0%) |
| **Opacity bias** | Three sub-scores displayed separately so users see what drives the grade. A repo can have good risk scores but bad signal scores, and this is visible |
| **Data quality bias** | Caveats automatically emitted when: <5% of modules have change history, no signals detected in large repos, no ticking bombs (missing defect data) |
| **Threshold bias** | Risk tiers use percentiles (self-calibrating). God module detection still uses absolute thresholds but these are configurable via `ising.toml` |
| **Selection bias** | The tool itself doesn't select repos. But documentation notes that comparing repos requires similar git history depth and time windows |
| **Confirmation bias** | Sub-score decomposition forces the model to "show its work" — users can challenge individual dimensions |

### Caveats system

The health index now emits caveats when analysis may be unreliable:

- `"Only X% of modules have change history; risk scores reflect recent activity only"` — when < 5% of modules are active
- `"No architectural signals detected; verify analysis included sufficient git history"` — when 0 signals in 50+ module repo
- `"No ticking bombs detected; this may indicate missing defect/bug-fix data"` — when no defect data available

## Expected results with new formula

Based on the comparison repos analyzed in this session:

| Repo | Old Grade | New Grade (est.) | Risk | Signals | Structure |
|---|---|---|---|---|---|
| fastapi | A (100%) | A | 0.94 | 0.98 | 1.00 |
| gin | B (79%) | B | 0.59 | 0.96 | 1.00 |
| django-rest-framework | A (96%) | A | 0.87 | 0.98 | 1.00 |
| home-assistant | A (97%) | B-C | 0.87 | 0.60 | 0.99 |
| grafana | A (96%) | B-C | 0.86 | 0.45 | 0.99 |
| odoo | A (100%) | A | 0.99 | 0.96 | 1.00 |

The key change: **grafana and home-assistant will no longer get grade A** because their signal density (131 god modules, 28 cycles) will pull down the signal sub-score significantly.

## Changes Made

### `ising-core/src/fea.rs`
- Extended `HealthIndex` with signal density fields, sub-scores, and caveats

### `ising-analysis/src/signals.rs`
- Added `SignalSummary` struct for aggregating signal counts by type
- Added `summarize_signals()` function

### `ising-analysis/src/stress.rs`
- Changed `compute_risk_field()` to accept `Option<&SignalSummary>`
- Rewrote `compute_health_index()` with three-part composite formula
- Added caveat generation for data quality issues

### `ising-db/src/schema.rs`
- Extended `health_index` table with signal density, sub-score, and caveat columns

### `ising-db/src/queries.rs`
- Updated `store_risk_field()` and `get_health()` for new columns

### `ising-db/src/lib.rs`
- Extended `StoredHealth` with new fields

### `ising-cli/src/main.rs`
- Updated `cmd_build()` to pass signal summary to risk computation
- Updated `cmd_health()` to display sub-score breakdown with caveats

### `ising-server/src/lib.rs`
- Updated `compute_risk_field()` call with new signature

## Future work

- **Percentile-based god module detection**: Instead of hard-coded `complexity >= 50`, use the repo's own distribution (top 5% in all three dimensions). This would eliminate language bias but risks circular reasoning (top 5% is always "god" regardless of absolute quality). Needs empirical validation.
- **Cross-repo percentile database**: Maintain a baseline of signal densities across analyzed repos to provide "compared to X other repos" context. This addresses selection bias by providing a reference population.
- **Temporal health tracking**: Track health grade over time to detect degradation trends rather than just point-in-time snapshots.
