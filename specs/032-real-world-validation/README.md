---
status: completed
created: 2026-03-23
priority: high
tags:
- validation
- real-world
- signal-quality
- new-signals
depends_on:
- '023'
created_at: 2026-03-23T05:18:00Z
updated_at: 2026-03-23T05:30:00Z
---

# Real-World Validation: Running Ising Against Public Repositories

> **Status**: completed · **Priority**: high · **Created**: 2026-03-23

## Overview

Ran Ising (with the 4 new signals from this branch: DependencyCycle, GodModule, ShotgunSurgery, UnstableDependency) against 4 public GitHub repositories spanning different sizes, languages, and architectures. The goal: validate that Ising finds real structural issues, not just noise.

## Repositories Analyzed

| Repository | Languages | Nodes | Structural Edges | Change Edges | Signals | Description |
|---|---|---|---|---|---|---|
| **codervisor/ising** | Rust | 301 | 256 | 2 | 8 | This project (meta — self-analysis) |
| **codervisor/lean-spec** | Rust + TypeScript | 4,148 | 3,166 | 108 | 118 | Spec management platform (monorepo) |
| **codervisor/synodic** | Rust | 362 | 299 | 0 | 39 | AI evaluation harness |
| **crawlab-team/crawlab** | Go + TypeScript | 3,725 | 3,118 | 0 | 7 | Web crawler management platform |

## Results by Repository

### 1. codervisor/ising (self-analysis)

**Signal breakdown**: 1 god_module, 1 ghost_coupling, 6 stable_core

**Key findings:**
- `ising-cli/src/main.rs` flagged as **GodModule** (complexity 113, 519 LOC, 22 deps) — accurate. The CLI entry point handles all command dispatch, build orchestration, and output formatting in one file.
- `change.rs ↔ structural.rs` **GhostCoupling** at 100% co-change rate — correctly reduced to severity 0.3 because they share parent `lib.rs`. Description notes: "verify no hidden dependency."
- 6 **StableCore** guards on language extractors (python.rs, vue.rs, go.rs) and DB modules — accurate, these are stable utility modules with high fan-in.

**Verdict**: Clean, well-modularized codebase with one actionable finding (main.rs needs decomposition).

### 2. codervisor/lean-spec (most active repo)

**Signal breakdown**: 19 god_module, 42 ghost_coupling, 6 shotgun_surgery, 2 dependency_cycle, 1 unnecessary_abstraction, 48 stable_core

This is the richest dataset — a fast-moving monorepo with 367 commits in 12 months.

**Top god modules (all real):**

| File | Complexity | LOC | Fan-out | Severity |
|---|---|---|---|---|
| `sessions/manager/lifecycle.rs` | 234 | 1,841 | 33 | 37.9 |
| `sessions/runner.rs` | 158 | 1,492 | 47 | 29.5 |
| `commands/session.rs` | 191 | 1,053 | 30 | 16.1 |
| `leanspec-sync-bridge/src/main.rs` | 165 | 811 | 41 | 14.6 |
| `sessions/worktree.rs` | 165 | 660 | 44 | 12.8 |

All are legitimately oversized modules that would benefit from decomposition.

**Shotgun surgery (all real):**
- `packages/ui/src/types/api.ts` — 14 co-changing files. Central type definition file; every API change fans out to consumers.
- `rust/leanspec-http/src/routes.rs` — 14 co-changing files. Route registration touches handlers across the codebase.
- `packages/ui/src/pages/SpecsPage.tsx` — 10 co-changing files.
- `rust/leanspec-core/src/sessions/manager.rs` — 10 co-changing files.

**Dependency cycles (2 detected):**
- 8-module cycle in `leanspec-cli/src/commands/` (init → update → ... → session) at severity 4.0
- 2-module cycle in `leanspec-http/src/` (lib.rs ↔ handlers/mod.rs) at severity 1.0

Both are real circular import chains that complicate the build graph.

**Ghost coupling highlights:**
- `acp-conversation.tsx ↔ SessionDetailPage.tsx` — 100% co-change, no structural link. Likely a component that should be imported by its consumer.
- `backend-adapter/http.ts ↔ backend-adapter/tauri.ts` — 94% co-change. Parallel implementations of the same interface; co-change is expected but missing an explicit shared type import.
- `handlers/specs.rs ↔ types.rs` — 89% co-change. Handler depends on types but import not tracked (likely via re-export).

**Hotspots** (where change frequency meets complexity):
1. `SpecsPage.tsx` — 29 changes, complexity 128
2. `commands/session.rs` — 17 changes, complexity 191
3. `sessions/runner.rs` — 17 changes, complexity 158

These overlap strongly with the GodModule signals — confirming that complex files also attract the most changes.

### 3. codervisor/synodic

**Signal breakdown**: 1 god_module, 38 stable_core

**Key findings:**
- `pipeline/executor.rs` flagged as **GodModule** (complexity 101, 520 LOC, 16 deps) — accurate, the pipeline execution engine in a single file.
- 38 **StableCore** guards. This is a young codebase (17 commits) with clean module decomposition. The high stable_core count means well-separated utilities.
- No change edges detected (co-change threshold not met with 17 commits) — expected for a young repo.

**Verdict**: Healthy architecture. One decomposition opportunity.

### 4. crawlab-team/crawlab

**Signal breakdown**: 7 god_module

**Key findings:**
- `grpc/model_service_v2_request.pb.go` — severity **125.2** (complexity 299, 1155 LOC, 136 deps). This is a generated protobuf file — **false positive**. Generated code should be excluded.
- `vcs/git.go` — severity 26.1 (complexity 191, 853 LOC, 60 deps). Real god module — git operations crammed into one file.
- `fs/seaweedfs_manager.go` — severity 10.9 (complexity 133, 642 LOC, 48 deps). Real — file system management logic.
- `core/controllers/spider_v2.go` — severity 5.7 (complexity 135, 684 LOC, 23 deps). Real — spider controller.
- `core/task/handler/runner_v2.go` — severity 4.9. Real — task runner.
- 2 more generated `.pb.go` files — false positives.

No change edges detected (crawlab had no commits within the shallow clone's 12-month window).

**Verdict**: 4 real god modules found, 3 false positives from generated protobuf code.

## Cross-Repository Analysis

### Signal Distribution

| Signal Type | ising | lean-spec | synodic | crawlab | Total |
|---|---|---|---|---|---|
| GodModule | 1 | 19 | 1 | 7 (3 FP) | 28 |
| GhostCoupling | 1 | 42 | 0 | 0 | 43 |
| StableCore | 6 | 48 | 38 | 0 | 92 |
| ShotgunSurgery | 0 | 6 | 0 | 0 | 6 |
| DependencyCycle | 0 | 2 | 0 | 0 | 2 |
| UnnecessaryAbstraction | 0 | 1 | 0 | 0 | 1 |
| UnstableDependency | 0 | 0 | 0 | 0 | 0 |
| **Total** | **8** | **118** | **39** | **7** | **172** |

### Key Observations

1. **GodModule is the highest-value new signal.** Found actionable issues in all 4 repos. The severity scoring (complexity × LOC × fan-out, normalized) produces a natural ranking that puts the worst offenders at the top.

2. **ShotgunSurgery requires active git history.** Only detected in lean-spec (367 commits). Repos with fewer commits or shallow clones produce no co-change edges, so shotgun surgery can't fire. This is working as designed — the signal genuinely requires temporal data.

3. **DependencyCycle found real cycles in lean-spec.** The 8-module cycle in the CLI commands directory is a genuine architectural concern. The 2-module cycle in HTTP lib/handlers is a common pattern in Rust (lib.rs importing mod.rs which re-exports).

4. **UnstableDependency produced zero signals.** This may indicate the threshold is too strict (requires instability gap ≥ 0.4 AND source < 0.3 AND target > 0.7). It may also reflect well-structured codebases. Needs more data points.

5. **Generated code is a false positive source for GodModule.** Protobuf `.pb.go` files in crawlab are flagged as god modules because they genuinely have high complexity/LOC/fan-out, but they're machine-generated and not actionable.

## False Positives Identified

### FP-1: Generated code triggers GodModule

**Affected repos**: crawlab (3 `.pb.go` files)

**Fix**: Add a `is_generated_code()` filter that skips:
- `*.pb.go` (protobuf Go)
- `*.pb.ts` / `*_pb.ts` (protobuf TypeScript)
- `*.generated.ts` / `*.generated.go`
- Files containing `// Code generated` header comment (Go convention)
- `*.g.dart` (Dart build_runner)

### FP-2: Small lib.rs ↔ mod.rs cycles in Rust

**Affected repos**: lean-spec

**Fix**: Consider suppressing 2-node DependencyCycle signals when both nodes are in the same crate and one is `lib.rs` or `mod.rs` (re-export barrel files). This is a standard Rust pattern, not a real cycle.

### FP-3: StableCore on repos with very few commits

**Affected repos**: synodic (38 stable_core signals from 17 commits)

**Discussion**: With only 17 commits, almost everything has "low change frequency." The stable_core signal is technically correct but not very useful — it flags 38 out of 362 nodes (~10%) which is more noise than signal. Consider requiring a minimum commit count (e.g., 50) before emitting StableCore/TickingBomb percentile-based signals.

## Recommendations

### Immediate (high confidence)

1. **Filter generated code from GodModule** — Add `is_generated_code()` check using filename patterns and file header heuristics.

2. **Suppress lib.rs ↔ mod.rs cycles** — Extend `is_reexport_module()` to also cover DependencyCycle detection for 2-node cycles.

3. **Require minimum commit history for percentile-based signals** — Add `min_commits_for_percentiles` config (default: 30). Skip StableCore/TickingBomb detection when commit count is below this threshold.

### Near-term (needs more data)

4. **Relax UnstableDependency thresholds** — Consider reducing `unstable_dep_gap` from 0.4 to 0.3, or relaxing the hard cutoffs (source < 0.3, target > 0.7) to softer percentile-based thresholds.

5. **Add ShotgunSurgery minimum change frequency filter** — A file that co-changes with 8 files but only has 2 commits total may be noise. Consider requiring `change_freq >= 5` for shotgun surgery.

6. **Cross-validate with deeper clones** — The `--depth=500` clone limit and crawlab's inactive window meant no change edges for 2 of 4 repos. Re-run with full clones on actively developed projects.

### Future consideration

7. **Signal grouping / deduplication** — In lean-spec, the ghost_coupling signals for `backend-adapter/{http,tauri,core}.ts` are 3 signals describing one issue (parallel implementations of a shared interface). Consider grouping related signals into a single finding.

## Performance Notes

All builds completed in under 5 seconds per repository, including the 4,148-node lean-spec monorepo. Build times:

| Repo | Nodes | Build time |
|---|---|---|
| ising | 301 | ~0.1s |
| lean-spec | 4,148 | ~3.0s |
| synodic | 362 | ~0.1s |
| crawlab | 3,725 | ~0.6s |
