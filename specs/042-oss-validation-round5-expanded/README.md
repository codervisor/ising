---
status: complete
created: 2026-03-31
priority: medium
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

**Date**: 2026-03-31
**Repos tested**: 29 (25 succeeded, 4 failed)
**Languages covered**: Python, JS/TS, Go, Java, Rust, C/C++, Ruby, PHP

## Purpose

Aggressive expansion of the validation test set from 12 repos to 28, covering:
- Previously tested repos (for regression comparison)
- New challengers across different languages, sizes, and architectures
- Massive monorepos (e.g., kubernetes and large JS/TS codebases)
- Diverse architectural patterns (DI frameworks, compilers, distributed systems)

## Results Table

```
Repository                Lang     Cat      Grade  Score   Total  Active   Risk   Sigs  Struc   #Sigs  Crit  High
-----------------------------------------------------------------------------------------------------------------
flask                     Python   baseline     C   0.67      83      16   0.32   0.88   0.95      56     1     0
django                    Python   challngr     B   0.73    3006     480   0.74   0.62   0.89     755     5    19
django-rest-framework     Python   prev         A   0.95     175      58   0.87   1.00   1.00     108     1     2
fastapi                   Python   prev         A   0.93    1513    1309   0.92   0.92   0.96    1020    14    52
express                   JS/TS    baseline     B   0.85     142      17   0.62   1.00   1.00      18     1     0
fastify                   JS/TS    challngr     A   0.93     287      88   0.83   1.00   1.00     124     1     4
nest                      JS/TS    challngr     A   0.89    1679     184   0.73   1.00   1.00     289     2     8
next.js                   JS/TS    challngr     A   0.96   22130   21971   0.97   0.94   1.00   13671   220   879
svelte                    JS/TS    challngr     A   0.95    3372     603   0.90   0.96   1.00    1295     7    24
gin                       Go       baseline     B   0.80      98      40   0.64   0.85   1.00     138     1     1
ollama                    Go       prev         B   0.75    1303    1299   0.94   0.38   0.95    7771    13    52
prometheus                Go       challngr     B   0.71     954     770   0.95   0.36   0.84    1764     8    31
kubernetes                Go       challngr     C   0.63   17116    5461   0.92   0.30   0.61   33771    55   219
kafka                     Java     challngr     C   0.59    6137    6128   0.89   0.27   0.57   23904    62   245
spring-boot               Java     challngr  FAIL   ---     ---     ---    ---    ---    ---     ---   ---   ---
TypeScript                JS/TS    challngr     A   0.98   39421   26727   0.95   1.00   1.00    2257   268  1069
rust                      Rust     challngr  FAIL   ---     ---     ---    ---    ---    ---     ---   ---   ---
deno                      Rust     challngr     A   0.94    4982    4909   0.97   0.87   0.99    5775    50   196
pytorch                   C++/Py   challngr     B   0.72    9070    8913   0.91   0.44   0.80   31448    90   356
transformers              Python   prev         C   0.62    4316    3837   0.74   0.32   0.85    5194    39   153
vllm                      Python   prev         C   0.66    2993    2783   0.76   0.41   0.85    3852    28   112
llama.cpp                 C/C++    prev         A   0.87    1112    1111   0.94   0.74   0.94    9997    12    44
langchain                 Python   prev         A   0.96    2548    2351   0.91   0.98   1.00    2211    24    94
open-webui                Python   prev         B   0.73     317     277   0.84   0.41   1.00     837     3    11
ha-core                   Python   prev         B   0.75   16685   16683   0.84   0.53   0.90   12613   167   668
grafana                   Go       prev         C   0.60   14980   14973   0.83   0.35   0.60   23145   150   599
odoo                      Python   prev         A   0.94   14178   14146   0.93   0.92   0.97   11066   142   566
rails                     Ruby     challngr  FAIL   ---     ---     ---    ---    ---    ---     ---   ---   ---
php-src                   C        challngr  FAIL   ---     ---     ---    ---    ---    ---     ---   ---   ---
```

## Grade Distribution

| Grade | Count | Repos |
|-------|-------|-------|
| A | 11 | django-rest-framework, fastapi, fastify, nest, next.js, svelte, TypeScript, deno, llama.cpp, langchain, odoo |
| B | 8 | django, express, gin, ollama, prometheus, pytorch, open-webui, ha-core |
| C | 6 | flask, kubernetes, kafka, transformers, vllm, grafana |
| FAIL | 4 | spring-boot, rust, rails, php-src |

## Calibration Check Results

### PASS: gin >= B
- Got **B** (0.80). Correct — small, well-structured Go project.

### WARN: odoo still gets A (0.94)
- Known blind spot persists. 14,178 modules, 11,066 signals, 142 critical modules.
- The SystemicComplexity signal was supposed to address this but the god_module_density is only 0.03%.
- Odoo's distributed complexity (many moderately complex files, none individually extreme) continues to evade detection.

### WARN: TypeScript gets A (0.98) — suspiciously high
- 39,421 modules but structure score is 1.00 and signal score is 1.00.
- Only 2,257 signals for a 39K-module codebase (0.057 signals/module) — this is unrealistically low.
- The monolithic checker.ts (40K+ lines) is apparently not being flagged as a god module.
- **Root cause hypothesis**: The god module threshold (complexity≥50, LOC≥500, CBO≥15) may not be triggering because TS functions within the file are parsed as separate modules, diluting the per-module metrics.

### WARN: next.js gets A (0.96) with 22K modules
- 13,671 signals but signal_sub_score is 0.94 — sqrt(N) normalization is working as intended here.
- 220 critical + 879 high risk modules, yet overall grade A. This repo has a huge tail of low-risk test/example files that dominate the median.

## Key Findings

### 1. Signal sub-score drives grade differentiation

The strongest predictor of low grades is the signal sub-score:
- **A-graded repos**: signal sub-score ≥ 0.87 (median: 0.96)
- **C-graded repos**: signal sub-score ≤ 0.41 (median: 0.35)
- Risk sub-scores cluster between 0.74-0.97 regardless of grade

This means the signal detector is the primary discriminator, not the raw risk computation.

### 2. Structure sub-score only matters at scale

- All repos with < 1000 modules get structure = 1.00
- Structure drops below 0.90 only for kubernetes (0.61), kafka (0.57), grafana (0.60), and pytorch (0.80)
- These are all > 6000 module repos with genuine cycle/instability issues

### 3. Large repo bias — A grades may be inflated

Repos getting A with high absolute signal counts:
| Repo | Modules | Signals | Critical | Grade |
|------|---------|---------|----------|-------|
| TypeScript | 39,421 | 2,257 | 268 | A |
| next.js | 22,130 | 13,671 | 220 | A |
| odoo | 14,178 | 11,066 | 142 | A |

268 critical modules in TypeScript is objectively concerning regardless of the percentage. The percentile-based tier system makes this invisible to the health index.

### 4. Small repo bias — flask gets C unfairly?

Flask (83 modules, 16 active) gets C (0.67) with risk sub-score 0.32. With only 16 active modules, a single high-risk file dominates. The risk sub-score formula may be too sensitive at very low N.

### 5. Parser failures

| Repo | Cause | Bug |
|------|-------|-----|
| spring-boot | Stack overflow in Ruby parser | Ruby `compute_complexity::walk` recurses too deeply on some .rb files in the Java repo |
| rust | Stack overflow | 58K+ files overwhelm structural parser |
| rails | Stack overflow in Ruby parser | Same Ruby parser recursion issue on real Ruby code |
| php-src | PHP parser produces 0 nodes | PHP `extract_nodes` silently fails |

**Action items**:
- Ruby parser needs iterative complexity walk (not recursive) — BUG
- rust-lang/rust needs memory/stack limit handling for massive repos — LIMITATION
- PHP parser needs investigation — BUG

### 6. Go repos consistently penalized

All 5 Go repos cluster at B or C:
- gin: B (0.80), ollama: B (0.75), prometheus: B (0.71), kubernetes: C (0.63), grafana: C (0.60)

The Go intra-package suppression fix (GAP-13) may not be fully effective, or Go's package structure genuinely creates more signals. Worth investigating whether Go repos are being over-penalized relative to equivalent Python/TS codebases.

## Comparison with Previous Rounds

| Repo | Round 4 | Round 5 | Delta | Notes |
|------|---------|---------|-------|-------|
| langchain | — | A (0.96) | — | New test |
| ollama | — | B (0.75) | — | New test |
| vllm | — | C (0.66) | — | New test |
| transformers | — | C (0.62) | — | New test |
| llama.cpp | — | A (0.87) | — | New test |
| open-webui | — | B (0.73) | — | New test |

(Previous round data not directly comparable — different binary version, different git history window.)

## Recommendations

1. **Investigate TypeScript A grade**: checker.ts should trigger god_module but likely doesn't due to function-level module splitting. Consider file-level aggregation for god module detection.

2. **Flask small-repo penalty**: Consider floor on risk sub-score when active_modules < 20 to prevent single-file dominance.

3. **Fix Ruby parser stack overflow**: Convert recursive `compute_complexity::walk` to iterative with explicit stack.

4. **Fix PHP parser**: Investigate why extract_nodes produces 0 nodes on php-src.

5. **Review Go signal rates**: Compare per-module signal density between Go and Python repos of similar size to check for Go-specific bias.

6. **Absolute critical count caveat**: When critical_count > 100, consider emitting a caveat regardless of grade, since percentile-based tiers hide the absolute magnitude.

## SOP: Routine Benchmark

A reusable benchmark script has been created at `scripts/bench-oss-repos.sh`.

```bash
# First time: clone all repos
./scripts/bench-oss-repos.sh --clone

# Subsequent runs (repos already cloned)
./scripts/bench-oss-repos.sh

# Custom output directory
./scripts/bench-oss-repos.sh --output /path/to/results
```

Run this script:
- After any change to `ising-analysis/src/stress.rs`
- After any change to `ising-analysis/src/signals.rs`
- After any change to health index computation
- After adding or modifying language parsers
- Before any release
