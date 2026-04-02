# Spec 043: Temporal Snapshot Analysis (Git History Video)

**Status**: Draft — future phase
**Created**: 2026-04-02
**Depends on**: Spec 034 (structural force analysis), λ_max implementation in `ising-core/src/metrics.rs`

## Motivation

Static analysis gives a **photograph** — the structural dependency graph at HEAD.
But codebases evolve. A single snapshot cannot answer:

- Is coupling growing or shrinking?
- Was this module always risky, or did it degrade recently?
- Are refactoring efforts actually improving structure?

Tracking structural metrics across git history gives a **video** — a time series
of snapshots that captures the rate and direction of change.

## Core Analogy

| Concept | Static (Camera) | Temporal (Video) |
|---------|-----------------|------------------|
| Input | HEAD revision | Sequence of revisions |
| Output | λ_max, zone fractions, health grade | Trajectory of λ_max, zone fractions over time |
| Answers | "How coupled is this codebase?" | "Is coupling getting worse?" |
| Cost | One parse pass | N parse passes |

## Design

### Frame Selection

Each frame is a full structural graph built at a specific git revision.

**Strategies** (pick one per analysis):

1. **Fixed time windows** — e.g., weekly snapshots for the past year (52 frames).
   - Pro: uniform spacing, easy to reason about.
   - Con: misses burst activity between frames.

2. **Fixed commit count** — e.g., every 100 commits.
   - Pro: captures periods of high activity with more frames.
   - Con: uneven time spacing; merge commits blur boundaries.

3. **Tag/release-based** — one frame per release tag.
   - Pro: aligns with meaningful project milestones.
   - Con: irregular spacing; some repos don't tag.

4. **Adaptive** — combine time windows with a minimum commit threshold.
   - E.g., weekly frames, but skip weeks with <5 commits.

Recommended default: **weekly snapshots, last 52 weeks**, with adaptive skip for
low-activity weeks. This balances cost and coverage.

### Per-Frame Metrics

At each frame (git revision), compute:

| Metric | Source | Purpose |
|--------|--------|---------|
| `lambda_max` | Structural Import graph, unit weights | Coupling regime (< 1.0 = modular, ≥ 1.0 = coupled) |
| `eigenvector_centrality` | Top-k nodes by centrality | Which modules drive coupling |
| `zone_fractions` | Safety factor zones (Critical/Danger/Warning/Healthy/Stable) | Risk distribution |
| `module_count` | Graph size | Growth rate |
| `edge_count` | Import edges | Dependency density |
| `health_grade` | Health index | Overall assessment |

### Derived Trajectory Metrics

From the time series of per-frame metrics:

| Metric | Formula | Interpretation |
|--------|---------|----------------|
| `lambda_trend` | Linear regression slope of λ_max over time | Positive = coupling growing |
| `lambda_velocity` | Δλ_max / Δtime (recent window) | Rate of coupling change |
| `zone_migration` | Change in zone fractions between first and last frame | Net movement between zones |
| `structural_momentum` | Weighted moving average of λ_trend | Smoothed direction indicator |
| `coupling_acceleration` | Second derivative of λ_max | Is growth accelerating? |

### Co-Change as a Derived Metric

Instead of building a co-change adjacency matrix (which requires arbitrary window
parameters and decays quickly), co-change coupling **emerges from the video**:

- Two modules that appear together in changed-file lists across multiple frames
  exhibit co-change coupling.
- The structural diff between consecutive frames shows which edges were added/removed.
- Modules that repeatedly gain new edges to the same neighbors are structurally
  co-evolving.

This avoids the "measuring a growing tree" problem — we don't try to pin down
co-change at a single point, we observe it as a pattern across frames.

## Implementation Plan

### Phase 1: Frame Builder

```
ising-analysis/src/temporal.rs (new)
```

- `build_frame(repo_path, revision) -> Frame`
  - `git checkout <revision>` (or `git worktree add`)
  - Run structural parser → `UnifiedGraph`
  - Compute `SpectralMetrics` + zone fractions
  - Return `Frame { revision, timestamp, metrics }`

- `build_timeline(repo_path, strategy) -> Timeline`
  - Enumerate revisions per strategy
  - Build frames (parallelizable with worktrees)
  - Return sorted `Vec<Frame>`

### Phase 2: Trajectory Analysis

- `compute_trajectory(timeline) -> TrajectoryMetrics`
  - Linear regression on λ_max series
  - Zone migration calculation
  - Identify inflection points (coupling started growing/shrinking)

### Phase 3: Storage & Display

- Store frames in SQLite (`frames` table with revision, timestamp, metrics JSON)
- CLI command: `ising timeline [--weeks 52] [--strategy weekly]`
- ASCII sparkline of λ_max trajectory
- Highlight inflection points and trend direction

### Phase 4: Incremental Updates

- Cache parsed frames by commit hash (content-addressable)
- On new analysis, only build frames for new revisions
- Incremental parsing: diff changed files, re-parse only those, merge into cached graph

## Cost Analysis

For a medium repo (~500 files):
- One frame: ~2-5 seconds (parse + spectral computation)
- 52 frames: ~2-4 minutes
- With caching: subsequent runs only build new frames

For a large repo (~5000 files):
- One frame: ~20-60 seconds
- 52 frames: ~15-50 minutes (first run)
- Parallelization with git worktrees can reduce wall time significantly

## Open Questions

1. **Worktree vs checkout**: `git worktree add` allows parallel frame building
   but creates disk pressure. `git checkout` is serial but lighter.

2. **Deleted files**: A module that existed in frame N but not N+1 was deleted.
   How does this affect trajectory metrics? Probably exclude from per-module
   tracking, but count as structural change.

3. **Branch strategy**: Do we follow `main` only, or allow branch comparison?
   ("How did this feature branch change coupling?")

4. **Ground truth validation**: Without defect data, we can't prove that a
   worsening λ_max trajectory predicts problems. We can validate that known
   "we refactored this" events show improving trajectories.

5. **Merge commits**: Should frames include merge commits or skip to first-parent?
   Merge commits may show temporary coupling from feature branches.

## Non-Goals (This Spec)

- Real-time monitoring / CI integration (future)
- Cross-repo comparison of trajectories (future)
- Automated "refactoring recommendation" from trajectory analysis (future)
- Co-change matrix construction (replaced by frame-differencing approach)
