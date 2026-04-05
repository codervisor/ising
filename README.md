# Ising

**Code maintainability analysis engine.** Ising builds a multi-layer graph of your codebase and computes risk scores, safety factors, and actionable signals to help teams find where technical debt is accumulating before it becomes a crisis.

## What It Does

Ising analyzes a codebase from three angles and combines them:

| Layer | Source | What It Captures |
|-------|--------|------------------|
| **Structural** | Source code (Tree-sitter) | Imports, complexity, coupling between modules |
| **Change** | Git history | Which files change together, churn rate, hotspots |
| **Defect** | Git blame + commit messages | Bug-prone files, fault propagation patterns |

From these layers, Ising computes:

- **Risk scores** — How much change pressure each module faces
- **Capacity** — How much change a module can absorb (inverse of complexity + instability + coupling)
- **Safety factors** — `capacity / risk_score` — the single number that tells you if a module is healthy or about to break
- **Health index** — An aggregate grade (A--F) for the entire repository
- **Cross-layer signals** — Anomalies like ghost coupling (files that always change together but have no code dependency)

## Quick Start

```bash
# Build analysis for a repository
ising build --repo-path /path/to/repo

# View the riskiest modules
ising safety --top 20

# Overall repository health grade
ising health

# Simulate: "what if I change this file?"
ising simulate routing.py

# Impact analysis for a specific file
ising impact src/main.rs

# Filter by zone
ising safety --zone critical

# View cross-layer signals
ising signals

# Top hotspots
ising hotspots --top 20
```

## Safety Zones and Risk Tiers

Every module gets two classifications: a **safety zone** (absolute thresholds) and a **risk tier** (relative percentile ranking).

### Safety Zones

Based on the safety factor (`capacity / risk_score`):

| Zone | Safety Factor | Meaning |
|------|--------------|---------|
| **CRITICAL** | < 1.0 | Risk exceeds capacity. This module is overloaded. |
| **DANGER** | 1.0 -- 1.5 | Thin margin. The next change may push it over. |
| **WARNING** | 1.5 -- 2.0 | Caution. Monitor closely. |
| **HEALTHY** | 2.0 -- 3.0 | Good margin. Well-maintained. |
| **STABLE** | > 3.0 | Low risk, high capacity. Not a concern. |

### Risk Tiers

Auto-calibrated percentile ranking based on `direct_score` (local risk without propagation):

| Tier | Percentile | Meaning |
|------|-----------|---------|
| **Critical** | Top 1% | Immediate attention needed |
| **High** | Top 1--5% | Elevated risk |
| **Medium** | Top 5--15% | Moderate risk |
| **Normal** | Bottom 85% | Normal |

Risk tiers are self-calibrating -- they rank modules relative to each other within the same codebase. Safety zones use fixed thresholds and are comparable across repositories.

## Risk Model

### Per-module computation

```
change_load     = normalize(defect_churn*3 + feature_churn)   -- defect-weighted change pressure
                  (falls back to change_freq * churn_rate when no commit classification data)
capacity        = 1.0 - (complexity*0.4 + instability*0.3 + coupling*0.3)  -- floor at 0.05
propagated_risk = Jacobi iteration over co-change + import edges
risk_score      = change_load + propagated_risk
safety_factor   = capacity / risk_score                        -- clamped to [0, 10]
direct_score    = change_load / capacity                       -- local risk, no propagation
```

### Capacity

Measures how much change a module can absorb:

- **Complexity burden** (40%) — cyclomatic complexity normalized against the graph maximum
- **Instability** (30%) — `fan_out / (fan_in + fan_out)` (Robert C. Martin's metric)
- **Coupling burden** (30%) — coupling between objects (CBO) normalized against graph maximum
- Minimum capacity floor: 0.05

### Risk propagation

Risk flows through the dependency graph via two edge types:
- **Co-change edges** (damping 0.3) -- files that historically change together
- **Import edges** (damping 0.15) -- structural dependencies, bidirectional
- **Boundary attenuation** (0.3) -- edges crossing detected boundaries are dampened

Propagation uses Jacobi iteration with per-node weight normalization (capped at 0.95 spectral radius) to guarantee convergence. Default convergence threshold: epsilon 0.001, max 100 iterations.

### Load case simulation

`ising simulate <file>` answers: "If I change this file, what happens to the rest of the codebase?"

It applies a 2x pressure multiplier to the target file and 1.5x to its co-change neighbors, then recomputes the full risk field and shows the delta.

## Health Index

Aggregate repository health grade (A--F) computed from the full risk field. The score is fully multiplicative:

```
score = zone_sub_score × coupling_modifier × signal_factor
        (× containment_modifier when boundary health is available)
```

Clamped to [0, 1]. Grade thresholds: A >= 0.85, B >= 0.70, C >= 0.55, D >= 0.40, F < 0.40.

### Components

| Component | What It Measures | Range |
|-----------|-----------------|-------|
| **Zone sub-score** | Weighted average of safety zone fractions (Stable=1.0, Healthy=0.90, Warning=0.65, Danger=0.35, Critical=0.15). Blends toward a 0.75 prior for small codebases (<50 active modules). | [0, 1] |
| **Coupling modifier** | Spectral radius of the import graph normalized by sqrt(N). Loosely coupled (<1.0 normalized) gets a slight bonus; tightly coupled (>1.0) gets a gentle penalty. | [0.85, 1.05] |
| **Signal factor** | Penalty from detected signals using an adaptive piecewise curve. Weighted signal score accounts for signal severity (cycles=4x, god modules=3x, etc.) normalized by sqrt(N). | [0.70, 1.0] |
| **Containment modifier** | Boundary health: `0.70 + 0.35 * avg_containment`. Only applied when boundaries are detected. | [0.70, 1.05] |
| **Tail risk cap** | If any non-test module's expected loss exceeds 5.0, score is capped at 0.84 (B ceiling). Prevents a single extreme outlier from hiding behind a good average. | ceiling at 0.84 |

## Cross-Layer Signals

Ising detects 16 signal types across four priority levels:

### Critical

| Signal | What It Means |
|--------|--------------|
| **DependencyCycle** | Circular import chain detected via Tarjan's algorithm. Increases coupling burden. |
| **FragileBoundary** | Structural dependency + high co-change + fault propagation across a boundary. |
| **TickingBomb** | Module is simultaneously a hotspot, defect-dense, and highly coupled. |

### High

| Signal | What It Means |
|--------|--------------|
| **GhostCoupling** | Two files always change together but have no code dependency. Hidden coupling. |
| **GodModule** | Extreme complexity (>=50) + high LOC (>=500) + high coupling (CBO>=15), or monolith (LOC>=5000, complexity>=200). |
| **ShotgunSurgery** | A file's changes scatter across 8+ other files. |
| **UnstableDependency** | A stable module (instability<0.3) depends on a volatile one (instability>0.7). |
| **SystemicComplexity** | Median or P75 complexity elevated across the entire codebase (>=50 modules). |
| **IntraFileHotspot** | A function churns far more than its siblings in the same file. |
| **BoundaryLeakage** | Module has >30% of change edges crossing a boundary despite low structural coupling. |
| **DeprecatedUsage** | A deprecated symbol is still being called or imported. |

### Guard

| Signal | What It Means |
|--------|--------------|
| **StableCore** | A heavily-depended-upon module that rarely changes. Not a failure condition -- a guard signal to preserve architectural integrity around critical shared code. |

### Informational

| Signal | What It Means |
|--------|--------------|
| **UnnecessaryAbstraction** | A structural dependency exists but the files never co-change. May be dead indirection. |
| **OrphanFunction** | A function with zero callers (excluding entry points like main, test, init). |
| **OrphanModule** | A module with zero importers (excluding entry points and generated code). |
| **StaleCode** | Code unchanged for an extended period with low connectivity. |

## CLI Commands

| Command | Purpose |
|---------|---------|
| `ising build` | Parse source code, analyze git history, compute risk, detect signals |
| `ising safety` | Safety factor analysis with zone filtering (`--top N`, `--zone critical`) |
| `ising health` | Aggregate repository health index (grade A--F) |
| `ising simulate <file>` | Load case simulation — predict blast radius of a change |
| `ising impact <file>` | Blast radius, dependencies, and risk signals for a file |
| `ising signals` | Cross-layer signals with type filtering and severity thresholds |
| `ising hotspots` | Top hotspots ranked by change frequency * complexity |
| `ising stats` | Global graph statistics |
| `ising boundaries` | Detected module boundaries (packages + modules) |
| `ising export --format <json\|dot\|mermaid\|viz-json>` | Graph export in the selected format |
| `ising serve` | Start MCP server (default port 3000) |

Most commands support `--format json` for machine-readable output.

## Architecture

Rust monorepo with 7 crates:

```
ising-core/       Core types: graph, config, risk types, metrics
ising-builders/   Graph construction from source code (Tree-sitter) and git history
ising-analysis/   Risk computation, signal detection, hotspot ranking
ising-db/         SQLite persistence and queries
ising-cli/        Command-line interface
ising-server/     HTTP/MCP server for AI agent integration
ising-scip/       SCIP index loader (alternative to Tree-sitter for supported languages)
```

## MCP Server (AI Agent Integration)

```bash
ising serve --port 3000
```

Exposes tools for AI coding agents:

| Endpoint | Purpose |
|----------|---------|
| `GET /tools` | List available MCP tools |
| `GET /safety?top=20` | Ranked list of riskiest modules |
| `GET /safety?zone=critical` | All modules in a specific zone |
| `GET /simulate?target=path/to/file.py` | Blast radius simulation |
| `GET /impact?target=path/to/file.py` | Impact analysis for a file |
| `GET /signals` | Active cross-layer signals |
| `GET /signals?type=ghost_coupling&min_severity=0.5` | Filtered signals |
| `GET /health` | Repository health index |

## Configuration

Create an `ising.toml` in the repository root:

```toml
[general]
time_window = "6 months ago"   # Git history window
max_commits = 5000             # Max commits to analyze
max_files_per_commit = 50      # Skip large commits (refactors, renames)

[thresholds]
min_co_changes = 3                 # Minimum co-change count for coupling edges
min_coupling = 0.15                # Minimum coupling score for edges
ghost_coupling_threshold = 0.5     # Co-change threshold for ghost coupling detection
god_module_complexity = 50         # Cyclomatic complexity threshold for god module
god_module_loc = 500               # LOC threshold for god module
god_module_fan_out = 15            # CBO threshold for god module
shotgun_surgery_breadth = 8        # Minimum scattered files for shotgun surgery
unstable_dep_gap = 0.4             # Min instability gap for unstable dependency

[fea]
cochange_damping = 0.3         # Risk propagation via co-change edges
structural_damping = 0.15      # Risk propagation via import edges
epsilon = 0.001                # Convergence threshold
max_iterations = 100           # Max propagation iterations
boundary_attenuation = 0.3     # Damping for edges crossing boundaries
```

## Building

```bash
cargo build                    # Debug build
cargo build --release          # Release build
cargo test --workspace         # Run all tests
cargo clippy --workspace       # Lint check
cargo fmt --check              # Format check
```

## Supported Languages

Python, TypeScript, JavaScript, Rust, Go, Java, C#, C/C++, Kotlin, Ruby, PHP, Vue.

## License

MIT
