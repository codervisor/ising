# Changelog

## [Unreleased]

- Renamed `OverEngineering` signal to `UnnecessaryAbstraction` for clarity — the old name was confused with the `SafetyZone::Stable` rename

## [0.1.0] - 2026-03-27

Initial release of `@codervisor/ising-cli`.

### Features

**Three-layer code graph analysis**
- Structural layer: tree-sitter AST extraction of functions, classes, and imports
- Change layer: git history analysis for co-change frequency, churn rate, and hotspot scoring
- Defect layer: fault propagation tracking correlated with git log keywords

**CLI commands**
- `ising build` — parse a repo and build the full graph into a local SQLite database
- `ising hotspots` — rank files by change frequency × complexity
- `ising signals` — show detected cross-layer structural signals
- `ising impact <target>` — blast radius, dependencies, and risk signals for a file
- `ising stats` — global graph statistics
- `ising export` — export the graph as JSON, DOT, Mermaid, or VizJSON
- `ising serve` — start an MCP server for AI agent integration

**Cross-layer signals**
- `GhostCoupling` — hidden dependency: high temporal coupling with no structural link
- `FragileBoundary` — broken interface: structural dep + high co-change + fault propagation
- `UnnecessaryAbstraction` — structural dep with no co-change evidence
- `StableCore` — reliable foundation: high fan-in, low churn, low defects
- `TickingBomb` — most dangerous code: high hotspot + high defects + high coupling
- `DependencyCycle` — circular dependency between modules
- `GodModule` — does too much: extreme complexity, LOC, and fan-out
- `ShotgunSurgery` — scattered responsibility: one file's changes ripple across many
- `UnstableDependency` — Stable Dependencies Principle violation

**Language support**
- TypeScript and JavaScript (`.ts`, `.tsx`, `.js`, `.jsx`) via tree-sitter
- Python (`.py`) via tree-sitter
- Rust (`.rs`) via tree-sitter
- Go (`.go`) via tree-sitter
- Vue SFCs (`.vue`) via tree-sitter

**Platform binaries**
- macOS (x64 and arm64)
- Linux (x64)
- Windows (x64)
