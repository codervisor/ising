---
status: active
created: 2026-03-31
updated: 2026-04-02
priority: medium
last_benchmark: 2026-04-02-round6
tags:
  - validation
  - benchmark
  - oss-repos
  - health-index
  - signals
depends_on:
  - 039-signal-aware-health-and-bias-prevention
  - 037-oss-validation-round4-ai-repos
---

# Spec 042: OSS Validation Round 5 — Expanded Benchmark

**Date**: 2026-03-31 (initial), 2026-04-01 (Round 5b re-run after fixes)
**Repos tested**: 29 (25 succeeded, 4 failed)
**Languages covered**: Python, JS/TS, Go, Java, Rust, C/C++, Ruby, PHP

## Purpose

Aggressive expansion of the validation test set from 12 repos to 28, covering:
- Previously tested repos (for regression comparison)
- New challengers across different languages, sizes, and architectures
- Massive monorepos (e.g., kubernetes and large JS/TS codebases)
- Diverse architectural patterns (DI frameworks, compilers, distributed systems)

## Historical Results (Rounds 5a–5e)

See [results-history.md](results-history.md) for full Round 5a–5e benchmark tables,
comparison matrices, and grade distribution evolution.

**Summary**: Rounds 5a→5e fixed parser crashes (Ruby/PHP/C++ stack overflow), added monolith
god module detection, and introduced P75 risk blend + critical mass penalty. Grade distribution
evolved from 12A/7B/6C/4FAIL (5a) to 11A/7B/9C/1D (5e). rust-lang/rust removed (memory exhaustion).

---

## SOP: Routine Benchmark
A reusable benchmark script has been created at `scripts/bench-oss-repos.sh`.
Missing repos are auto-cloned on first run.

```bash
# Run benchmark (auto-clones any missing repos)
./scripts/bench-oss-repos.sh

# Custom repos/output directory
./scripts/bench-oss-repos.sh --repos-dir /path/to/repos --output /path/to/results
```

Run this script:
- After any change to `ising-analysis/src/stress.rs`
- After any change to `ising-analysis/src/signals.rs`
- After any change to health index computation
- After adding or modifying language parsers
- Before any release

See also: **Post-Fix Verification SOP** in `CLAUDE.md` — always run all 28 repos.

## Spec 047 — Scoring Formula Redesign (2026-04-03)

See [../047-scoring-formula-redesign/RESULTS.md](../047-scoring-formula-redesign/RESULTS.md) for full
results. Key changes: fully multiplicative formula, defect churn separation, widened modifier
ranges, adaptive signal curve, Basel II/Moody's-inspired tail risk cap. Grade distribution:
A=11, B=14, C=3, D=0 (was A=14, B=7, C=4, D=3 in Round 6). All calibration targets pass.

## Round 6 — Scoring Redesign: Zone Fractions + λ_max (2026-04-02)

### What Changed

The entire health index scoring was redesigned. The old model (three arbitrary sub-scores with
40/35/25% weights) was replaced with an FEA-aligned model:

**Old**: `score = risk_sub × 0.40 + signal_sub × 0.35 + structural_sub × 0.25`
**New**: `score = zone_sub_score × coupling_modifier − signal_penalty`

| Component | Description | Range |
|-----------|-------------|-------|
| Zone sub-score | Weighted fraction of active modules per safety zone: Stable×1.0 + Healthy×0.90 + Warning×0.65 + Danger×0.35 + Critical×0.15. Small-sample blend toward 0.75 prior for <50 active modules. | [0, 1] |
| Coupling modifier | Structural λ_max (spectral radius of Import+Calls graph, unit weights), normalized by √N. norm<1: +3% bonus, norm>1: gentle log penalty, capped at ±5%. | [0.95, 1.03] |
| Signal penalty | Same weighted signal formula as before, but as a subtracted penalty (sigmoid saturation) instead of a multiplicative sub-score. Max penalty 0.25. | [0, 0.25] |

Key implementation changes:
- `compute_spectral_metrics()` now uses both Import AND Calls edges (was Import-only, giving λ=0 for TS/JS repos)
- Power iteration extracted into shared `power_iteration()` function
- `compute_health_index()` now takes `&UnifiedGraph` parameter to compute λ_max
- Zone fractions (frac_stable, frac_healthy, etc.) added to `HealthIndex` struct and DB schema

### Results Table (Round 6 — 2026-04-02)

```
Repo                      Gr Score   Zone    CM     SP  %Crit %Dang %Warn %Heal %Stab       λ  λ/√N    Act/  Tot
==================================================================================================================================
svelte                     A  1.00  0.992 1.021  0.010    0.5   0.4   0.3   0.4  98.5    17.5  0.30   3374/ 3374
rails                      A  1.00  0.988 1.016  0.006    0.7   0.4   0.7   0.8  97.4    28.3  0.48    850/ 3476
TypeScript                 A  1.00  1.000 1.005  0.008    0.0   0.0   0.0   0.0 100.0   165.7  0.83  26726/39421
fastify                    A  0.99  0.977 1.015  0.000    1.1   1.1   0.0   5.7  92.0     8.6  0.51     88/  287
fastapi                    A  0.98  0.997 1.007  0.023    0.3   0.1   0.0   0.4  99.2    29.3  0.75   1311/ 1515
nest                       A  0.98  0.958 1.024  0.000    2.2   2.2   2.2   2.2  91.3     8.7  0.21    184/ 1679
langchain                  A  0.98  0.985 1.003  0.010    0.9   0.5   0.6   1.6  96.4    45.0  0.89   2351/ 2548
spring-boot                A  0.96  0.998 1.010  0.051    0.0   0.1   0.1   0.6  99.2    64.3  0.67   9108/ 9108
deno                       A  0.95  1.000 1.011  0.057    0.0   0.0   0.0   0.0 100.0    45.7  0.65   4928/ 5001
django-rest-framework      A  0.94  0.944 0.993  0.000    0.0   6.9   1.7   5.2  86.2    16.7  1.26     58/  175
php-src                    A  0.93  0.999 1.022  0.096    0.0   0.1   0.0   0.0  99.9    12.7  0.26   2244/ 2393
odoo                       A  0.92  1.000 0.950  0.030    0.0   0.0   0.0   0.0 100.0  2676.8 22.47  14157/14189
next.js                    A  0.91  1.000 0.950  0.041    0.0   0.0   0.0   0.0  99.9   887.0  5.96  21975/22132
llama.cpp                  A  0.86  0.998 0.950  0.084    0.1   0.0   0.1   0.4  99.4   382.0 11.44   1113/ 1114
pytorch                    B  0.81  0.960 1.002  0.151    0.7   2.5   4.0   3.3  89.5    87.6  0.92   8928/ 9085
express                    B  0.80  0.779 1.027  0.000   11.8   5.9   5.9   5.9  70.6     1.0  0.08     17/  142
ollama                     B  0.80  0.998 0.964  0.166    0.1   0.2   0.2   0.0  99.6   124.3  3.44   1299/ 1303
kafka                      B  0.78  0.982 0.986  0.189    0.1   0.5   1.0  10.5  87.8   128.9  1.65   6132/ 6141
kubernetes                 B  0.77  0.998 0.950  0.180    0.1   0.1   0.1   0.8  98.9   759.0  5.80   5446/17116
open-webui                 B  0.77  0.923 0.997  0.153    8.7   0.4   0.0   1.4  89.5    19.7  1.10    277/  317
django                     B  0.76  0.853 1.005  0.102    4.0   6.9  12.1  26.2  50.8    45.4  0.83    480/ 3006
gin                        C  0.66  0.734 0.950  0.042   22.5   7.5   5.0  12.5  52.5   100.2 10.12     40/   98
flask                      C  0.65  0.703 0.972  0.032   43.8   0.0   0.0  25.0  31.2    23.9  2.63     16/   83
prometheus                 C  0.65  0.855 0.950  0.167    8.4  10.2   0.9   3.2  77.2   227.5  7.36    771/  955
grafana                    C  0.62  0.818 0.969  0.168   17.9   4.4   0.3   0.3  77.1   361.1  2.95  14990/14997
ha-core                    D  0.49  0.616 0.992  0.123   39.1   7.2   1.2   0.5  52.0   167.7  1.30  16701/16703
vllm                       D  0.47  0.629 1.002  0.155   27.6  15.9   7.2   7.6  41.6    51.7  0.94   2818/ 3031
transformers               D  0.46  0.639 0.993  0.176   34.5   7.0   4.7   5.1  48.6    85.2  1.30   3844/ 4323
```

### Grade Distribution (Round 6)

| Grade | Count | Repos |
|-------|-------|-------|
| A | 14 | svelte, rails, TypeScript, fastify, fastapi, nest, langchain, spring-boot, deno, DRF, php-src, odoo, next.js, llama.cpp |
| B | 7 | pytorch, express, ollama, kafka, kubernetes, open-webui, django |
| C | 4 | gin, flask, prometheus, grafana |
| D | 3 | ha-core, vllm, transformers |

### Round 5e → Round 6 Comparison

| Repo | 5e | R6 | Delta | Why |
|------|:--:|:--:|:-----:|-----|
| flask | C (0.63) | C (0.65) | +0.02 | Zone 0.703, small-sample blend pulls toward 0.75 |
| gin | **B (0.75)** | **C (0.66)** | -0.09 | **REGRESSION** — 23% critical zone, zone score 0.73 drives it down |
| express | B (0.77) | B (0.80) | +0.03 | Zone 0.78, small-sample blend bonus |
| ollama | C (0.69) | **B (0.80)** | +0.11 | Zone 0.998 (99.6% stable), signal penalty 0.17 is the only drag |
| prometheus | C (0.66) | C (0.65) | -0.01 | Zone 0.86 good, but signal penalty 0.17 pulls it to C |
| kubernetes | C (0.60) | **B (0.77)** | +0.17 | Zone 0.998 (99% stable), signal penalty 0.18 |
| kafka | **D (0.54)** | **B (0.78)** | +0.24 | Zone 0.98 (88% stable), signal penalty 0.19 |
| spring-boot | B (0.81) | **A (0.96)** | +0.15 | Zone 0.998, low signal penalty 0.05 |
| llama.cpp | B (0.80) | **A (0.86)** | +0.06 | Zone 0.998, signal penalty 0.08 |
| ha-core | C (0.70) | **D (0.49)** | -0.21 | 39% critical zone — honestly bad |
| transformers | C (0.57) | **D (0.46)** | -0.11 | 35% critical zone, high signal penalty |
| vllm | C (0.61) | **D (0.48)** | -0.13 | 28% critical + 16% danger zones |

### Critical Analysis

#### 1. The model is now more transparent but less discriminating in the middle

The zone-based scoring makes it easy to explain *why* a repo gets its grade — you just look
at the zone distribution. But the practical effect is that repos with good zone distributions
(>90% stable) are now separated primarily by signal penalty, not by zone fractions.

**The A-grade cluster**: 14 repos all have zone_sub_score > 0.94. Their grades are differentiated
almost entirely by signal penalty: deno (0.057) vs php-src (0.096) vs odoo (0.030). This is the
same "signals gate the grade" problem from Round 5 — it just moved from a multiplicative sub-score
to an additive penalty.

**The B-grade cluster**: Here the model works well. Django (zone=0.85), express (zone=0.78), and
pytorch (zone=0.96 but SP=0.15) show genuine differentiation. Repos get B either because they
have moderate zone scores or because signal penalties pull good zone scores down.

#### 2. Massive repos still get inflated grades (worse than before)

| Repo | Modules | Critical Tier | %Critical Zone | Grade |
|------|---------|---------------|----------------|-------|
| TypeScript | 39,421 | 268 | 0.0% | A (1.00) |
| next.js | 22,132 | 220 | 0.0% | A (0.91) |
| odoo | 14,189 | 142 | 0.0% | A (0.92) |
| kubernetes | 17,116 | 55 | 0.1% | B (0.77) |

TypeScript has 268 modules classified as critical risk tier, yet its %Critical *zone* is 0.0%.
This is because zone classification uses safety_factor (capacity/risk_score), and in massive
repos the vast majority of modules have zero change_load → SF=10.0 (max) → Stable zone. The
percentile-based tier system assigns 1% as Critical regardless, but zone fractions only count
modules with SF<1.0. These are different questions:

- **Risk tier**: "Is this module in the top 1% of risk?" (relative, always flags something)
- **Safety zone**: "Is this module under more stress than it can handle?" (absolute, can be zero)

The zone-based model is correct in one sense: if 99.9% of modules have SF>>1, the codebase
genuinely has very little active stress. But it's blind to the absolute scale problem: 268
"stressed" modules is a lot of tech debt regardless of what percentage they represent.

**This is the density problem documented in CLAUDE.md: `count/N lets large repos hide.`** The
zone fractions ARE count/N. We moved to zones to be more FEA-aligned, but we reintroduced
the exact bias we were trying to eliminate.

#### 3. Small repos get honest but harsh treatment

Flask (16 active, 44% critical) and gin (40 active, 23% critical) are genuinely under change
pressure. The model is telling the truth: their core modules have safety_factor < 1.0 because
they're being actively developed. The small-sample blend helps (pulls flask from ~0.60 to 0.70)
but doesn't fully compensate.

The question is philosophical: **is "under active development" the same as "at risk"?** The FEA
model says yes — a bridge girder under stress IS at risk, regardless of whether the stress comes
from normal load or exceptional load. But software is different: a frequently-changed module in
a well-maintained framework is *normal work*, not a risk event.

This reveals a fundamental limitation of the safety factor model: it conflates **change activity**
with **risk**. A module that gets changed 10 times because it's being actively improved has the
same change_load as one changed 10 times because it keeps breaking.

#### 4. The coupling modifier is nearly irrelevant

With the gentle ±3% range (clamped [0.95, 1.03]), the coupling modifier barely affects scores:

| Repo | CM | Effect on Score |
|------|-----|-----------------|
| odoo (λ/√N=22.5) | 0.950 | -5% of zone score |
| gin (λ/√N=10.1) | 0.950 | -5% of zone score |
| TypeScript (λ/√N=0.83) | 1.005 | +0.5% of zone score |
| express (λ/√N=0.08) | 1.027 | +2.7% of zone score |

The modifier hits its floor (0.950) for many repos. With a max effect of ±5%, λ_max contributes
at most 0.05 to the final score. Signal penalty has 5× more impact (up to 0.25). The structural
coupling measurement we spent effort on is being effectively muted.

**Options**:
- (a) Accept this — zone fractions + signals are sufficient; λ_max is stored for future use
- (b) Widen the range — but we already saw that wider ranges crush small/coupled repos unfairly
- (c) Use λ_max differently — not as a modifier, but as a separate axis (e.g., coupling grade)

#### 5. Signal penalty still dominates grade boundaries

Repos where signal penalty is the deciding factor between grade tiers:

| Repo | Zone Score | Signal Penalty | Final Score | Without SP |
|------|-----------|----------------|-------------|------------|
| prometheus | 0.855 | 0.167 | 0.65 (C) | 0.81 (B) |
| grafana | 0.818 | 0.168 | 0.62 (C) | 0.79 (B) |
| ollama | 0.998 | 0.166 | 0.80 (B) | 0.96 (A) |
| kubernetes | 0.998 | 0.180 | 0.77 (B) | 0.95 (A) |
| kafka | 0.982 | 0.189 | 0.78 (B) | 0.97 (A) |

Prometheus has zone 0.855 (should be B) but signal penalty drags it to C. Kafka has zone 0.98
(should be A) but signal penalty drags it to B. The signal penalty is the main discriminator
for repos with good zone distributions.

This is defensible IF the signals are reliable. But signal weights (cycles=4x, god_modules=3x,
ghost_coupling=1x) are engineering judgment, not empirically validated. A single miscalibrated
signal weight could shift multiple repos by a full grade letter.

#### 6. Three repos moved to D — are they justified?

| Repo | Score | %Crit | %Stable | Signal Penalty | Justified? |
|------|-------|-------|---------|----------------|------------|
| ha-core | 0.49 | 39.1% | 52.0% | 0.123 | **Probably yes** — 39% critical in a 16K module repo is genuine systemic risk. 668 high-tier modules. |
| transformers | 0.46 | 34.5% | 48.6% | 0.176 | **Questionable** — actively developed ML library, high churn is normal. But 35% critical is still a lot. |
| vllm | 0.47 | 27.6% | 41.6% | 0.155 | **Questionable** — fast-moving ML project, 16% danger + 28% critical = 44% of modules under stress. Signal penalty adds insult to injury. |

For transformers and vllm, the high %Critical likely reflects rapid development pace, not poor
architecture. These repos are among the fastest-moving in the Python ecosystem. The model
penalizes velocity the same as instability.

### Unresolved Issues

| # | Issue | Severity | Status |
|---|-------|----------|--------|
| 1 | Zone fractions are count/N — large repo grade inflation persists | HIGH | OPEN — same density bias as before, just with different metrics |
| 2 | gin regression B→C — calibration target violated | MEDIUM | OPEN — 23% critical zone is genuine, but target may need updating |
| 3 | Change activity conflated with risk in safety factor model | HIGH | KNOWN — would require distinguishing "normal churn" from "defect churn" (needs defect data we don't have) |
| 4 | λ_max coupling modifier too gentle to matter | LOW | ACCEPTED — zone fractions + signals are primary; λ_max stored for future temporal analysis |
| 5 | Signal penalty dominates B/C boundary without empirical validation | MEDIUM | OPEN — signal weights are engineering judgment, not validated against defect rates |
| 6 | Go repos cluster at C due to high signal counts | MEDIUM | PARTIALLY ADDRESSED — Go intra-package suppression helped but Go package imports still generate more signals |

### Recommendations

| # | Recommendation | Priority |
|---|---------------|----------|
| 1 | Investigate absolute critical count penalty for zone scoring (not just zone fractions) — repos with >100 critical-zone modules should be penalized regardless of percentage | HIGH |
| 2 | Update gin calibration target from ≥B to ≥C — the 23% critical zone is real data, not a model artifact | LOW |
| 3 | Consider distinguishing "active churn" from "defect churn" — modules with only additive changes (new features) vs modules with corrective changes (bug fixes) have different risk profiles | MEDIUM (requires git message analysis, future phase) |
| 4 | Validate signal weights against real defect data — until this is done, treat all grades as "structural health estimate with unvalidated signal component" | HIGH (research project) |
| 5 | Consider reporting λ_max as a separate coupling metric rather than folding it into the grade | LOW |
