---
status: planned
created: 2026-03-04
priority: high
tags:
- phase-2
- indexing
depends_on:
- 001-rust-core
- 005-lsp-vs-scip-research
created_at: 2026-03-04T01:48:38.439719280Z
updated_at: 2026-03-04T03:03:55.077914015Z
---
# SCIP Loader — Index Integration

## Overview

Translate SCIP (Source Code Intelligence Protocol) protobuf index files into the IsingGraph model. SCIP provides high-fidelity symbol definitions and references across files, enabling language-agnostic dependency graph construction.

## Design

### Crate placement

New crate `ising-scip` in the workspace. Keeps indexing concerns separate from the core graph/physics engine. Depends on `ising-core` for `IsingGraph`, `Symbol`, and `SymbolKind`.

### Dependencies

- `scip` crate (v0.6.x) — provides Protobuf types for the SCIP schema (`scip::types::Index`, `Document`, `Occurrence`, `SymbolInformation`).
- `protobuf` — transitive via `scip` crate; needed for parsing `.scip` files.

### Core API

    pub struct ScipLoader;

    impl ScipLoader {
        /// Load a `.scip` file and return a populated IsingGraph.
        pub fn load_from_file(path: &Path) -> Result<IsingGraph, ScipError>;

        /// Load from an already-parsed SCIP Index.
        pub fn load_from_index(index: &scip::types::Index) -> Result<IsingGraph, ScipError>;
    }

### Symbol mapping

| SCIP SymbolInformation.Kind | IsingGraph SymbolKind |
|---|---|
| Function, Method, Macro | Function |
| Class, Enum, Struct | Class |
| Package, Namespace | Module |
| Variable, Constant, Property | Variable |
| Interface, Trait, Protocol | Interface |
| Everything else | Other(string) |

### Processing pipeline (two-pass)

The loader uses a two-pass approach because `IsingGraph::add_dependency()` requires both symbols to exist before an edge can be created. Since a reference in Document A may point to a symbol defined in Document B (not yet iterated), all definitions must be collected first.

**Pass 1 — Collect definitions:**

1. Read `.scip` file → deserialize into `scip::types::Index`.
2. Iterate `index.documents` — each Document represents one file.
3. For each Document, iterate `document.symbols` (SymbolInformation entries) and `occurrences` with role = Definition → call `IsingGraph::add_symbol()` for each.
4. Symbol identification uses SCIP's fully-qualified symbol string as `Symbol::name`. If a symbol appears in both `document.symbols` and as a definition occurrence, deduplicate by symbol string.

**Pass 2 — Resolve references:**

5. Iterate all documents and occurrences again.
6. For each occurrence with role = Reference → call `IsingGraph::add_dependency(referencing_symbol, referenced_symbol)`.
7. References to symbols not found in the graph (e.g., external/stdlib symbols not defined in the index) are silently skipped rather than producing errors. This is expected — SCIP indexes only the project under analysis, but references may point to dependencies.

### Error handling

    #[derive(Debug, thiserror::Error)]
    pub enum ScipError {
        #[error("failed to read SCIP file: {0}")]
        Io(#[from] std::io::Error),
        #[error("failed to parse SCIP protobuf: {0}")]
        Parse(#[from] protobuf::Error),
        #[error("invalid SCIP data: {0}")]
        InvalidData(String),
    }

## Plan

- [ ] Create `ising-scip` crate in workspace
- [ ] Add `scip` crate dependency, verify protobuf types compile
- [ ] Implement `ScipLoader::load_from_file` and `load_from_index`
- [ ] Implement symbol kind mapping (SCIP → SymbolKind)
- [ ] Implement two-pass occurrence processing (pass 1: definitions → nodes, pass 2: references → edges)
- [ ] Add `ScipError` error type
- [ ] Integration tests with sample `.scip` files

## Test

- [ ] Parse a minimal `.scip` file with 2 symbols and 1 reference
- [ ] Cross-file reference creates correct directed edge
- [ ] Unknown/malformed symbol references produce `ScipError::InvalidData`
- [ ] Reference to external symbol (not in index) is silently skipped
- [ ] SCIP symbol kinds map correctly to `SymbolKind`
- [ ] Empty `.scip` file returns empty IsingGraph
- [ ] Round-trip: generate `.scip` with `rust-analyzer scip .` on a sample crate, load, verify node/edge counts

## Open Questions (resolved)

- **Which crate?** → `scip` v0.6.x from crates.io (confirmed available).
- **Separate crate or module?** → Separate `ising-scip` crate to keep `ising-core` dependency-light.
- **Incremental loading?** → Deferred. V1 loads full `.scip` file. Can add streaming later if perf requires it.
- **One-pass or two-pass?** → Two-pass. `IsingGraph::add_dependency()` requires both endpoints to exist, so all definitions must be registered before references are resolved.
