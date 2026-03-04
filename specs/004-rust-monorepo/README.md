---
status: in-progress
created: 2026-03-04
priority: medium
tags:
- infra
- dx
created_at: 2026-03-04T01:59:24.970706554Z
updated_at: 2026-03-04T01:59:24.970706554Z
---

# Rust Monorepo — Workspace Configuration

## Overview

Upgrade the Rust workspace from a minimal single-crate setup to a proper monorepo with shared workspace metadata and centralized dependency management. Ensures consistency as new crates (e.g., `ising-scip`, workers) are added.

## Design

- **`[workspace.package]`**: Share edition, version, license across all crates
- **`[workspace.dependencies]`**: Centralize all dependency versions in root `Cargo.toml`
- **Crate inheritance**: Each crate uses `workspace = true` to inherit shared settings and deps

## Plan

- [ ] Add `[workspace.package]` with shared metadata to root `Cargo.toml`
- [ ] Add `[workspace.dependencies]` with all current deps to root `Cargo.toml`
- [ ] Update `ising-core/Cargo.toml` to inherit from workspace
- [ ] Verify build and tests pass

## Test

- [ ] `cargo build` succeeds
- [ ] `cargo test` passes
