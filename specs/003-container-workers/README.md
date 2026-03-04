---
status: planned
created: 2026-03-04
priority: medium
tags:
- phase-3
- cloud
depends_on:
- 002-scip-loader
created_at: 2026-03-04T01:48:38.439898044Z
updated_at: 2026-03-04T01:48:43.096008720Z
---

# Containerized Workers — Cloud Analysis

## Overview

Containerized workers that encapsulate language-specific SCIP indexers (e.g., `scip-python`, `scip-typescript`) to provide a zero-configuration cloud experience. Users point at a git repo and get an Ising analysis without installing anything locally.

## Design

- Docker image with pre-installed SCIP indexers for target languages.
- Entrypoint script: `git clone` → run indexer → produce `.scip` → run `ising-core` analysis.
- Output: JSON report with HealthScore, λ_max, modularity Q, and identified clusters.
- Support for multiple languages via composable indexer images.

## Plan

- [ ] Design Dockerfile with multi-stage build (indexers + Rust binary)
- [ ] Create entrypoint script for clone → index → analyze workflow
- [ ] Define JSON output schema for analysis results
- [ ] Support language auto-detection
- [ ] Add CI/CD pipeline for building and publishing container images
- [ ] Integration tests with sample repositories

## Test

- [ ] Container builds successfully
- [ ] End-to-end: clone a small Python repo → produce valid JSON report
- [ ] Language auto-detection works for Python, TypeScript
- [ ] Graceful failure on unsupported languages

## Notes

- Start with Python and TypeScript indexers as first-class citizens.
- Consider using GitHub Actions or similar CI for automated container builds.
