---
status: planned
created: 2026-03-04
priority: high
tags:
- phase-3
- mvp
- cli
depends_on:
- 001-rust-core
- 002-scip-loader
created_at: 2026-03-04T03:16:09.165977733Z
updated_at: 2026-03-04T03:16:20.010069063Z
---

# CLI MVP — Local Analysis Tool

## Overview

A local `ising` CLI binary that lets users analyze a codebase directly from the command line — no Docker, no cloud, no setup friction. This replaces containerized workers (003) as the MVP delivery mechanism.

### Why CLI over Docker?

- **Zero infrastructure**: No Docker install required. Single binary, works anywhere.
- **Faster iteration**: Easier to build, test, and ship than container images.
- **Lower barrier**: Users run `ising analyze .` in their repo and get results immediately.
- **Docker later**: Containers can wrap the CLI later for cloud/CI use cases — the CLI becomes the foundation either way.

## Design

### Binary crate

Add `ising-cli/` as a new workspace member. Depends on `ising-core` for graph construction and spectral analysis, and `ising-scip` (002) for SCIP index loading.

### Subcommands

    ising analyze <path>        # Analyze a codebase or .scip index
    ising report <path>         # Generate a human-readable health report
    ising --version             # Show version
    ising --help                # Show help

### `analyze` workflow

1. Accept a path: directory (codebase root) or `.scip` file.
2. If directory: look for an existing `index.scip`, or instruct user to generate one.
3. Load SCIP index via `ising-scip` (002).
4. Build `IsingGraph` from symbols and references.
5. Run spectral analysis (λ_max, modularity Q).
6. Output JSON report to stdout.

### Output format

    {
      "version": "0.1.0",
      "path": "/path/to/repo",
      "health": {
        "lambda_max": 1.42,
        "status": "critical",
        "modularity_q": 0.35
      },
      "summary": {
        "symbols": 1200,
        "dependencies": 3400
      }
    }

### `report` subcommand

Same pipeline as `analyze`, but outputs a human-readable summary:

    Ising Health Report
    ═══════════════════
    Repository:  /path/to/repo
    Symbols:     1,200
    Dependencies: 3,400

    λ_max:       1.42  ⚠ CRITICAL (> 1.0)
    Modularity:  0.35  ○ MODERATE

    Top coupling hotspots:
      1. src/engine.rs  (degree: 42)
      2. src/api/mod.rs (degree: 31)

### CLI framework

Use `clap` (derive API) for argument parsing — idiomatic Rust, minimal deps.

### Output options

- `--format json|text` flag (default: `json` for `analyze`, `text` for `report`)
- `--output <file>` to write to file instead of stdout
- Exit code: 0 = stable (λ < 1), 1 = critical (λ ≥ 1) — enables CI gating

## Plan

- [ ] Create `ising-cli/` binary crate with clap scaffolding
- [ ] Implement `analyze` subcommand (SCIP path → JSON output)
- [ ] Implement `report` subcommand (human-readable output)
- [ ] Wire up `ising-core` graph + physics pipeline
- [ ] Add `--format` and `--output` flags
- [ ] CI exit code based on λ_max threshold
- [ ] Integration test: analyze a sample `.scip` index

## Test

- [ ] `ising analyze index.scip` produces valid JSON with correct schema
- [ ] `ising report index.scip` produces readable text output
- [ ] Exit code 0 when λ_max < 1, exit code 1 when λ_max ≥ 1
- [ ] `--format json` works on `report`, `--format text` works on `analyze`
- [ ] `--output report.json` writes to file
- [ ] `--help` and `--version` work correctly

## Notes

- This spec supersedes 003-container-workers as the MVP delivery path. Docker can later wrap this CLI for cloud/CI use.
- Depends on 002-scip-loader for index parsing and 001-rust-core for the analysis engine.
