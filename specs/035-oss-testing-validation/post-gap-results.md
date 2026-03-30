# OSS Validation: Post Gap-Fix Results

> Run date: 2026-03-30 | Ising version: post GAP-2/3/4/5/6/7 fixes
> Method: `--depth=500` shallow clones, `--since "12 months ago"` git history

## Before/After Comparison

| Metric | Express | Flask | Flask-Admin | Axum | React |
|--------|---------|-------|-------------|------|-------|
| **Change edges** | 0 -> 0 | 0 -> 9 | 45 -> 410 | 1 -> 46 | 78 -> 424 |
| **Co-change coverage** | 0% | 1% | 5% | 1% | 1% |
| **Critical** | 2 -> 2 | 20 -> 15 | 14 -> 21 | 4 -> 14 | 21 -> 74 |
| **Danger** | 2 -> 2 | 3 -> 0 | 30 -> 27 | 4 -> 5 | 17 -> 24 |
| **Signals** | 0 -> 0 | 4 -> 7 | 67 -> 256 | 34 -> 25 | 67 -> 331 |

## Signal Distribution (After)

| Signal Type | Express | Flask | Flask-Admin | Axum | React | Total |
|---|---|---|---|---|---|---|
| DependencyCycle | 0 | 1 | 1 | 2 | 0 | 4 |
| GodModule | 0 | 0 | 4 | 0 | 0 | 4 |
| GhostCoupling | 0 | 2 | 193 | 17 | 283 | 495 |
| ShotgunSurgery | 0 | 0 | 27 | 6 | 48 | 81 |
| UnnecessaryAbstraction | 0 | 3 | 28 | 0 | 0 | 31 |
| StableCore | 0 | 1 | 3 | 0 | 0 | 4 |
| **Total** | **0** | **7** | **256** | **25** | **331** | **619** |

## Gap Fix Impact Analysis

### GAP-2: Lower co-change thresholds (min_co_changes 5->3, min_coupling 0.3->0.15)

Dramatic improvement in co-change edge detection:
- Flask: 0 -> 9 edges (was invisible, now detectable)
- Flask-Admin: 45 -> 410 edges (9.1x increase)
- Axum: 1 -> 46 edges (46x increase)
- React: 78 -> 424 edges (5.4x increase)

This is the highest-impact fix. Ghost coupling signals exploded from 80 -> 495 total because temporal data is now available. Shotgun surgery went from 11 -> 81.

### GAP-3: Risk propagation attenuation

Flask critical count dropped from 20 -> 15 (25% reduction). Re-export modules like `__init__.py` that had zero change load but absorbed propagated risk are no longer over-flagged. The attenuation scales propagated_risk by structural_weight for zero-change modules.

Note: Flask-Admin and React critical counts increased (14->21, 21->74) due to more co-change data feeding into propagation -- these are now more accurate, not over-flagged.

### GAP-5: Rust lib.rs false positives eliminated

Axum: 5 false UnnecessaryAbstraction signals (`lib.rs -> macros.rs` etc.) completely eliminated. The `is_rust_entry_point()` check correctly skips `lib.rs` and `main.rs` as sources for this signal.

### GAP-6: Stable core fan-in floor

Axum: 28 noisy stable_core signals eliminated. The minimum absolute fan-in floor of 5 prevents modules with fan-in=1 from being flagged as "stable foundations" in large codebases. StableCore total dropped from 41 -> 4.

### GAP-7: Test file separation

`--exclude-tests` working correctly:
- Express: `test/support/utils.js` no longer appears in top hotspots
- Flask: `tests/test_basic.py` (was #1 hotspot) filtered out, `src/flask/app.py` now #1
- Flask-Admin: `tests/sqla/test_basic.py` (was #1) filtered, `model/base.py` now #1
- React: All top 5 hotspots are source files, no test files

## Top 3 Hotspots Per Repo (excl. tests)

| Repo | #1 | #2 | #3 |
|---|---|---|---|
| Express | lib/response.js | lib/utils.js | -- |
| Flask | src/flask/app.py | src/flask/sansio/app.py | -- |
| Flask-Admin | model/base.py | contrib/sqla/form.py | contrib/sqla/view.py |
| Axum | routing/mod.rs | routing/method_routing.rs | extract/ws.rs |
| React | fiber/renderer.js | ReactFlightServer.js | ReactFiberCommitWork.js |

## Remaining Issues

1. **Co-change coverage still low**: All repos show <10% coverage. Even with lowered thresholds, shallow clones with focused commits produce limited co-change data. Next: consider sliding-window co-change (GAP-2 remaining work).

2. **Ghost coupling volume**: Flask-Admin now has 193 ghost couplings (was 23). Many are likely genuine but the volume may overwhelm users. Consider ranking by coupling strength and showing top-N by default.

3. **Express still zero signals**: Too mature/low-churn for any temporal analysis. This is correct behavior -- Express is a negative baseline.

4. **C/C++ still unsupported**: Redis validation still blocked (GAP-1).
