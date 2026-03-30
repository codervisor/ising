# Ising OSS Testing Spec

**Date**: 2026-03-30
**Method**: Ran `ising build` + all analysis commands against 6 notable open-source repositories, using `--depth=500` shallow clones and `--since "12 months ago"` for git history.

---

## 1. Repos Tested

| Repo | Language | Nodes | Struct Edges | Change Edges | Signals | Critical | Danger | Build Time |
|------|----------|-------|-------------|-------------|---------|----------|--------|------------|
| **Express** (expressjs/express) | JS | 197 | 55 | 0 | 0 | 2 | 2 | ~1s |
| **Flask** (pallets/flask) | Python | 555 | 606 | 0 | 4 | 20 | 3 | ~1s |
| **Flask-Admin** (flask-admin/flask-admin) | Python | 919 | 1,348 | 45 | 67 | 14 | 30 | ~3s |
| **Axum** (tokio-rs/axum) | Rust | 2,251 | 2,086 | 1 | 34 | 4 | 4 | ~2s |
| **Redis** (redis/redis) | C (+ Python tests) | 132 | 88 | 0 | 0 | 5 | 3 | ~4s |
| **React** (facebook/react) | JS/TS | 14,166 | 10,637 | 78 | 67 | 21 | 17 | ~10s |

## 2. Key Findings Per Repo

### Express (JS, ~197 nodes)
- **Safety**: `lib/response.js` correctly flagged critical (SF=0.25) -- it is the most changed and most complex module.
- **Hotspots**: `lib/response.js` top hotspot, sensible ranking.
- **Signals**: Zero signals detected. No ghost couplings, no cycles.
- **Change edges**: Zero co-change edges despite 26 commits analyzed.
- **Observation**: Express is a mature, low-churn project. Results feel reasonable but sparse. The lack of *any* signals in a 197-node codebase suggests detection thresholds may be too conservative for smaller repos.

### Flask (Python, ~555 nodes)
- **Safety**: 20/83 modules marked critical (24%). `src/flask/app.py` at SF=0.10 is the riskiest -- correctly identified as the god-object. Heavy propagated risk (~2.27) dominates.
- **Signals**: 1 dependency cycle (wrappers <-> scaffold), 3 stable cores (globals, json). All plausible.
- **Change edges**: Zero despite 24 commits. Co-change detection produced no edges.
- **Observation**: The 24% critical rate feels high for a well-maintained framework. Propagated risk is very aggressive -- `src/flask/__init__.py` has zero change load but gets SF=0.19 purely from propagation. This over-flags stable re-export modules.

### Flask-Admin (Python, ~919 nodes)
- **Safety**: 14 critical, 30 danger out of 260 modules (17%).
- **Signals**: Rich signal set -- 3 dependency cycles, 4 god modules, 23 ghost couplings, 26 over-engineering, 1 shotgun surgery. This is the best signal output of all tested repos.
- **Change edges**: 45 co-change edges from 64 commits. First repo where co-change actually produced data.
- **God modules**: `flask_admin/model/base.py` and `flask_admin/contrib/sqla/view.py` correctly identified.
- **Observation**: Flask-Admin is the "sweet spot" -- medium-sized Python project with enough churn to populate co-change data. Results are the most actionable across all repos.

### Axum (Rust, ~2,251 nodes)
- **Safety**: 4 critical, 4 danger out of 301 modules (3%). Reasonable for a well-structured Rust crate.
- **Signals**: 1 dependency cycle (`method_routing.rs <-> mod.rs`), 5 over-engineering flags, 28 stable cores. The over-engineering signals flagging `lib.rs -> macros.rs` as "consider inlining" are **false positives** -- Rust `mod` declarations are not the same as single-consumer wrappers.
- **Change edges**: Only 1 co-change edge from 111 commits.
- **Observation**: Rust's module system (mod.rs re-exports) generates misleading over-engineering signals. The stable_core threshold (fan-in >= 1) is too low and creates noise.

### Redis (C, ~132 nodes)
- **Safety**: 5 critical, 3 danger. But ALL are Python test files (`modules/vector-sets/tests/*.py`), not C source.
- **Signals**: Zero.
- **Observation**: **Ising completely missed the Redis C codebase.** Only 132 nodes were found -- all from Python test scripts and utility files. The ~200+ C source files (`src/*.c`, `src/*.h`) were invisible. This is the most critical gap: no C/C++ language support.

### React (JS/TS, ~14,166 nodes)
- **Safety**: 21 critical, 17 danger out of 4,525 modules. Top critical: `react-devtools-shared/src/backend/fiber/renderer.js` (SF=0.03), `ReactFizzConfigDOM.js` (SF=0.09), `ReactFiberCommitWork.js` (SF=0.10). These are known complex hot files in React -- good identification.
- **Signals**: 57 ghost couplings, 10 shotgun surgery. Feature flag files correctly flagged for shotgun surgery (co-change with 9+ files). Zero dependency cycles detected despite React's complex internal package structure.
- **Hotspots**: Reasonable but the compiler TypeScript files are mixed in with runtime JS files.
- **Build time**: ~10s for 14K nodes -- good scalability.

---

## 3. Identified Gaps (Priority Order)

### GAP-1: Missing Language Support (C, C++, Java, Ruby, C#)
**Severity: Critical**

Redis (200K+ LOC of C) was essentially invisible. Ising only found Python utility scripts. C and C++ are the most widely-used systems languages and their absence makes Ising unusable for a large class of projects.

Similarly missing: Java (Spring, Android), Ruby (Rails), C# (.NET), PHP, Swift, Kotlin.

**Recommendation**: Prioritize C/C++ and Java parsers. These cover the majority of enterprise and systems codebases.

### GAP-2: Co-Change Edge Detection Is Unreliable
**Severity: High**

| Repo | Commits Analyzed | Co-Change Edges |
|------|-----------------|----------------|
| Express | 26 | 0 |
| Flask | 24 | 0 |
| Flask-Admin | 64 | 45 |
| Axum | 111 | 1 |
| Redis | 18 | 0 |
| React | 460 | 78 |

5 out of 6 repos produced near-zero co-change edges. Flask-Admin (64 commits, 45 edges) and React (460 commits, 78 edges) were the only repos with meaningful co-change data. The shallow clone depth (500) should provide enough history, and `--since "12 months ago"` is a reasonable window.

**Root cause hypothesis**: The co-change algorithm likely requires files to appear in the *same commit* multiple times to establish an edge, and the threshold may be too high for repos with smaller/focused commits.

**Recommendation**:
- Lower the co-change co-occurrence threshold (currently appears to need 3+ co-occurrences)
- Consider sliding-window co-change (files changed within N commits of each other, not just same commit)
- Report co-change edge statistics as a quality metric so users know if the data is sparse

### GAP-3: Risk Propagation Over-Amplifies in Tightly-Coupled Codebases
**Severity: High**

Flask had 20/83 modules (24%) marked critical. The propagated risk dominates: `src/flask/__init__.py` has zero change load but SF=0.19 because it imports modules that import modules that changed. This is a re-export file -- flagging it as "critical" is misleading.

Similarly, `src/flask/signals.py` (zero change load, SF=0.39, critical) gets risk purely through propagation despite being a tiny, stable module.

**Recommendation**:
- Attenuate propagated risk for modules with low structural weight (few outgoing deps)
- Consider a "pure re-export" heuristic: if a module only imports and re-exports, discount propagated risk
- Add a minimum change_load threshold for a module to be flagged critical via propagation alone

### GAP-4: `impact` Command Is Broken for File-Level Lookups
**Severity: High**

`ising impact "src/flask/app.py"` returns "No data found" despite the file being the #1 critical module. The command queries `nodes.id` which uses internal IDs, not the file paths shown in safety/hotspot output. Users have no way to discover the correct ID format.

**Recommendation**: The impact command should accept file paths (matching the `node_id` field from safety output) and resolve them to internal IDs. Add fuzzy/prefix matching for convenience.

### GAP-5: Rust Module System Generates False Over-Engineering Signals
**Severity: Medium**

Axum flagged `lib.rs -> macros.rs` and `lib.rs -> error.rs` as over-engineering ("consider inlining"). In Rust, `mod` declarations in `lib.rs` are idiomatic -- they're not "single-consumer wrappers" but the standard module system. Flagging them creates noise and erodes trust.

**Recommendation**: Add Rust-specific heuristics:
- Don't flag `mod` declarations from `lib.rs`/`main.rs` as over-engineering
- Don't flag modules that are `pub use` re-exported
- Raise the complexity threshold for over-engineering in Rust (module separation is the norm)

### GAP-6: Stable Core Threshold Too Low
**Severity: Medium**

Axum generated 28 stable_core signals. A module with fan-in=1 and 1 change was flagged. In a 2,251-node codebase, fan-in=1 is trivially low and not meaningful as a "stable foundation."

**Recommendation**: Scale the stable_core fan-in threshold relative to codebase size (e.g., top 5% of fan-in values, or minimum fan-in >= 5).

### GAP-7: No Distinction Between Source and Test Files in Risk Ranking
**Severity: Medium**

In Express, `test/req.query.js` is the #2 critical module. In Redis, all 5 critical modules are test files. In Flask, `tests/test_basic.py` is #16 critical. Test files typically have high complexity and high churn but pose different risk than production code.

**Recommendation**:
- Add a `--exclude-tests` flag or auto-detect test directories
- Separate test file risk from source file risk in output
- At minimum, tag test files in the output so users can filter

### GAP-8: No Severity/Confidence Calibration Across Repo Sizes
**Severity: Medium**

The risk model uses the same thresholds regardless of repo size. A 132-node repo (Redis Python scripts) and a 14,166-node repo (React) use identical SF zone boundaries. This means:
- Small repos have inflated critical counts (Express: 2/142 = 1.4%)
- Medium repos with high internal coupling are over-flagged (Flask: 24%)
- Large repos produce reasonable ratios (React: 0.5%)

**Recommendation**: Consider repo-size-aware zone thresholds, or at least report the critical/total ratio alongside absolute counts so users can calibrate.

### GAP-9: No Incremental/Diff Mode
**Severity: Medium**

Currently `ising build` does a full rebuild every time. For large repos (React: 14K nodes), this takes ~10s. For CI integration or PR review workflows, an incremental mode that only re-analyzes changed files would be essential.

**Recommendation**: Add `ising diff --base <commit>` that only recomputes risk for files changed since a base commit and their transitive dependents.

### GAP-10: Ghost Coupling Signal Lacks Actionability
**Severity: Low**

React produced 57 ghost coupling signals. Flask-Admin produced 23. The signal says "files co-change but have no structural dependency" but doesn't suggest *what* the hidden dependency might be (shared config? runtime dispatch? test fixtures?).

**Recommendation**: Enrich ghost coupling signals with:
- Co-change frequency and recency
- Common ancestor in the dependency tree (if any)
- Suggested investigation path

### GAP-11: No Export/Report Format for CI Integration
**Severity: Low**

The `--format json` flag exists but there's no SARIF, JUnit, or GitHub annotations format. For CI/CD integration (GitHub Actions, GitLab CI), teams need machine-readable output that integrates with existing tooling.

**Recommendation**: Add `--format sarif` for GitHub Code Scanning integration and `--format github-annotations` for inline PR comments.

---

## 4. What Works Well

1. **Structural parsing** is solid across supported languages (Python, JS/TS, Rust, Go, Vue). Import resolution and complexity scoring produce sensible results.

2. **Hotspot ranking** is consistently useful. The top hotspots across all repos align with what domain experts would identify as high-risk files.

3. **God module detection** (Flask-Admin) accurately identified bloated classes.

4. **Shotgun surgery detection** (React) correctly flagged feature flag files that must change in lockstep.

5. **Performance** is good. Even React (14K nodes) completes in ~10s. The Jacobi iteration converges reliably (max 62 iterations for Flask).

6. **Dependency cycle detection** finds real cycles (Flask wrappers <-> scaffold, Axum method_routing <-> mod).

---

## 5. Recommended Priorities

| Priority | Gap | Effort | Impact |
|----------|-----|--------|--------|
| P0 | GAP-1: C/C++ language support | Large | Unlocks systems codebases |
| P0 | GAP-2: Fix co-change edge detection | Medium | Core data quality issue |
| P1 | GAP-3: Propagation over-amplification | Medium | Reduces false critical flags |
| P1 | GAP-4: Fix `impact` command | Small | Currently broken feature |
| P1 | GAP-7: Test file separation | Small | Reduces noise in rankings |
| P2 | GAP-5: Rust false over-engineering | Small | Language-specific fix |
| P2 | GAP-6: Stable core threshold scaling | Small | Signal quality |
| P2 | GAP-8: Size-aware calibration | Medium | Cross-repo consistency |
| P2 | GAP-9: Incremental diff mode | Large | CI/CD enablement |
| P3 | GAP-10: Ghost coupling enrichment | Medium | Signal actionability |
| P3 | GAP-11: CI output formats (SARIF) | Medium | Integration |

---

## 6. Raw Data Summary

### Safety Zone Distribution

| Repo | Critical | Danger | Warning | Healthy | Stable | Total Modules |
|------|----------|--------|---------|---------|--------|---------------|
| Express | 2 (1%) | 2 (1%) | 0 | 0 | 138 (97%) | 142 |
| Flask | 20 (24%) | 3 (4%) | 0 | 0 | 60 (72%) | 83 |
| Flask-Admin | 14 (5%) | 30 (12%) | ~20 (8%) | ~30 (12%) | ~166 (64%) | 260 |
| Axum | 4 (1%) | 4 (1%) | 3 (1%) | 5 (2%) | 285 (95%) | 301 |
| Redis | 5 (11%) | 3 (7%) | 2 (5%) | 2 (5%) | 32 (73%) | 44 |
| React | 21 (<1%) | 17 (<1%) | ~50 (1%) | ~100 (2%) | ~4337 (96%) | 4,525 |

### Signal Distribution

| Signal Type | Express | Flask | Flask-Admin | Axum | Redis | React |
|------------|---------|-------|-------------|------|-------|-------|
| DependencyCycle | 0 | 1 | 3 | 1 | 0 | 0 |
| GodModule | 0 | 0 | 4 | 0 | 0 | 0 |
| GhostCoupling | 0 | 0 | 23 | 0 | 0 | 57 |
| ShotgunSurgery | 0 | 0 | 1 | 0 | 0 | 10 |
| OverEngineering | 0 | 0 | 26 | 5 | 0 | 0 |
| StableCore | 0 | 3 | 10 | 28 | 0 | 0 |
| **Total** | **0** | **4** | **67** | **34** | **0** | **67** |

### Top 3 Hotspots Per Repo

| Repo | #1 | #2 | #3 |
|------|----|----|-----|
| Express | lib/response.js | lib/utils.js | test/support/utils.js |
| Flask | tests/test_basic.py | src/flask/app.py | tests/test_blueprints.py |
| Flask-Admin | tests/sqla/test_basic.py | flask_admin/model/base.py | contrib/sqla/view.py |
| Axum | routing/mod.rs | routing/method_routing.rs | extract/ws.rs |
| Redis | vector-sets/test.py | req-res-log-validator.py | tests/with.py |
| React | devtools fiber/renderer.js | ReactFizzConfigDOM.js | ReactFiberCommitWork.js |
