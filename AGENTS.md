# Ising

## 🧭 Project Context

**Ising** is a maintainability analysis tool for growing software projects. It uses spectral graph theory and concepts from statistical physics (the Ising Model) to analyze code dependency graphs and surface actionable code quality improvements.

### Positioning

- **Goal**: Help teams ensure maintainability of growing codebases and identify concrete code quality improvements before technical debt compounds.
- **Not**: An AI agent, a linter, or an IDE plugin. Ising is an analysis engine that quantifies architectural health.
- **Mechanism**: SCIP-based code indexing → dependency graph construction → spectral analysis (λ_max, modularity Q) → health scoring.

### Key Concepts

| Concept | Meaning |
|---|---|
| **λ_max (spectral radius)** | Measures change propagation risk. λ < 1 = stable, λ > 1 = fragile. |
| **Modularity Q** | Measures how well code separates into independent modules. Low Q = "Big Ball of Mud." |
| **SCIP indexing** | Language-agnostic batch extraction of symbols and references from source code. |
| **IsingGraph** | Our core graph model wrapping petgraph — symbols as nodes, dependencies as edges. |

### Architecture

- **`ising-core/`**: Rust crate with `graph` module (IsingGraph, Symbol types) and `physics` module (spectral analysis, health scoring).
- **Build/Test**: `cargo build` and `cargo test` from workspace root.

### Roadmap

| Phase | Spec | Status |
|---|---|---|
| Spectral engine | 001-rust-core | in-progress |
| SCIP index loading | 002-scip-loader | planned |
| Containerized workers | 003-container-workers | planned |

---

## 🚨 CRITICAL: Before ANY Task

**STOP and check these first:**

1. **Discover context** → Use `board` tool to see project state
2. **Search for related work** → Use `search` tool before creating new specs
3. **Never create files manually** → Always use `create` tool for new specs

> **Why?** Skipping discovery creates duplicate work. Manual file creation breaks LeanSpec tooling.

## 🔧 Managing Specs

### MCP Tools (Preferred) with CLI Fallback

| Action         | MCP Tool   | CLI Fallback                                   |
| -------------- | ---------- | ---------------------------------------------- |
| Project status | `board`    | `lean-spec board`                              |
| List specs     | `list`     | `lean-spec list`                               |
| Search specs   | `search`   | `lean-spec search "query"`                     |
| View spec      | `view`     | `lean-spec view <spec>`                        |
| Create spec    | `create`   | `lean-spec create <name>`                      |
| Update spec    | `update`   | `lean-spec update <spec> --status <status>`    |
| Link specs     | `link`     | `lean-spec link <spec> --depends-on <other>`   |
| Unlink specs   | `unlink`   | `lean-spec unlink <spec> --depends-on <other>` |
| Dependencies   | `deps`     | `lean-spec deps <spec>`                        |
| Token count    | `tokens`   | `lean-spec tokens <spec>`                      |
| Validate specs | `validate` | `lean-spec validate`                           |

## ⚠️ Core Rules

| Rule                                | Details                                                                                                               |
| ----------------------------------- | --------------------------------------------------------------------------------------------------------------------- |
| **NEVER edit frontmatter manually** | Use `update`, `link`, `unlink` for: `status`, `priority`, `tags`, `assignee`, `transitions`, timestamps, `depends_on` |
| **ALWAYS link spec references**     | Content mentions another spec → `lean-spec link <spec> --depends-on <other>`                                          |
| **Track status transitions**        | `planned` → `in-progress` (before coding) → `complete` (after done)                                                   |
| **Keep specs current**              | Document progress, decisions, and learnings as work happens. Obsolete specs mislead both humans and AI                |
| **No nested code blocks**           | Use indentation instead                                                                                               |

### 🚫 Common Mistakes

| ❌ Don't                             | ✅ Do Instead                                |
| ----------------------------------- | ------------------------------------------- |
| Create spec files manually          | Use `create` tool                           |
| Skip discovery                      | Run `board` and `search` first              |
| Leave status as "planned"           | Update to `in-progress` before coding       |
| Edit frontmatter manually           | Use `update` tool                           |
| Complete spec without documentation | Document progress, prompts, learnings first |

## 📋 SDD Workflow

```
BEFORE: board → search → check existing specs
DURING: update status to in-progress → code → document decisions → link dependencies
AFTER:  document completion → update status to complete
```

**Status tracks implementation, NOT spec writing.**

## Spec Dependencies

Use `depends_on` to express blocking relationships between specs:
- **`depends_on`** = True blocker, work order matters, directional (A depends on B)

Link dependencies when one spec builds on another:
```bash
lean-spec link <spec> --depends-on <other-spec>
```

## When to Use Specs

| ✅ Write spec        | ❌ Skip spec                |
| ------------------- | -------------------------- |
| Multi-part features | Bug fixes                  |
| Breaking changes    | Trivial changes            |
| Design decisions    | Self-explanatory refactors |

## Token Thresholds

| Tokens      | Status               |
| ----------- | -------------------- |
| <2,000      | ✅ Optimal            |
| 2,000-3,500 | ✅ Good               |
| 3,500-5,000 | ⚠️ Consider splitting |
| >5,000      | 🔴 Must split         |

## Quality Validation

Before completing work, validate spec quality:
```bash
lean-spec validate              # Check structure and quality
lean-spec validate --check-deps # Verify dependency alignment
```

Validation checks:
- Missing required sections
- Excessive length (>400 lines)
- Content/frontmatter dependency misalignment
- Invalid frontmatter fields

## 🎯 Implementation Guidelines

When contributing code to Ising, keep these in mind:

- **Maintainability is our product** — our own code must exemplify what we preach. Keep modules decoupled, APIs clean, and dependencies minimal.
- **Portable builds** — avoid external C library dependencies (e.g., LAPACK). Pure Rust preferred.
- **Workspace conventions** — shared deps in root `Cargo.toml` via `[workspace.dependencies]`. Crates inherit with `workspace = true`.
- **Testing** — unit tests alongside modules. Integration tests for cross-module workflows (e.g., graph → physics pipeline).

## First Principles (Priority Order)

1. **Context Economy** - <2,000 tokens optimal, >3,500 needs splitting
2. **Signal-to-Noise** - Every word must inform a decision
3. **Intent Over Implementation** - Capture why, let how emerge
4. **Bridge the Gap** - Both human and AI must understand
5. **Progressive Disclosure** - Add complexity only when pain is felt

---

**Remember:** LeanSpec tracks what you're building. Keep specs in sync with your work!
