---
status: planned
created: 2026-03-04
priority: low
tags:
- phase-3
- cloud
depends_on:
- 002-scip-loader
- 005-lsp-vs-scip-research
created_at: 2026-03-04T01:48:38.439898044Z
updated_at: 2026-03-04T03:15:26.605458427Z
---

# Containerized Workers — Cloud Analysis

## Overview

Containerized workers that encapsulate language-specific SCIP indexers (e.g., `scip-python`, `scip-typescript`) to provide a zero-configuration cloud experience. Users point at a git repo and get an Ising analysis without installing anything locally.

## Design

### Container architecture

- **Base image**: Debian slim + Rust binary (`ising-cli`) for analysis.
- **Language images**: Extend base with language runtime + SCIP indexer.
  - `ising-worker-python`: Python 3.x + `scip-python`
  - `ising-worker-typescript`: Node.js + `scip-typescript`
- **Multi-stage builds**: Build Rust binary in builder stage, copy to slim runtime.

### Workflow (entrypoint)

1. `git clone --depth 1 <repo_url>` into working directory.
2. Auto-detect primary language(s) from file extensions / config files.
3. Run SCIP indexer: `scip-<lang> index --output index.scip`.
4. Run `ising-cli analyze index.scip` → produce JSON report.
5. Output JSON to stdout / upload to configured endpoint.

### Output JSON schema

    {
      "repository": "owner/repo",
      "commit": "abc123",
      "timestamp": "2026-03-04T12:00:00Z",
      "health": {
        "lambda_max": 1.42,
        "status": "critical",   // "stable" | "critical"
        "modularity_q": 0.35
      },
      "summary": {
        "symbols": 1200,
        "dependencies": 3400
      }
    }

### Language detection

Priority-ordered check:
1. Presence of language-specific config files (`pyproject.toml`, `package.json`, `Cargo.toml`, etc.).
2. File extension frequency analysis as fallback.

## Plan

- [ ] Create `ising-cli` binary crate with `analyze` subcommand (reads `.scip`, outputs JSON)
- [ ] Define Dockerfile for base image (Rust binary only)
- [ ] Create Python worker Dockerfile (base + Python + scip-python)
- [ ] Create TypeScript worker Dockerfile (base + Node + scip-typescript)
- [ ] Implement entrypoint script (clone → detect → index → analyze)
- [ ] Implement language auto-detection logic
- [ ] Integration test: Python repo end-to-end
- [ ] Integration test: TypeScript repo end-to-end
- [ ] CI pipeline for container image builds

## Test

- [ ] Base container builds successfully
- [ ] End-to-end: clone a small Python repo → valid JSON report with correct schema
- [ ] End-to-end: clone a small TypeScript repo → valid JSON report
- [ ] Language auto-detection: Python project detected from `pyproject.toml`
- [ ] Graceful error on unsupported language (non-zero exit + error JSON)

## Prerequisites

- `ising-cli` binary depends on `ising-scip` (002) for `.scip` loading.
- `ising-core` modularity Q (001 remaining item) needed for full `health` output.

## Notes

- Start with Python and TypeScript as first-class. Other languages added by extending the pattern.
- `gossiphs` (tree-sitter based) identified in 005 as potential lightweight fallback for unsupported languages — deferred to a future spec.
- Security: containers run with read-only filesystem, no network after clone, resource limits enforced by orchestrator.
