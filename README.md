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
- **Cross-layer signals** — Anomalies like ghost coupling (files that always change together but have no code dependency)

## Quick Start

```bash
# Build analysis for a repository
ising build --repo-path /path/to/repo

# View the riskiest modules
ising safety --top 20

# Simulate: "what if I change this file?"
ising simulate routing.py

# Filter by zone
ising safety --zone critical
```

## Safety Zones

Every module gets classified into a zone based on its safety factor:

| Zone | Safety Factor | Meaning |
|------|--------------|---------|
| **CRITICAL** | < 1.0 | Risk exceeds capacity. This module is overloaded. |
| **DANGER** | 1.0 -- 1.5 | Thin margin. The next change may push it over. |
| **WARNING** | 1.5 -- 2.0 | Caution. Monitor closely. |
| **HEALTHY** | 2.0 -- 3.0 | Good margin. Well-maintained. |
| **STABLE** | > 3.0 | Low risk, high capacity. Not a concern. |

## Risk Model

The risk model is direct and honest -- no physics theater, no fake stress tensors.

### Per-module computation

```
change_load     = normalize(change_freq * churn_rate)     -- how much change pressure
capacity        = 1.0 - (complexity*0.4 + instability*0.3 + coupling*0.3)  -- how much it can absorb
propagated_risk = sum of neighbor risks via import and co-change edges
risk_score      = change_load + propagated_risk
safety_factor   = capacity / risk_score
```

### Risk propagation

Risk flows through the dependency graph via two edge types:
- **Co-change edges** (damping 0.3) -- files that historically change together
- **Import edges** (damping 0.15) -- structural dependencies, bidirectional

Propagation uses Jacobi iteration with per-node weight normalization (capped at 0.95 spectral radius) to guarantee convergence.

### Load case simulation

`ising simulate <file>` answers: "If I change this file, what happens to the rest of the codebase?"

It applies a 2x pressure multiplier to the target file and 1.5x to its co-change neighbors, then recomputes the full risk field and shows the delta.

## Cross-Layer Signals

| Signal | What It Means |
|--------|--------------|
| **GhostCoupling** | Two files always change together but have no code dependency. Hidden coupling. |
| **DependencyCycle** | Circular import chain. Increases coupling burden. |
| **GodModule** | One file with extremely high complexity and fan-out. |
| **UnstableDependency** | A stable module depends on a volatile one. |
| **StableCore** | A heavily-depended-upon module that rarely changes. Protect it. |

## Architecture

Rust monorepo with 6 crates:

```
ising-core/       Core types: graph, config, risk types, metrics
ising-builders/   Graph construction from source code (Tree-sitter) and git history
ising-analysis/   Risk computation, signal detection, hotspot ranking
ising-db/         SQLite persistence and queries
ising-cli/        Command-line interface
ising-server/     HTTP/MCP server for AI agent integration
```

## MCP Server (AI Agent Integration)

```bash
ising serve --port 8080
```

Exposes tools for AI coding agents:

| Endpoint | Purpose |
|----------|---------|
| `GET /safety?top=20` | Ranked list of riskiest modules |
| `GET /safety?zone=critical` | All modules in a specific zone |
| `GET /simulate?target=path/to/file.py` | Blast radius simulation |
| `GET /signals` | Active cross-layer signals |
| `GET /hotspots?top=20` | Top hotspots by change frequency * complexity |

## Configuration

Create an `ising.toml` in the repository root:

```toml
[general]
time_window = "6 months ago"   # Git history window
max_commits = 5000             # Max commits to analyze
max_files_per_commit = 50      # Skip large commits (refactors, renames)

[change]
min_co_changes = 5             # Minimum co-change count for coupling edges
min_coupling = 0.3             # Minimum coupling score for edges

[fea]
cochange_damping = 0.3         # Risk propagation via co-change edges
structural_damping = 0.15      # Risk propagation via import edges
epsilon = 0.001                # Convergence threshold
max_iterations = 100           # Max propagation iterations
```

## Building

```bash
cargo build                    # Debug build
cargo build --release          # Release build
cargo test --workspace         # Run all tests
```

## Supported Languages

Python, TypeScript, JavaScript, Rust, Go, Java, C#, C/C++, Kotlin, Swift, Ruby, PHP, Vue.

## License

MIT
