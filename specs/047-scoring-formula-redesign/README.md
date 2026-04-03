---
status: draft
created: 2026-04-03
priority: high
tags:
- scoring
- health-index
- formula
- calibration
- validation
depends_on:
- '046'
- '042'
- '039'
created_at: 2026-04-03T18:00:00Z
updated_at: 2026-04-03T18:00:00Z
---

# Spec 047: Scoring Formula Redesign — Fixing Structural Weaknesses

## Motivation

Spec 046 (boundary-aware analysis) was implemented and validated against 14 OSS repos.
The benchmark revealed that while boundary attenuation produces meaningful improvements
(django B->A, gin C->B), the **scoring formula itself has structural weaknesses** that
incremental fixes cannot resolve.

This spec documents the findings from the spec 046 post-implementation analysis and
proposes concrete next steps to address the formula's limitations.

## Current Formula

```
score = zone_sub_score × coupling_modifier × containment_modifier − signal_penalty
```

Where:
- `zone_sub_score`: weighted average of safety zone fractions, with small-sample blend
- `coupling_modifier`: f(λ_max/√N), clamped [0.95, 1.03]
- `containment_modifier`: 0.85 + 0.15 × avg_containment, range [0.85, 1.00]
- `signal_penalty`: 0.25 × wss / (wss + 3.0), range [0, 0.25]

Grade thresholds: A >= 0.85, B >= 0.70, C >= 0.55, D >= 0.40, F < 0.40.

## Spec 046 Benchmark Results (2026-04-03)

14 of 28 repos analyzed (14 FAILed from pre-existing DB FK constraint bug).

### Results Table

```
Repo                      Gr  Score   Zone    CM     BM    SP   %Crit %Stab   Act/  Tot   #Sigs
================================================================================================
TypeScript                 A   1.00  1.000  1.005  ~1.00  0.007   0.0  99.98  26726/39421   2262
fastify                    A   0.99  0.977  1.015  ~1.00  0.000   1.1  92.0      88/  287    124
nest                       A   0.98  0.958  1.024  ~1.00  0.000   2.2  91.3     184/ 1679    289
django-rest-framework      A   0.96  0.964  0.993  ~1.00  0.000   0.0  91.4      58/  175    104
spring-boot                A   0.96  0.999  1.010  ~1.00  0.051   0.01 99.8    9113/ 9113  15276
php-src                    A   0.93  0.999  1.022  ~1.00  0.094   0.0  99.9    2244/ 2393  22429
odoo                       A   0.92  1.000  0.950  ~1.00  0.030   0.0  99.95  14158/14190  11071
django                     A   0.88  0.981  1.005  0.982  0.102   0.6  94.3    2399/ 3007   1052
express                    B   0.80  0.779  1.027  ~1.00  0.000  11.8  70.6      17/  142     18
kubernetes                 B   0.77  0.998  0.950  0.988  0.177   0.1  98.9    5446/17116  33704
gin                        B   0.75  0.835  0.950  0.978  0.042   7.5  72.5      40/   98    135
flask                      C   0.65  0.703  0.972  0.982  0.032  43.8  31.2      16/   83     56
prometheus                 C   0.64  0.853  0.950  ~0.98  0.167   8.7  76.9     771/  955   1774
grafana                    C   0.59  0.818  0.969  0.972  0.167  17.9  77.1   14996/15006  23230
```

### Grade Changes vs Round 6

| Repo | Round 6 | Spec 046 | Delta | Cause |
|------|---------|----------|-------|-------|
| django | B (0.76) | A (0.88) | +0.12 | Boundary attenuation: 94% stable (was 51%) |
| gin | C (0.66) | B (0.75) | +0.09 | Go sub-package boundaries reduce propagation |
| DRF | A (0.94) | A (0.96) | +0.02 | 2 modules moved from Danger to Stable |
| grafana | C (0.62) | C (0.59) | -0.03 | Noise (underlying metrics unchanged) |
| All others | — | — | ±0.01 | Negligible |

### Calibration Checks

- [PASS] gin >= B: got B (0.75)
- [WARN] odoo != A (known blind spot): got A (0.92) — unchanged
- [WARN] TypeScript sanity: got A (1.00) — unchanged

### Repos Not Tested (FAIL)

14 repos failed due to pre-existing DB FK constraint bug (not spec 046 regression):
fastapi, next.js, svelte, ollama, kafka, deno, pytorch, transformers, vllm,
llama.cpp, langchain, open-webui, ha-core, rails.

Key omissions: **transformers, vllm, ha-core** (D-grade repos that spec 046 was designed
to help) could not be validated.

---

## Identified Problems

### Problem 1: Multiplicative × Subtractive Mixing

The formula multiplies three terms then subtracts a fourth:
```
score = (A × B × C) − D
```

This creates **asymmetric penalty behavior**: the same signal penalty hurts repos with
low zone scores proportionally more than repos with high zone scores.

```
zone=0.98: score = 0.98 − 0.15 = 0.83  (penalty = 15% of pre-penalty)
zone=0.72: score = 0.72 − 0.15 = 0.57  (penalty = 21% of pre-penalty)
```

This is not principled. The signal penalty should either be proportional (multiplicative)
or absolute (additive), not mixed. The current form means signals disproportionately
punish repos that already have zone problems.

**Severity**: Medium. Affects grade boundaries for repos in the B-C range.

### Problem 2: Three Components Have Near-Zero Effective Range

| Component | Theoretical Range | Effective Range | Impact on Score |
|-----------|-------------------|-----------------|-----------------|
| Zone sub-score | [0.15, 1.0] | **[0.15, 1.0]** | **±85%** (dominant) |
| Coupling modifier | [0.95, 1.03] | [0.95, 1.03] | ±4% |
| Containment modifier | [0.85, 1.00] | [0.95, 1.00] | ±3% |
| Signal penalty | [0, 0.25] | [0, 0.22] | ±22% |

The coupling modifier and containment modifier together can swing the score by at most
~8%. This is below the noise floor of zone fractions. They're correct signals measured
at irrelevant magnitudes.

The grade is effectively determined by just two numbers: zone_sub_score and signal_penalty.
The other two components are decorative.

**Severity**: Low (they don't produce wrong results, they just don't contribute).

### Problem 3: Signal Penalty Saturates Too Early

The sigmoid `0.25 × x/(x+3)` saturates quickly:

```
x=3  → penalty=0.125
x=10 → penalty=0.192
x=20 → penalty=0.217
x=50 → penalty=0.236
```

For repos with weighted_signal_score > 5, the penalty is effectively flat (~0.16-0.22).
This means:
- kubernetes (33,704 signals): penalty = 0.177
- prometheus (1,774 signals): penalty = 0.167
- grafana (23,230 signals): penalty = 0.167

A 19× difference in signal count produces a 6% difference in penalty. The sigmoid
treats 1,774 and 33,704 signals as essentially equivalent. This loses discrimination
in the upper range.

**Severity**: Medium. Repos with genuinely different signal profiles get similar penalties.

### Problem 4: Signal Type Weights Are Unjustified

Current weights (engineering judgment):
```
cycles=4×, god_modules=3×, ticking_bombs=3×, fragile_boundaries=2×,
unstable_deps=2×, shotgun_surgery=1.5×, ghost_coupling=1×, systemic=2.5×
```

The ordering (cycles > god modules > ghost coupling) is defensible. The magnitudes
are arbitrary. A 500-signal ghost coupling repo gets penalty=0.21 while a 5-signal
god module repo gets penalty=0.04. Ghost coupling is the lowest-confidence signal
(most prone to false positives), yet volume overwhelms weight.

**Severity**: Medium. Affects repos with asymmetric signal profiles.

### Problem 5: Change Velocity ≠ Risk (Fundamental)

The core risk model:
```
change_load = normalize(change_freq × churn_rate)
direct_score = change_load / capacity
```

All changes are treated equally. But:
- **Feature development** (new code, high churn) is normal work
- **Bug fixes** (targeted changes) indicate actual problems
- **Refactoring** (large churn, many files) is risk reduction

The model cannot distinguish these. This is why transformers and vllm get D grades —
they're under heavy active development, which the model interprets as high risk.

**Severity**: Critical. This is the root cause of the most visible false positives
(active repos rated worse than stagnant ones).

### Problem 6: Capacity Weights Are Arbitrary

```
burden = complexity × 0.4 + instability × 0.3 + coupling × 0.3
capacity = max(1.0 − burden, 0.05)
```

The 0.4/0.3/0.3 weights have never been validated against defect data. The instability
metric `fan_out / (fan_in + fan_out)` is Robert Martin's SDP definition, but it was
designed for package-level analysis, not file-level. A leaf module with zero imports
gets instability=0 regardless of actual volatility.

**Severity**: Medium. Affects per-module risk ranking, which feeds into zone fractions.

### Problem 7: Propagation Topology Dominates Content

Hub modules (many imports) accumulate propagated risk from all directions, regardless
of whether their neighbors are actually risky. A utility module imported by 50 files
gets 50 propagation channels even if all 50 importers have zero change_load.

Boundary attenuation (spec 046) partially addresses this by limiting cross-boundary
propagation, but intra-module hubs still receive inflated propagated risk.

**Severity**: Medium. Partially mitigated by GAP-3 attenuation and boundary attenuation.

### Problem 8: Small-Sample Blend Prior Is Biased

The blend prior is 0.75 (B-grade equivalent):
```
zone_sub_score = raw × (N/50) + 0.75 × (1 − N/50)    for N < 50
```

For bad small repos, this pulls scores UP (flask: 0.60→0.70).
For good small repos, this pulls scores DOWN (hypothetical: 0.98→0.94 at N=16).

The prior is generous — it assumes repos are B-grade by default. A neutral prior
should be the population median or derived from the data, not a fixed constant.

**Severity**: Low. Only affects repos with <50 active modules (flask, express, gin).

---

## Proposed Changes

### Phase 1: Fix the Formula Structure (score calculation)

**Goal**: Eliminate multiplicative × subtractive mixing. Move to either fully
multiplicative or fully additive.

**Proposal**: Fully multiplicative with signal as a discount factor:
```
score = zone_sub_score × coupling_modifier × containment_modifier × signal_factor
```
Where:
```
signal_factor = 1.0 − signal_penalty    (range [0.75, 1.0])
```

This makes the signal penalty proportional: 15% penalty reduces any pre-penalty
score by 15%, regardless of the base value.

**Alternative**: Component decomposition with explicit weights:
```
score = w₁ × zone_component + w₂ × signal_component + w₃ × boundary_component
```
With weights summing to 1.0. More transparent but requires weight calibration.

**Validation**: Re-run benchmark. Verify gin >= B, flask stays C, django stays A.

### Phase 2: Widen Modifier Ranges

**Coupling modifier**: Expand from [0.95, 1.03] to [0.85, 1.05]. This gives
structural coupling a 20% total swing — enough to shift a grade. Requires
validation that gin and other small high-λ repos don't get destroyed.

**Containment modifier**: Expand from [0.85, 1.00] to [0.70, 1.05]. Well-contained
repos get a bonus; leaky repos get a meaningful penalty. Makes boundary health
actually affect grades.

**Severity**: These are parameter changes, not structural changes. Easy to test.

### Phase 3: Separate Defect Churn from Feature Churn (Critical)

**Goal**: Stop treating all changes as equal risk.

**Approach**: Use git commit message heuristics to classify commits:
- **Fix commits**: messages containing "fix", "bug", "patch", "revert", "hotfix",
  "issue #", "CVE-", "security"
- **Feature commits**: messages containing "feat", "add", "implement", "new",
  "support", "enable"
- **Refactor commits**: messages containing "refactor", "cleanup", "rename",
  "reorganize", "simplify", "extract"
- **Maintenance commits**: messages containing "update", "bump", "upgrade",
  "dependency", "version"
- **Unknown**: everything else (treated as feature by default)

Then compute two separate change metrics:
```
defect_churn = change_freq(fix_commits) × churn_rate(fix_commits)
feature_churn = change_freq(feature_commits) × churn_rate(feature_commits)
```

And redefine change_load to weight defect churn higher:
```
change_load = normalize(defect_churn × 3.0 + feature_churn × 1.0)
```

**Impact**: This directly addresses Problem 5. Repos with high feature velocity
(transformers, vllm) would see reduced change_load because their churn is
predominantly feature-driven. Repos with high bug-fix frequency would see
elevated change_load — correctly identifying actual risk.

**Validation**: Transformers/vllm should improve from D. Flask/gin should be
unaffected (their churn is mixed). Need to fix the FK constraint bug first
to test D-grade repos.

### Phase 4: Empirical Signal Weight Calibration

**Goal**: Replace engineering-judgment signal weights with data-driven weights.

**Approach**:
1. For each benchmark repo, compute signal counts by type
2. Look up external quality proxies: GitHub issue counts, CVE counts, Stack Overflow
   "bug" question frequency, contributor survey data (if available)
3. Run correlation analysis: which signal types best predict actual quality problems?
4. Set weights proportional to correlation strength

**Minimum viable version**: Use GitHub issue-to-commit ratio as a proxy for defect
rate. Repos with high issue rates should have high signal scores; repos with low
issue rates should have low signal scores. Adjust weights to maximize this correlation.

**Known limitation**: This requires external data collection. The current benchmark
only measures internal metrics. Phase 4 is research, not implementation.

### Phase 5: Adaptive Signal Penalty Curve

**Goal**: Better discrimination in the upper signal range.

**Proposal**: Replace `0.25 × x/(x+3)` with a piecewise curve that has more
dynamic range above x=5:
```
if x <= 5:    penalty = 0.25 × x / (x + 3)         (same as now)
if x > 5:     penalty = 0.156 + 0.094 × log₂(x/5)  (log growth above)
               capped at 0.30
```

This keeps the gentle start but allows more discrimination for signal-heavy repos.
kubernetes (x=11.4) would get penalty=0.28 instead of 0.19.

**Risk**: Raising the penalty cap from 0.25 to 0.30 could drop some repos a grade.
Must validate.

### Phase 6: Fix the DB FK Constraint Bug

**Goal**: Get all 28 benchmark repos passing.

**Priority**: High — we cannot validate key predictions (transformers D->C,
vllm D->C, ha-core D->C) without this fix. The FK constraint bug predates
spec 046 and blocks 14 repos.

---

## Implementation Priority

| Phase | Effort | Impact | Priority |
|-------|--------|--------|----------|
| Phase 6 (FK bug) | Low | High (unblocks validation) | **P0** |
| Phase 1 (formula structure) | Low | Medium (cleaner semantics) | P1 |
| Phase 3 (defect vs feature churn) | Medium | **Critical** (fixes root cause) | **P1** |
| Phase 2 (widen ranges) | Low | Low-Medium | P2 |
| Phase 5 (penalty curve) | Low | Medium | P2 |
| Phase 4 (signal weight calibration) | High (research) | Medium | P3 |

Phase 3 is the most impactful single change. Phase 6 is prerequisite for validating
anything on D-grade repos. Phase 1 is a clean-up that should happen before further
formula changes.

---

## Acceptance Criteria

- [ ] All 28 benchmark repos pass (FK bug fixed)
- [ ] Formula uses consistent combination semantics (multiplicative or additive, not mixed)
- [ ] Coupling and containment modifiers have measurable grade impact (>0.05 on at least 3 repos)
- [ ] Defect churn separated from feature churn in change_load computation
- [ ] transformers/vllm improve from D to C or better (with churn separation)
- [ ] No calibration regressions: gin >= B, flask C, express B
- [ ] Signal penalty discriminates between prometheus (1.7K) and kubernetes (33K)
- [ ] All weight choices documented with rationale (not just values)

## What This Spec Does NOT Do

- **No ML-based scoring.** The formula should remain interpretable and decomposable.
  If you can't explain why a repo got its grade by pointing to 3-4 numbers, the
  formula is too complex. Spec 045 (GNN risk model) is the ML path — this spec
  stays in the interpretable-formula lane.
- **No cross-repo calibration.** Grades are repo-internal assessments. Comparing
  "flask C vs django A" is meaningful only if both are measured on the same scale,
  which requires external ground truth we don't have. Don't try to calibrate the
  scale — calibrate the formula's internal consistency.
- **No new signal types.** Signal detection improvements belong in signal-specific
  specs (e.g., spec 023 for ghost coupling FPs). This spec only changes how
  existing signals feed into the health score.
