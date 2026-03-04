---
status: planned
created: 2026-03-04
priority: high
tags:
- e2e
- testing
- scip
- indexing
depends_on:
- 001-rust-core
- 002-scip-loader
- 006-cli-mvp
created_at: 2026-03-04T07:39:45.295619945Z
updated_at: 2026-03-04T07:39:45.295619945Z
---

# End-to-End Testing — SCIP Index Generation & Analysis

## Overview

Validate the full Ising pipeline end-to-end: generate a real SCIP index from source code, feed it to `ising analyze`, and verify meaningful output. This also documents how to generate SCIP indexes across languages so users know the prerequisites.

## Research Findings

### SCIP Indexers by Language

| Language | Indexer | Install | Generate |
|---|---|---|---|
| Rust | `rust-analyzer` (built-in) | `rustup component add rust-analyzer` | `rust-analyzer scip .` |
| TypeScript | `scip-typescript` | `npm i -g @sourcegraph/scip-typescript` | `scip-typescript index` |
| JavaScript | `scip-typescript` | `npm i -g @sourcegraph/scip-typescript` | `scip-typescript index --infer-tsconfig` |
| Python | `scip-python` | `npm i -g @sourcegraph/scip-python` | `scip-python index . --project-name=NAME` |
| Java/Scala/Kotlin | `scip-java` | See sourcegraph/scip-java releases | `scip-java index` |
| C/C++ | `scip-clang` | See sourcegraph/scip-clang releases | `scip-clang` |
| Ruby | `scip-ruby` | See sourcegraph/scip-ruby releases | `scip-ruby` |
| C#/VB | `scip-dotnet` | See sourcegraph/scip-dotnet releases | `scip-dotnet index` |

All indexers output `index.scip` in the project root by default.

### Self-analysis Results (Ising on itself)

Generated `index.scip` (165KB) for the Ising workspace using `rust-analyzer scip .` in ~10s:

    {
      "version": "0.1.0",
      "path": ".",
      "health": {
        "lambda_max": 2.247,
        "status": "critical",
        "modularity_q": ~0.0
      },
      "summary": {
        "symbols": 211,
        "dependencies": 769
      }
    }

- **lambda_max = 2.247**: Above 1.0 threshold → "critical" status. Expected for a project where modules are still tightly coupled (core + scip + cli sharing types).
- **modularity_q ≈ 0.0**: The Louvain algorithm finds effectively no community structure beyond trivial partitions. This makes sense for a small codebase with 3 crates that heavily cross-reference.
- **211 symbols, 769 dependencies**: Reasonable for ~600 lines of Rust across 3 crates.

### Observations

1. `rust-analyzer scip` emitted one non-fatal bug: `definition at ising-core/src/graph/mod.rs:21:0-21:38 should have been in an SCIP document but was not`. This is a known rust-analyzer edge case — doesn't affect results.
2. The CLI exits with code 1 on "critical" status. This is useful for CI gating but should be documented.
3. Python indexer requires Python 3.10+ and Node v16+.
4. TypeScript indexer requires a `tsconfig.json` (or `--infer-tsconfig` for JS).
5. For pnpm workspaces: `scip-typescript index --pnpm-workspaces`.

## Plan

- [ ] Add a `tests/` or `e2e/` directory with a minimal Rust sample project for automated testing
- [ ] Create an integration test that generates SCIP index → runs `ising analyze` → validates JSON output schema
- [ ] Document SCIP index generation in a user-facing README section
- [ ] Investigate the exit code 1 on "critical" — decide if exit code should reflect health status or just success/failure
- [ ] Test with a TypeScript project (npm install → scip-typescript index → ising analyze)
- [ ] Test with a Python project (pip install → scip-python index → ising analyze)

## Test

- [ ] `rust-analyzer scip .` generates valid `index.scip` with non-zero size
- [ ] `ising analyze .` produces valid JSON with correct schema fields (version, path, health, summary)
- [ ] health.lambda_max is a positive number
- [ ] health.status is "stable" or "critical"
- [ ] summary.symbols > 0 and summary.dependencies > 0
- [ ] TypeScript e2e: small TS project → index.scip → valid report
- [ ] Python e2e: small Python project → index.scip → valid report