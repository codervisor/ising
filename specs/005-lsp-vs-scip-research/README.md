---
status: complete
created: 2026-03-04
priority: low
tags:
- research
- adr
- indexing
created_at: 2026-03-04T03:03:48.697674119Z
updated_at: 2026-03-04T03:03:48.697674119Z
---

# LSP vs SCIP Research — Indexing Strategy Decision

## Overview

Architecture Decision Record evaluating whether existing LSP libraries could replace SCIP for Ising's code indexing needs. Conclusion: **SCIP is the correct choice**; LSP is architecturally unsuitable for batch dependency graph extraction.

## Decision

Use SCIP (not LSP) as the indexing protocol for Ising. This confirms the approach in specs 002 and 003.

## Analysis

### Why LSP is unsuitable

1. **Interactive, not batch.** LSP is a JSON-RPC protocol for real-time editor features. No "export all symbols" endpoint — you must open files, query each position, crawl iteratively.
2. **Requires a running server.** Long-lived processes designed for editing sessions — overkill for batch extraction.
3. **No whole-project export.** Building a dependency graph requires synthesizing thousands of individual requests.
4. **LSIF/SCIP exist to solve this.** Microsoft created LSIF to pre-compute LSP results; Sourcegraph created SCIP as its successor (4-5x smaller, 3x faster, Protobuf typed).

### Why SCIP is correct

- **Batch-native**: `git clone → run indexer CLI → .scip file` matches Ising's container workflow.
- **Production indexers exist**: `rust-analyzer scip .`, `scip-python`, `scip-typescript`, `scip-java`, plus Go, C/C++, Ruby, C#, PHP, Dart, Kotlin.
- **Rust bindings**: `scip` crate provides ready-made Protobuf types.
- **Rich schema**: Symbol kinds, relationships (implements, type-def), hover docs, cross-package references.

### Key projects evaluated

| Project | Lang | What | Relevance |
|---|---|---|---|
| sourcegraph/scip | Go/Rust | SCIP protocol + bindings | Core dep for 002 |
| butttons/dora | TS | AI-agent CLI over SCIP, SQLite | Reference for SCIP usage patterns (different goal: dora is an AI-agent CLI; Ising focuses on maintainability analysis and code quality insights for growing projects) |
| williamfzc/gossiphs | Rust | tree-sitter + git heuristic graphs | Fast-mode fallback (~90% accuracy) |
| williamfzc/srctx | Go | Def/ref graphs from LSIF/SCIP | Direct prior art |
| ebkalderon/tower-lsp | Rust | LSP server framework | Not useful for indexing |

## Implications

### For 002-scip-loader
- Use `scip` crate or generate from scip.proto via prost.
- SCIP Document → per-file processing; Occurrence → IsingGraph edges.
- `dora`'s SQLite pattern worth studying for queryable storage.

### For 003-container-workers
- Each container: language runtime + SCIP indexer binary → `scip-<lang> index --output index.scip`.
- `gossiphs` as lightweight fallback for unsupported languages (tree-sitter, zero config).

## Notes

- SCIP protobuf key types: Index, Document, Occurrence, SymbolInformation, Relationship.
- Research complete — no code changes. Decision only.
