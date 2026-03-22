# Analysis of codervisor/lean-spec

## Overview

**LeanSpec** is a Spec-Driven Development (SDD) framework for AI-powered
development workflows. Specs are small (< 2,000 tokens), focused documents that
keep both human developers and AI coding agents aligned.

- **Repository:** https://github.com/codervisor/lean-spec
- **License:** MIT
- **Version:** 0.2.27 (as of 2026-03-22)
- **Stars:** 207 | Forks: 14

## Technology Stack

| Layer          | Technology                                        |
| -------------- | ------------------------------------------------- |
| Core engine    | Rust (49%) — CLI, MCP server, HTTP server, parsers |
| Web UI         | TypeScript / React 19 + Vite 7 (47%)              |
| Build system   | Turborepo + pnpm workspaces                       |
| Distribution   | Platform-specific npm packages wrapping Rust bins  |

## Repository Structure

### Rust workspace (`rust/`)

- `leanspec-core` — Core library (AI, database, GitHub, parsers, search,
  session management, spec operations, storage, validators)
- `leanspec-cli` — CLI binary
- `leanspec-http` — Unified HTTP server (REST API + bundled React UI, port 3000)
- `leanspec-mcp` — MCP server for AI agent integration
- `leanspec-sync-bridge` — Synchronization bridge

### TypeScript packages (`packages/`)

- `cli/` — JS wrapper resolving the correct platform-specific Rust binary
- `ui/` — Vite 7 + React 19 SPA (web dashboard)
- `mcp/` — MCP server wrapper delegating to the Rust binary
- `http-server/` — HTTP server package

### Specifications (`specs/`)

372 spec documents serving as both project documentation and SDD examples.

## Key Features

1. **CLI** — `lean-spec init`, `create`, `board` (Kanban), `stats`, `search`, `ui`
2. **MCP Server** — Feeds structured spec context to AI coding agents
3. **Web Dashboard** — Browser-based spec management and visualization
4. **Dependency Tracking** — Maps relationships between specs
5. **Smart Search** — Content and metadata queries across specs
6. **GitHub Integration** — Repo import, cloud deployment readiness
7. **Interactive TUI** — Terminal-based spec management

## Architecture

Unified HTTP server architecture: the Rust backend serves both the REST API and
the bundled React UI on a single port (3000). During development, Vite's proxy
forwards `/api/*` requests to the Rust server for hot-reload.

## Relevance to Ising

The ising project already uses LeanSpec for spec-driven development:
- 23 specs in `specs/` directory
- `.lean-spec/` configuration directory
- Specs guide feature implementation, research, and improvements

Both projects share a similar Rust + TypeScript architecture pattern and are
built for developer tooling / code intelligence use cases.

## Open Issues (6)

- #147 — Bug: inconsistent spec detection behavior
- #144 — Bug: `lean-spec create` fails with invalid template format
- #130 — Feature: author info in spec headers
- #123 — Feature: OpenCode support
- #100 — Feature: knowledge feedback loop
- #116 — Docs: video tutorial request

---

## Ising Analysis Results

Graph built from 877 commits (6-month window, 22 large commits skipped).

### Graph Statistics

| Metric           | Value |
| ---------------- | ----- |
| Nodes            | 4,569 |
| Total edges      | 2,630 |
| Structural edges | 2,427 |
| Change edges     | 203   |
| Cycles           | 2     |
| Signals          | 128   |

### Top 10 Hotspots

Files ranked by `change_frequency x complexity`:

| Rank | File | Score | Freq | Complexity |
| ---- | ---- | ----- | ---- | ---------- |
| 1 | `rust/leanspec-cli/src/commands/session.rs` | 0.32 | 17 | 191 |
| 2 | `rust/leanspec-cli/src/commands/init.rs` | 0.27 | 20 | 139 |
| 3 | `rust/leanspec-core/src/sessions/runner.rs` | 0.26 | 17 | 158 |
| 4 | `rust/leanspec-cli/src/main.rs` | 0.25 | 40 | 64 |
| 5 | `rust/leanspec-core/src/sessions/database.rs` | 0.20 | 15 | 140 |
| 6 | `rust/leanspec-core/src/ai_native/chat.rs` | 0.20 | 18 | 115 |
| 7 | `rust/leanspec-core/src/sessions/manager/lifecycle.rs` | 0.18 | 8 | 234 |
| 8 | `rust/leanspec-cli/src/commands/create.rs` | 0.12 | 12 | 100 |
| 9 | `rust/leanspec-sync-bridge/src/main.rs` | 0.11 | 7 | 165 |
| 10 | `rust/leanspec-http/src/routes.rs` | 0.11 | 44 | 25 |

**Key observations:**
- The **sessions subsystem** dominates the hotspot list (ranks 1, 3, 5, 7) — this
  is the most volatile and complex area of the codebase.
- `lifecycle.rs` has the highest single-file complexity (234) but lower frequency,
  suggesting large, infrequent refactors.
- `routes.rs` has the highest change frequency (44) but low complexity — it's a
  routing registry that gets touched with every new endpoint.
- `main.rs` at rank 4 (freq 40) is expected for a CLI entry point that grows
  with every new command.

### Signals (128 total)

#### Ghost Coupling (96 signals)

Files that always change together but have no structural (import) dependency.
Top ghost coupling pairs by confidence score:

| Score | File A | File B | Assessment |
| ----- | ------ | ------ | ---------- |
| 1.00 | `BoardView.tsx` | `ListView.tsx` | Sibling views — likely share a common data model that should be extracted |
| 1.00 | `dependencies-client.tsx` | `types.ts` | Missing import — types.ts likely defines interfaces used by the client |
| 1.00 | `commands.rs` | `shortcuts.rs` | Desktop Tauri commands tightly coupled to keyboard shortcuts |
| 1.00 | `dashboard-client.tsx` | `main-sidebar.tsx` | UI layout coupling — sidebar navigation drives dashboard content |
| 1.00 | `main.rs` (http) | `routes.rs` | Route registration — expected co-change, likely a false positive |
| 1.00 | `priority-editor.tsx` | `status-editor.tsx` | Parallel UI components — share metadata editing patterns |
| 0.94 | `http.ts` | `tauri.ts` | Backend adapter variants — must stay in sync by contract |
| 0.83 | `api.ts` (types) | `sessions/types.rs` | Cross-language type coupling — TS types mirror Rust types |

**Patterns identified:**
1. **UI sibling coupling** (most signals): Parallel React components (Board/List,
   Priority/Status, Dashboard/Sidebar) that share implicit contracts. Consider
   extracting shared interfaces or using a shared context provider.
2. **Backend adapter coupling**: `http.ts`, `tauri.ts`, `core.ts` in the backend
   adapter layer always change together — they implement the same interface for
   different platforms. This is expected but could benefit from codegen or a
   shared trait.
3. **Cross-language type sync**: TypeScript API types mirror Rust types with no
   structural link. Consider generating TS types from Rust structs.
4. **Multiple UI package generations**: Signals span `packages/ui/`, `packages/ui-vite/`,
   `packages/web/`, and `src/` — indicating the UI has been rewritten multiple
   times, leaving legacy coupling patterns.

#### Over-Engineering (1 signal)

| Score | Files | Assessment |
| ----- | ----- | ---------- |
| 0.40 | `rust/leanspec-http/src/lib.rs` <-> `utils.rs` | Low-traffic utility module — may warrant inlining |

#### Stable Core (31 signals)

High-dependency, low-change files — reliable foundations:

- MCP tool handlers (`view.rs`, `update.rs`, `validate.rs`, `list.rs`, etc.)
- Search subsystem (`query.rs`, `filters.rs`, `scorer.rs`, `fuzzy.rs`)
- Core modules (`github/mod.rs`, `compute/mod.rs`, `io/mod.rs`, `spec_ops/mod.rs`)
- HTTP middleware (`middleware/mod.rs`, `middleware/auth.rs`)

These are well-stabilized modules that other code depends on but rarely need changes.

### Recommendations

1. **Extract shared UI contracts**: The dominant signal is ghost coupling between
   parallel UI components. Introduce shared TypeScript interfaces or context
   providers to make these dependencies explicit.

2. **Generate cross-language types**: The `api.ts` <-> Rust `types.rs` coupling
   could be eliminated with a type generation step (e.g., `ts-rs` or `specta`).

3. **Consolidate UI packages**: Signals across `ui/`, `ui-vite/`, `web/`, and
   `src/` suggest incomplete migrations. Consolidating to a single UI package
   would reduce the maintenance surface.

4. **Decompose session commands**: The sessions subsystem has 4 of the top 7
   hotspots. Consider splitting `session.rs` (complexity 191) into smaller
   subcommand modules.

5. **Watch `lifecycle.rs`**: At complexity 234 it's the most complex single file.
   Its low change frequency suggests it's hard to modify safely — a candidate
   for proactive refactoring before it becomes a bottleneck.
