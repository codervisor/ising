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
