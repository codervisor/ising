---
status: complete
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
updated_at: 2026-03-04T05:51:32.223060367Z
completed_at: 2026-03-04T05:51:32.223060367Z
transitions:
- status: in-progress
  at: 2026-03-04T05:48:36.627231839Z
- status: complete
  at: 2026-03-04T05:51:32.223060367Z
---

# CLI MVP — Local Analysis Tool

## Overview

A local `ising` CLI binary that lets users analyze a codebase directly from the command line — no Docker, no cloud, no setup friction. This replaces containerized workers (003) as the MVP delivery mechanism.

### Why CLI over Docker?

- **Zero infrastructure**: No Docker install required. Single binary, works anywhere.
- **Faster iteration**: Easier to build, test, and ship than container images.
- **Lower barrier**: Users run `ising analyze <path>` in their repo and get results immediately.
- **Docker later**: Containers can wrap the CLI later for cloud/CI use cases — the CLI becomes the foundation either way.

## Design

### Binary crate

Add `ising-cli/` as a new workspace member. Depends on `ising-core` for graph construction and spectral analysis, and `ising-scip` (002) for SCIP index loading.

### Usage

    ising analyze <path> [--format json|text] [--output <file>]
    ising --version
    ising --help

Subcommands are used so the CLI can grow (e.g., `ising diff`, `ising watch`) without breaking existing usage. `analyze` is the first and primary subcommand.

### Workflow (`analyze` subcommand)

1. Accept a path: directory (codebase root) or `.scip` file.
2. If directory: look for an existing `index.scip`, or instruct user to generate one.
3. Load SCIP index via `ising-scip` (002).
4. Build `IsingGraph` from symbols and references.
5. Run spectral analysis (λ_max, modularity Q).
6. Output report to stdout.

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

### Text output format

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

- `--format json|text` flag (default: `json`)
- `--output <file>` to write to file instead of stdout
- Exit code: 0 = stable (λ < 1), 1 = critical (λ ≥ 1) — enables CI gating

## Plan

- [x] Create `ising-cli/` binary crate with clap subcommand scaffolding
- [x] Implement `analyze` subcommand (SCIP path → JSON/text output)
- [x] Wire up `ising-core` graph + physics pipeline
- [x] Add `--format` and `--output` flags
- [x] CI exit code based on λ_max threshold
- [x] Integration test: analyze a sample `.scip` index

## Test

- [x] `ising analyze index.scip` produces valid JSON with correct schema
- [x] `ising analyze index.scip --format text` produces readable text output
- [x] Exit code 0 when λ_max < 1, exit code 1 when λ_max ≥ 1
- [x] `ising analyze --output report.json index.scip` writes to file
- [x] `--help` and `--version` work correctly
- [x] `ising analyze --help` shows subcommand-specific help

## Notes

- This spec supersedes 003-container-workers as the MVP delivery path. Docker can later wrap this CLI for cloud/CI use.
- Depends on 002-scip-loader for index parsing and 001-rust-core for the analysis engine.

- Implemented as new `ising-cli` workspace member with binary name `ising`.
- `analyze` accepts directory or `.scip` file input; directories require `index.scip`.
- Added JSON/text output formatting, `--output` file writing, and CI-friendly exit codes (`0` stable, `1` critical).
- Added CLI tests that generate temporary `.scip` fixtures and verify schema, text rendering, output writing, help/version, and exit code behavior.