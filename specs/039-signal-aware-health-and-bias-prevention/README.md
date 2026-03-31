---
status: completed
created: 2026-03-30
priority: high
tags:
- health-index
- bias-prevention
- signal-density
- empirical-validation
depends_on:
- '038'
created_at: 2026-03-30T20:00:00Z
updated_at: 2026-03-30T22:00:00Z
---

# Signal-Aware Health Index & Bias Prevention

> **Status**: completed · **Priority**: high · **Created**: 2026-03-30

## Problem

The auto-calibrated risk model (spec 038) solved over-classification, but the health index only measured change-risk distribution and ignored architectural signals. All repos got grade A:

| Repo | Signals | God Modules | Cycles | Old Grade |
|---|---|---|---|---|
| fastapi | 26 | 1 | 0 | A (100%) |
| grafana | 3,666 | 131 | 14 | A (96%) |
| home-assistant | 2,215 | 90 | 28 | A (97%) |

## Solution: Composite Health with Three Sub-Scores

### Sub-score 1: Risk (weight 0.40)

```
median_direct = median of direct_score across active modules
base_health = 1.0 / (1.0 + median_direct * 5.0)
risk_sub = base_health * (0.8 + 0.2 * risk_concentration)
```

**Uses median, not mean.** The mean is dominated by outliers. In gin (98 modules), one hot file (`context.go`) inflated the mean from ~0.05 to ~0.52. With mean, gin scored 0.26 (grade D). With median, gin scores 0.64 (grade B). Gin is a well-maintained framework — D was wrong.

### Sub-score 2: Signals (weight 0.35)

```
sqrt_n = sqrt(total_modules)
weighted = (god_count / sqrt_n) * 3.0
         + (cycle_count / sqrt_n) * 4.0
         + (bomb_count / sqrt_n) * 3.0
         + (fragile_count / sqrt_n) * 2.0
         + (shotgun_count / sqrt_n) * 1.5
         + (unstable_count / sqrt_n) * 2.0
         + (ghost_count / sqrt_n) * 1.0
         + systemic_complexity_count * 2.5

signal_sub = 1.0 / (1.0 + weighted * 0.3)
```

**Uses sqrt(N) normalization, not density (count/N).** This is the same principle as TF-IDF document frequency normalization. Pure density lets large repos hide: 131 god modules / 15,000 = 0.87% looks fine. With sqrt: 131/122 = 1.07, which correctly reflects that 131 god modules is a lot.

Note: `systemic_complexity_count` is NOT divided by sqrt(N) because it's a codebase-level signal (at most 1 per repo), not a per-module signal.

| Repo | god_count/N | god_count/sqrt(N) | Better? |
|---|---|---|---|
| gin (98 mod) | 2.0% | 0.20 | Yes — less punished |
| grafana (15K mod) | 0.87% | 1.07 | Yes — can't hide |

### Sub-score 3: Structure (weight 0.25)

```
entanglement = (cycle_count + unstable_dep_count) / sqrt(N)
structural_sub = 1.0 / (1.0 + entanglement * 0.5)
```

### Signal type weights

| Signal | Weight | Why |
|---|---|---|
| Cycles | 4.0 | Hardest to fix; cascading rebuild/test impacts |
| God modules | 3.0 | Strongest defect density correlate |
| Ticking bombs | 3.0 | Triple threat: hotspot + defect + coupling |
| Systemic complexity | 2.5 | Codebase-level distributed complexity (flat, not sqrt-normalized) |
| Fragile boundary | 2.0 | Active breakage pattern |
| Unstable deps | 2.0 | Stability principle violation |
| Shotgun surgery | 1.5 | Change amplification |
| Ghost coupling | 1.0 | Often benign (shared config) |

**These weights are NOT research-derived.** They reflect engineering judgment. We have no empirical validation that cycles deserve 4x and ghost coupling deserves 1x. The ordering is defensible but the magnitudes are arbitrary.

## Empirical validation (12 repos)

We ran the formula against 12 real repos and checked whether the grades are defensible:

| Repo | Modules | Grade | Risk | Signal | Struct | Verdict |
|---|---|---|---|---|---|---|
| fastapi | 1,513 | A (93%) | 0.92 | 0.92 | 0.96 | Correct |
| gin | 98 | B (80%) | 0.64 | 0.85 | 1.00 | Correct (was D before median fix) |
| django-rest-framework | 175 | A (95%) | 0.87 | 1.00 | 1.00 | Correct |
| langchain | 2,548 | A (96%) | 0.91 | 0.98 | 1.00 | **Questionable** — see below |
| llama.cpp | 313 | A (91%) | 0.90 | 0.88 | 0.97 | Plausible |
| open-webui | 317 | B (73%) | 0.84 | 0.41 | 1.00 | Plausible |
| ha-core | 16,679 | B (75%) | 0.84 | 0.52 | 0.90 | Plausible |
| ollama | 956 | C (65%) | 0.75 | 0.32 | 0.94 | **Questionable** — see below |
| vllm | 2,893 | C (67%) | 0.76 | 0.42 | 0.86 | Plausible |
| transformers | 4,303 | C (62%) | 0.75 | 0.32 | 0.85 | Plausible |
| grafana | 14,966 | C (60%) | 0.83 | 0.35 | 0.59 | Plausible |
| **odoo** | **14,178** | **A (94%)** | **0.93** | **0.92** | **0.97** | **WRONG** |

### Known inaccuracies

**Odoo gets A (94%) despite being notoriously complex.** With 14,178 modules, only 4 trigger god module detection (complexity≥50, LOC≥500, CBO≥15). Odoo's complexity is systemic — distributed across thousands of moderately-complex files rather than concentrated in a few giant ones. **Partially addressed**: the new `SystemicComplexity` signal detects elevated median/P75 complexity, which should fire for Odoo's pattern. Needs re-validation to confirm grade impact.

**LangChain gets A (96%) with only 3 signals.** Either LangChain is genuinely well-structured (its heavily modular design is intentional) or our signals miss its failure mode (rapid API churn, unstable interfaces across versions). We don't measure API stability or breaking change frequency — that's a gap.

**Ollama gets C (65%) which may be too harsh.** Its signal_density of 0.424 is driven by Go-specific false positives (GAP-13: package-level imports inflate ghost coupling and unnecessary abstraction counts). The formula correctly reflects what the detector reports, but the detector has known Go-specific biases.

## Bias analysis

### Biases we fixed

| Bias | Problem | Fix |
|---|---|---|
| Outlier sensitivity | Mean dominated by one hot file in small repos | **Median** direct score |
| Large-repo hiding | count/N dilutes signals in large repos | **sqrt(N)** normalization |
| Opacity | Single number hides what's measured | **Three sub-scores** shown separately |
| Data quality | No warning when data is insufficient | **Caveats** system |

### Biases we did NOT fix

| Bias | Problem | Status |
|---|---|---|
| **God module thresholds** | Hard-coded complexity≥50 misses Odoo's distributed complexity | **Partially addressed**: `SystemicComplexity` signal detects elevated median/P75 complexity. Validate against Odoo to confirm grade impact. |
| **Go signal inflation** | Go packages produce more ghost coupling/unnecessary abstraction signals | **Addressed**: Go intra-package pairs suppressed in both ghost coupling and unnecessary abstraction detection (GAP-13). |
| **Missing signal types** | API stability, breaking changes, test coverage not measured | Would need new data sources |
| **Signal weight magnitudes** | 4x for cycles vs 1x for ghost coupling is engineering judgment | Would need defect correlation study to validate |
| **Time window sensitivity** | Different `--since` windows produce different change_load distributions | Inherent to the approach; documented but not solved |

### What "grade A" actually means

Grade A means: "Among modules with change activity, the median risk is low, and the architectural signals our detector can find are sparse relative to codebase size."

Grade A does NOT mean: "This codebase is well-maintained" or "This codebase has low defect rates." Those claims require data we don't have (defect histories, maintenance cost records, developer surveys).

## Changes Made

### `ising-core/src/fea.rs`
- Extended `HealthIndex` with signal density fields, sub-scores, and caveats vector

### `ising-analysis/src/signals.rs`
- Added `SignalSummary` struct and `summarize_signals()` function
- Added `SystemicComplexity` signal type and `detect_systemic_complexity()` detection
- Added `systemic_complexity_count` to `SignalSummary`
- Added `is_go_intra_package_pair()` for Go ghost coupling and unnecessary abstraction suppression (GAP-13)
- Fixed `is_source_file()` to include all supported extensions (`.vue`, `.php`, `.cc`, `.cxx`, `.hpp`, `.hh`, `.hxx`, `.kts`, `.csx`)
- Expanded `is_generated_code()` with Django migrations, Rails schema, Alembic, vendor/third_party patterns

### `ising-analysis/src/stress.rs`
- Changed `compute_risk_field()` to accept `Option<&SignalSummary>`
- Rewrote `compute_health_index()`: median-based risk, sqrt-normalized signals, three sub-scores
- Added `systemic_complexity_count * 2.5` to signal sub-score (flat weight, not sqrt-normalized)
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

### `ising-core/src/path_utils.rs`
- Expanded `is_test_file()` with Java, Kotlin, C#, PHP, Ruby test conventions
- Added directory patterns: `spec/`, `src/test/`, `__tests__/`

## Future work

1. ~~**New signal: systemic complexity**~~ — **DONE.** `SystemicComplexity` signal detects elevated median/P75 complexity (≥15 median or ≥30 P75 across 50+ modules). Integrated into health index at 2.5x flat weight. Validate against Odoo to confirm grade impact.
2. **Defect correlation study** — validate signal weights against actual defect rates across repos. Currently the weights are assumptions.
3. ~~**Go-specific signal calibration**~~ — **Done.** Go intra-package pairs suppressed in both unnecessary abstraction and ghost coupling detection (GAP-13).
4. **API stability signal** — measure breaking changes, deprecation frequency, interface churn. Would catch LangChain's failure mode if it exists.
