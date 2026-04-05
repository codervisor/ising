# Ising

## Project Context

**Ising** is a code maintainability analysis engine. It builds a multi-layer dependency graph from source code and git history, computes per-module risk scores and safety factors, detects cross-layer anomalies (signals), and serves results via CLI and HTTP/MCP server.

### What It Does

1. **Builds a graph** from source code (Tree-sitter) and git history (gix) with three layers: structural, change, defect
2. **Computes risk** for every module: how much change pressure it faces vs. how much it can absorb
3. **Detects 16 signal types** like ghost coupling, dependency cycles, god modules, ticking bombs, shotgun surgery
4. **Simulates changes** to predict blast radius before code is written
5. **Computes health index** -- aggregate repository grade (A--F) from zone distribution, coupling, and signal penalties
6. **Serves results** to AI coding agents via MCP tools

### Key Concepts

| Concept | Meaning |
|---|---|
| **Safety Factor (SF)** | `capacity / risk_score`. The primary health metric. SF < 1.0 = critical. |
| **Capacity** | Module resilience: `1.0 - (complexity*0.4 + instability*0.3 + coupling*0.3)`. Range [0.05, 1.0]. |
| **Change Load** | `normalize(defect_churn*3 + feature_churn)`, falls back to `change_freq * churn_rate`. |
| **Risk Score** | Change load + propagated risk from neighbors via Jacobi iteration. |
| **Direct Score** | `change_load / capacity` -- local risk without propagation, used for tier ranking. |
| **Safety Zone** | Absolute classification: Critical (<1.0), Danger (1.0--1.5), Warning (1.5--2.0), Healthy (2.0--3.0), Stable (>3.0). |
| **Risk Tier** | Relative percentile ranking: Critical (top 1%), High (top 5%), Medium (top 15%), Normal (rest). |
| **Health Index** | Aggregate grade: `zone_sub_score × coupling_modifier × signal_factor`. A>=0.85, B>=0.70, C>=0.55, D>=0.40. |
| **Signal** | Cross-layer anomaly (e.g., ghost coupling = files co-change without structural dependency). |

### Architecture

```
ising-core/       Types, config, graph model, metrics
ising-builders/   Graph construction (Tree-sitter + git)
ising-analysis/   Risk computation, signals, hotspots, health index
ising-db/         SQLite persistence and queries
ising-cli/        CLI: build, safety, health, simulate, impact, signals, hotspots, serve
ising-server/     HTTP/MCP server for AI agent integration
ising-scip/       SCIP index loader (alternative to Tree-sitter)
```

### CLI Commands

```bash
ising build --repo-path <path>        # Build graph + compute risk
ising safety --top 20                 # View riskiest modules
ising safety --zone critical          # Filter by zone
ising health                          # Aggregate repository health grade
ising simulate <file>                 # Predict blast radius of a change
ising impact <file>                   # Dependencies, risk signals for a file
ising signals                         # View cross-layer anomalies
ising hotspots --top 20               # View change hotspots
ising stats                           # Global graph statistics
ising boundaries                      # Detected module boundaries
ising export --format json            # Export graph (json, dot, mermaid, viz-json)
ising serve --port 3000               # Start MCP server
```

## Skills

This project uses [forge](https://github.com/codervisor/forge) skills:

| Skill | Description |
|-------|-------------|
| `leanspec-sdd` | Spec-Driven Development -- plan before you code |
| `rust-npm-publish` | Distribute Rust binaries via npm platform packages |
| `hybrid-ci` | CI/CD for Rust+Node.js with GitHub Actions |
| `monorepo-version-sync` | Coordinated versioning across packages and languages |

## Conventions

- **Version source of truth**: Root `package.json`
- **Workspace protocol**: Use `workspace:*` for internal deps during development
- **Specs first**: Create a spec before starting non-trivial work
- **CI must pass**: All PRs require passing CI
- **Testing**: `cargo test --workspace` must pass. Unit tests alongside modules.
- **Formatting**: `cargo fmt` and `cargo clippy` must be clean.

## Implementation Guidelines

- **Maintainability is our product** -- our own code must exemplify what we preach
- **Portable builds** -- pure Rust preferred, avoid external C library dependencies
- **Workspace conventions** -- shared deps in root `Cargo.toml` via `[workspace.dependencies]`

## Spec Management

Use `lean-spec` CLI or MCP tools for spec management:

| Action | Command |
|--------|---------|
| Project status | `lean-spec board` |
| List specs | `lean-spec list` |
| Search specs | `lean-spec search "query"` |
| View spec | `lean-spec view <spec>` |
| Create spec | `lean-spec create <name>` |
| Update spec | `lean-spec update <spec> --status <status>` |
| Validate | `lean-spec validate` |

### Rules

- Never edit spec frontmatter manually -- use `update`, `link`, `unlink` tools
- Track status transitions: `planned` -> `in-progress` -> `complete`
- Keep specs current and in sync with implementation
- No nested code blocks in specs (use indentation instead)
