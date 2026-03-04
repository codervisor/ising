---
status: planned
created: 2026-03-04
priority: high
tags:
- phase-2
- indexing
depends_on:
- 001-rust-core
created_at: 2026-03-04T01:48:38.439719280Z
updated_at: 2026-03-04T01:48:43.095703990Z
---

# SCIP Loader — Index Integration

## Overview

Translate SCIP (Source Code Intelligence Protocol) protobuf index files into the IsingGraph model. SCIP provides high-fidelity symbol definitions and references across files, enabling language-agnostic dependency graph construction.

## Design

- Parse `.scip` protobuf files to extract symbol occurrences (definitions and references).
- Map each definition to a `Symbol` node in `IsingGraph`.
- Map each reference to a directed edge from the referencing symbol to the referenced symbol.
- Support incremental loading for large codebases.

## Plan

- [ ] Add protobuf/prost dependency for SCIP parsing
- [ ] Define SCIP proto schema (or use existing scip crate)
- [ ] Implement `ScipLoader` that reads `.scip` files
- [ ] Map SCIP occurrences to `IsingGraph` nodes and edges
- [ ] Handle cross-file references
- [ ] Integration tests with sample `.scip` files

## Test

- [ ] Parse a minimal `.scip` file with 2 symbols
- [ ] Cross-file reference creates correct edge
- [ ] Unknown symbol references are handled gracefully
- [ ] Performance test with large index file

## Notes

- SCIP is used by Sourcegraph and supports many languages via `scip-python`, `scip-typescript`, `scip-java`, etc.
- Consider using the `scip` crate from crates.io if available, otherwise generate from proto definitions.
