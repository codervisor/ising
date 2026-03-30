---
status: completed
created: 2026-03-30
priority: high
tags:
- validation
- real-world
- signal-quality
- gap-analysis
depends_on:
- '032'
created_at: 2026-03-30T05:14:00Z
updated_at: 2026-03-30T05:45:00Z
---

# OSS Testing Validation: Running Ising Against 6 Public Repositories

> **Status**: completed · **Priority**: high · **Created**: 2026-03-30

## Overview

Ran Ising against 6 notable open-source repositories spanning JS, Python, Rust, C, and TS. Goal: validate signal quality, identify gaps preventing real-world usefulness, and prioritize improvements. Extends the earlier 4-repo validation in spec 032.

**Method**: `--depth=500` shallow clones, `--since "12 months ago"` git history, all analysis commands (`build`, `safety`, `hotspots`, `signals`, `impact`, `stats`).

## Repositories Analyzed

| Repository | Languages | Nodes | Struct Edges | Change Edges | Signals | Critical / Danger | Build Time |
|---|---|---|---|---|---|---|---|
| **expressjs/express** | JS | 197 | 55 | 0 | 0 | 2 / 2 | ~1s |
| **pallets/flask** | Python | 555 | 606 | 0 | 4 | 20 / 3 | ~1s |
| **flask-admin/flask-admin** | Python | 919 | 1,348 | 45 | 67 | 14 / 30 | ~3s |
| **tokio-rs/axum** | Rust | 2,251 | 2,086 | 1 | 34 | 4 / 4 | ~2s |
| **redis/redis** | C (+Python) | 132 | 88 | 0 | 0 | 5 / 3 | ~4s |
| **facebook/react** | JS/TS | 14,166 | 10,637 | 78 | 67 | 21 / 17 | ~10s |

## Results by Repository

### Express (JS, 197 nodes)

- `lib/response.js` correctly flagged critical (SF=0.25) -- most changed, most complex.
- Zero signals, zero co-change edges from 26 commits.
- Mature, low-churn project. Thresholds may be too conservative for small repos.

### Flask (Python, 555 nodes)

- 20/83 modules critical (24%). `src/flask/app.py` at SF=0.10 -- correct god-object identification.
- Propagated risk dominates: `__init__.py` (zero change load) gets SF=0.19 purely via propagation. Over-flags re-export modules.
- 1 dependency cycle (wrappers <-> scaffold), 3 stable cores. All plausible.
- Zero co-change edges from 24 commits.

### Flask-Admin (Python, 919 nodes)

- Best signal output: 3 dependency cycles, 4 god modules, 23 ghost couplings, 26 over-engineering, 1 shotgun surgery.
- 45 co-change edges from 64 commits -- only repo where co-change reliably produced data.
- `flask_admin/model/base.py` and `contrib/sqla/view.py` correctly identified as god modules.
- **Sweet spot** for Ising: medium-sized Python project with enough churn.

### Axum (Rust, 2,251 nodes)

- 4 critical, 4 danger (3%). Reasonable for well-structured Rust.
- 1 real dependency cycle (`method_routing.rs <-> mod.rs`).
- 5 over-engineering signals are **false positives** -- Rust `mod` declarations from `lib.rs` are idiomatic, not single-consumer wrappers.
- 28 stable_core signals with fan-in=1 -- threshold too low for large codebase.
- Only 1 co-change edge from 111 commits.

### Redis (C, 132 nodes)

- **Ising completely missed the C codebase.** All 132 nodes are Python test/utility scripts.
- ~200+ C source files invisible due to no C language support.
- Most critical gap identified.

### React (JS/TS, 14,166 nodes)

- 21 critical, 17 danger out of 4,525 modules. Top critical files (`fiber/renderer.js`, `ReactFizzConfigDOM.js`, `ReactFiberCommitWork.js`) are known complex hot files -- good identification.
- 57 ghost couplings, 10 shotgun surgery. Feature flag files correctly flagged (co-change with 9+ files).
- ~10s build for 14K nodes -- good scalability.

## Identified Gaps

### P0 -- Critical

**GAP-1: Missing C/C++ language support.** Redis was invisible. C/C++ and Java are the most-used languages not yet supported. Blocks analysis of systems codebases.

**GAP-2: Co-change edge detection unreliable.** 5/6 repos produced near-zero co-change edges. Only Flask-Admin (64 commits -> 45 edges) and React (460 commits -> 78 edges) had meaningful data. Hypothesis: co-occurrence threshold too high for repos with focused commits. Fix: lower threshold, consider sliding-window co-change, report data sparsity.

### P1 -- High

**GAP-3: Risk propagation over-amplifies.** Flask: 24% critical rate. Re-export modules (`__init__.py`, `signals.py`) flagged critical with zero change load. Fix: attenuate propagated risk for low-structural-weight modules, add minimum change_load for critical-via-propagation.

**GAP-4: `impact` command broken.** `ising impact "src/flask/app.py"` returns "No data found" despite file being #1 critical. Queries `nodes.id` (internal) instead of file path. Fix: accept file paths matching `node_id` from safety output.

**GAP-7: No test file separation.** Test files dominate rankings (Express #2, Redis all 5, Flask #16). Fix: add `--exclude-tests` or auto-detect test directories, tag test files in output.

### P2 -- Medium

**GAP-5: Rust false over-engineering signals.** `lib.rs -> macros.rs` flagged as "consider inlining." Rust `mod` declarations are idiomatic. Fix: skip over-engineering for `lib.rs`/`main.rs` mod declarations.

**GAP-6: Stable core threshold too low.** Axum: 28 stable_core signals with fan-in=1 in a 2,251-node codebase. Fix: scale fan-in threshold relative to codebase size (minimum fan-in >= 5, or top 5% percentile).

**GAP-8: No size-aware calibration.** Same SF zone thresholds for 132-node and 14K-node repos. Small repos get inflated critical counts, medium repos with high coupling get over-flagged. Fix: report critical/total ratio, consider adaptive thresholds.

**GAP-9: No incremental diff mode.** Full rebuild every time. For CI/PR workflows, need `ising diff --base <commit>` that recomputes only changed files and transitive dependents.

### P3 -- Low

**GAP-10: Ghost coupling lacks actionability.** 57 signals in React with no investigation guidance. Fix: add co-change frequency, common ancestors, suggested investigation path.

**GAP-11: No CI output formats.** Missing SARIF and GitHub annotations. Fix: add `--format sarif` for GitHub Code Scanning integration.

## What Works Well

1. **Structural parsing** solid across Python, JS/TS, Rust, Go, Vue.
2. **Hotspot ranking** consistently useful -- top hotspots match expert intuition.
3. **God module detection** accurate (Flask-Admin).
4. **Shotgun surgery** nailed React's feature flag pattern.
5. **Performance** good -- 14K nodes in ~10s, Jacobi converges reliably (max 62 iterations).
6. **Dependency cycle detection** finds real cycles.

## Raw Data

### Safety Zone Distribution

| Repo | Critical | Danger | Warning | Healthy | Stable | Total |
|---|---|---|---|---|---|---|
| Express | 2 (1%) | 2 (1%) | 0 | 0 | 138 (97%) | 142 |
| Flask | 20 (24%) | 3 (4%) | 0 | 0 | 60 (72%) | 83 |
| Flask-Admin | 14 (5%) | 30 (12%) | ~20 (8%) | ~30 (12%) | ~166 (64%) | 260 |
| Axum | 4 (1%) | 4 (1%) | 3 (1%) | 5 (2%) | 285 (95%) | 301 |
| Redis | 5 (11%) | 3 (7%) | 2 (5%) | 2 (5%) | 32 (73%) | 44 |
| React | 21 (<1%) | 17 (<1%) | ~50 (1%) | ~100 (2%) | ~4337 (96%) | 4,525 |

### Signal Distribution

| Signal Type | Express | Flask | Flask-Admin | Axum | Redis | React | Total |
|---|---|---|---|---|---|---|---|
| DependencyCycle | 0 | 1 | 3 | 1 | 0 | 0 | 5 |
| GodModule | 0 | 0 | 4 | 0 | 0 | 0 | 4 |
| GhostCoupling | 0 | 0 | 23 | 0 | 0 | 57 | 80 |
| ShotgunSurgery | 0 | 0 | 1 | 0 | 0 | 10 | 11 |
| OverEngineering | 0 | 0 | 26 | 5 | 0 | 0 | 31 |
| StableCore | 0 | 3 | 10 | 28 | 0 | 0 | 41 |
| **Total** | **0** | **4** | **67** | **34** | **0** | **67** | **172** |

### Top 3 Hotspots Per Repo

| Repo | #1 | #2 | #3 |
|---|---|---|---|
| Express | lib/response.js | lib/utils.js | test/support/utils.js |
| Flask | tests/test_basic.py | src/flask/app.py | tests/test_blueprints.py |
| Flask-Admin | tests/sqla/test_basic.py | model/base.py | contrib/sqla/view.py |
| Axum | routing/mod.rs | routing/method_routing.rs | extract/ws.rs |
| Redis | vector-sets/test.py | req-res-log-validator.py | tests/with.py |
| React | fiber/renderer.js | ReactFizzConfigDOM.js | ReactFiberCommitWork.js |
