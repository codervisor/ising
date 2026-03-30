---
status: completed
created: 2026-03-30
priority: critical
tags:
- risk-model
- auto-calibration
- percentile
- health-index
depends_on:
- '037'
created_at: 2026-03-30T18:00:00Z
updated_at: 2026-03-30T19:00:00Z
---

# Auto-Calibrated Risk Model

> **Status**: completed · **Priority**: critical · **Created**: 2026-03-30

## Problem

The original risk model used hard-coded safety factor thresholds (SF < 1.0 = Critical, etc.) applied uniformly to all codebases. Propagated risk dominated the classification, causing massive over-classification in dense graphs:

| Repo | Old Critical | Total Modules | Critical Rate |
|---|---|---|---|
| Ollama (Go) | 538 | 956 | **56%** |
| vLLM (Python) | 797 | 2,893 | **28%** |
| Transformers (Python) | 1,322 | 4,303 | **31%** |
| Open WebUI (Python+TS) | 84 | 322 | **26%** |

Analysis showed 95-99% of "critical" modules were propagation-dominated (propagated_risk > 3x change_load), with most barely below the SF < 1.0 threshold. Only 1-11 modules per repo had genuinely high direct risk.

## Solution: Camera Auto-Balance Model

Inspired by how cameras auto-calibrate to scene conditions, the new model separates three concerns:

### 1. Direct Score (the "subject brightness")

```
direct_score = change_load / capacity
```

Purely local measurement. No propagation involved. Invariant to graph density, language, or architecture. A high-churn complex file is dangerous regardless of context.

### 2. Risk Tiers (auto-exposure)

Instead of fixed SF thresholds, classify modules by their percentile rank within the graph's own distribution of direct scores:

| Tier | Percentile | Meaning |
|---|---|---|
| Critical | Top 1% | Immediate attention needed |
| High | Top 1-5% | Elevated risk, monitor closely |
| Medium | Top 5-15% | Moderate risk |
| Normal | Bottom 85% | No action needed |

Only modules with `change_load > 0` (actually changed in the time window) are eligible for Critical/High tiers. This auto-calibrates to each repo's own conditions.

### 3. Health Index (exposure meter)

Single aggregate score for the repository:

```
base_health = 1.0 / (1.0 + avg_direct_score)
concentration = top_10%_risk / total_risk
health = base_health * (0.9 + 0.2 * concentration)
```

Grades: A (>85%), B (>70%), C (>55%), D (>40%), F (<40%).

High concentration + low average = "healthy with known hotspots" (easy to fix).
Low concentration + high average = "systemic debt" (harder).

### What Propagation Still Does

Propagation is kept in the model — it powers the `impact` command for blast radius queries ("if I change X, what else is affected?"). It's removed from the zone/tier classification where it caused over-classification.

## Results

| Repo | Old Critical | New Critical | New High | Health |
|---|---|---|---|---|
| **Ollama** | 538 | **10** | 38 | A (91%) |
| **vLLM** | 797 | **27** | 107 | A (93%) |
| **Transformers** | 1,322 | **39** | 153 | A (93%) |
| **Open WebUI** | 84 | **3** | 12 | A (94%) |
| **LangChain** | 36 | **24** | 94 | A (98%) |
| **llama.cpp** | 5 | **4** | 12 | A (100%) |

### Hotspot ranking stability

Top-1 hotspot matches between old and new model in 5/6 repos. The one divergence (Transformers) is between `modeling_utils.py` and `trainer.py` — both are genuinely the top two risks and differ only in whether propagation or direct change pressure is weighted more.

### What each tier means (actionable)

- **Critical (top 1%)**: Refactor these. Break them apart. Add test coverage. These are the files that will cause the most regressions.
- **High (top 5%)**: Monitor closely. These are on the path to becoming critical. Good candidates for tech debt sprints.
- **Medium (top 15%)**: Normal development attention. No emergency.
- **Normal (85%)**: No action needed.

## Changes Made

### `ising-core/src/fea.rs`
- Added `RiskTier` enum (Critical, High, Medium, Normal)
- Added `HealthIndex` struct
- Added `direct_score`, `risk_tier`, `percentile` fields to `NodeRisk`
- Added `health` field to `RiskField`

### `ising-analysis/src/stress.rs`
- Added `assign_risk_tiers()` — percentile-based auto-calibration
- Added `compute_health_index()` — aggregate repo health score
- Changed sort order: direct_score descending (was: safety_factor ascending)
- Computes `direct_score = change_load / capacity` for every module

### `ising-db/src/schema.rs`
- Added `direct_score`, `risk_tier`, `percentile` columns to `risk_data` table
- Added `health_index` table

### `ising-db/src/queries.rs`
- Updated `store_risk_field()` to persist new fields + health index
- Updated all SELECT queries to include new columns
- Added `get_health()` query
- Changed `get_safety_ranking()` to ORDER BY `direct_score DESC`

### `ising-cli/src/main.rs`
- Added `health` command
- Updated `safety` command to show Direct score, Percentile, and Tier instead of Risk, SF, and Zone
- Updated `build` summary to show tier counts + health grade

### Backward Compatibility
- Legacy `SafetyZone` enum and `safety_factor` field retained
- Legacy `zone` column still stored in DB
- `get_nodes_by_zone()` accepts both zone names and tier names
- JSON output includes both old and new fields via `#[serde(default)]`
