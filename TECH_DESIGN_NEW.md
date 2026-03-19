# Ising: Three-Layer Code Graph Analysis Engine

## Design Document v0.1

-----

## 1. Problem Statement

AI coding agents (Claude Code, Aider, Cursor, etc.) lack a reliable “map” of codebases they operate on. Current solutions (code-review-graph, GitNexus, Aider RepoMap) only provide **structural graphs** extracted from code via Tree-sitter. They answer “what calls what” but not “what actually changes together” or “where do bugs propagate.”

Ising builds a **three-layer overlay graph** that combines:

- **Layer 1 — Structural Graph**: static dependencies from code (AST)
- **Layer 2 — Change Graph**: temporal coupling from git history
- **Layer 3 — Defect Graph**: fault propagation from issue tracker + git blame

The key innovation is **cross-layer signal detection**: anomalies that only appear when comparing across layers (e.g., files with no structural dependency that always change together).

-----

## 2. Architecture Overview

```
┌─────────────────────────────────────────────────┐
│                  Ising CLI                     │
│         ising build | query | watch            │
└──────────┬──────────┬──────────┬────────────────┘
           │          │          │
     ┌─────▼──┐ ┌────▼───┐ ┌───▼─────┐
     │Struct  │ │Change  │ │Defect   │
     │Graph   │ │Graph   │ │Graph    │
     │Builder │ │Builder │ │Builder  │
     └───┬────┘ └───┬────┘ └───┬─────┘
         │          │          │
         ▼          ▼          ▼
     Tree-sitter  git log    git blame
     AST parser   analysis   + issue
                              tracker
         │          │          │
         └──────────┼──────────┘
                    ▼
            ┌──────────────┐
            │ Unified Graph │  ← nodes = files/functions/classes
            │   (SQLite)    │  ← edges = typed, multi-layer
            └──────┬───────┘
                   │
            ┌──────▼───────┐
            │ Signal Engine │  ← cross-layer anomaly detection
            └──────┬───────┘
                   │
            ┌──────▼───────┐
            │  Agent Tools  │  ← MCP server / function calling
            └──────────────┘
```

-----

## 3. Layer 1 — Structural Graph

### 3.1 Data Source

Tree-sitter AST parsing of the codebase. Support Python, TypeScript, Go, C# as initial languages.

### 3.2 Node Types

|Node Type |Granularity     |Attributes                                                     |
|----------|----------------|---------------------------------------------------------------|
|`module`  |file-level      |path, language, LOC                                            |
|`class`   |class-level     |name, file, line_start, line_end, methods_count                |
|`function`|function-level  |name, file, line_start, line_end, cyclomatic_complexity, params|
|`import`  |import statement|source, target, symbols                                        |

### 3.3 Edge Types

|Edge Type |Description                       |Weight                    |
|----------|----------------------------------|--------------------------|
|`calls`   |function A calls function B       |call_count within file    |
|`imports` |module A imports from module B    |number of symbols imported|
|`inherits`|class A extends/implements class B|1                         |
|`contains`|module contains class/function    |1                         |

### 3.4 Node-Level Metrics

For each node, compute:

- **Fan-in**: number of incoming `calls` + `imports` edges
- **Fan-out**: number of outgoing `calls` + `imports` edges
- **CBO** (Coupling Between Objects): count of distinct modules this node depends on
- **Cyclomatic Complexity**: from AST control flow analysis
- **LOC**: lines of code (excluding blanks and comments)
- **Nesting Depth**: max indentation level (proxy for cognitive complexity)

### 3.5 Graph-Level Metrics

- **Modularity Score**: Newman modularity (community detection quality, range [-0.5, 1])
- **Cycle Count**: number of circular dependency chains
- **Instability per module**: Fan-out / (Fan-in + Fan-out) — Robert C. Martin’s metric

### 3.6 Implementation

```rust
// Pseudocode
use tree_sitter::{Parser, Language};
use rayon::prelude::*;

fn build_structural_graph(repo_path: &Path) -> Result<Graph> {
    let mut graph = Graph::new();
    
    let source_files = walk_source_files(repo_path);
    
    // Parallel parse with rayon
    let file_results: Vec<FileAnalysis> = source_files
        .par_iter()
        .filter_map(|file| {
            let lang = detect_language(file)?;
            let mut parser = Parser::new();
            parser.set_language(lang).ok()?;
            let source = fs::read_to_string(file).ok()?;
            let tree = parser.parse(&source, None)?;
            Some(analyze_file(file, &tree, &source))
        })
        .collect();
    
    // Build graph from results (single-threaded, graph not Sync)
    for result in file_results {
        for func in &result.functions {
            graph.add_node(&func.qualified_name, NodeType::Function, &func.attrs);
        }
        for imp in &result.imports {
            graph.add_edge(&result.file, &imp.target, EdgeType::Imports);
        }
        for call in &result.calls {
            graph.add_edge(&call.caller, &call.callee, EdgeType::Calls);
        }
    }
    
    Ok(graph)
}
```

-----

## 4. Layer 2 — Change Graph

### 4.1 Data Source

`git log` parsed via gix (gitoxide, pure Rust). Extract co-change relationships from commit history.

### 4.2 Core Metrics

**4.2.1 Change Frequency**

```
change_freq(file) = number of commits touching file in time window
```

Time window is configurable (default: 6 months). Normalize by total commits in window.

**4.2.2 Temporal Coupling (Co-Change Frequency)**

For any two files A and B:

```
co_change(A, B) = number of commits where both A and B are modified
coupling(A, B) = co_change(A, B) / min(change_freq(A), change_freq(B))
```

Threshold: ignore pairs where `co_change < 5` (avoid noise from accidental co-changes).

coupling ∈ [0, 1], where 1 = always change together.

**4.2.3 Sum of Coupling**

```
sum_coupling(file) = Σ coupling(file, other) for all other files where coupling > threshold
```

Files with high Sum of Coupling are “architectural hubs” — central to the system’s change dynamics.

**4.2.4 Code Churn**

```
churn(file, window) = lines_added + lines_deleted in window
churn_rate(file) = churn(file) / LOC(file)
```

High churn_rate = volatile code. Combined with high complexity → high-risk hotspot.

**4.2.5 Hotspot Score**

Following Tornhill’s model:

```
hotspot(file) = normalize(change_freq(file)) * normalize(complexity(file))
```

Where complexity comes from Layer 1 (cyclomatic complexity or LOC).

### 4.3 Edge Types

|Edge Type          |Description                           |Weight                 |
|-------------------|--------------------------------------|-----------------------|
|`co_changes`       |A and B changed in same commit        |coupling score [0,1]   |
|`change_propagates`|A changed → B changed within N commits|propagation probability|

### 4.4 Implementation

```rust
fn build_change_graph(repo_path: &Path, since: &str) -> Result<Graph> {
    let mut graph = Graph::new();
    let repo = gix::open(repo_path)?;
    
    // Parse git log via gix
    let commits = parse_commit_history(&repo, since)?;
    
    // Build co-change matrix
    let mut file_changes: HashMap<String, u32> = HashMap::new();
    let mut co_changes: HashMap<(String, String), u32> = HashMap::new();
    
    for commit in &commits {
        let files = &commit.changed_files;
        for f in files {
            *file_changes.entry(f.clone()).or_default() += 1;
        }
        // All unique pairs
        for (i, a) in files.iter().enumerate() {
            for b in &files[i + 1..] {
                let key = ordered_pair(a, b);
                *co_changes.entry(key).or_default() += 1;
            }
        }
    }
    
    // Compute coupling scores
    for ((a, b), count) in &co_changes {
        if *count >= MIN_CO_CHANGES {  // threshold = 5
            let denom = file_changes[a].min(file_changes[b]) as f64;
            let score = *count as f64 / denom;
            if score >= MIN_COUPLING {  // threshold = 0.3
                graph.add_edge(a, b, EdgeType::CoChanges, score);
            }
        }
    }
    
    // Compute hotspot scores
    for (file, freq) in &file_changes {
        let complexity = structural_graph.get_complexity(file).unwrap_or(1);
        let hotspot = normalize(*freq) * normalize(complexity);
        let churn_rate = compute_churn(&repo, file, since)? as f64 / get_loc(file) as f64;
        graph.set_node_attr(file, NodeAttr::Hotspot(hotspot));
        graph.set_node_attr(file, NodeAttr::ChurnRate(churn_rate));
    }
    
    Ok(graph)
}
```

-----

## 5. Layer 3 — Defect Graph

### 5.1 Data Source

Two sources combined:

1. **Issue tracker** (GitHub Issues, Jira): bug reports with fix commits linked via commit messages (e.g., “fixes #123”)
1. **git blame + SZZ algorithm**: trace from fix commit back to the commit that introduced the bug

### 5.2 Core Metrics

**5.2.1 Defect Density**

```
defect_density(file) = bug_count(file, window) / LOC(file)
```

**5.2.2 Fix-Inducing Change Probability**

For a file, what percentage of its changes later require a fix?

```
fix_inducing_rate(file) = fix_inducing_commits(file) / total_commits(file)
```

**5.2.3 Fault Propagation Probability**

If file A was changed and file B later had a bug attributed to that change:

```
fault_propagation(A → B) = count(A_change_causes_B_bug) / count(A_changes)
```

This is the most valuable metric — it tells the Agent “if you change A, watch out for B.”

### 5.3 Edge Types

|Edge Type         |Description                                  |Weight                 |
|------------------|---------------------------------------------|-----------------------|
|`fault_propagates`|change in A historically causes bug in B     |propagation probability|
|`co_fix`          |A and B are both modified in the same bug fix|co-fix count           |

### 5.4 SZZ Algorithm (Simplified)

```rust
fn identify_bug_introducing_commits(repo: &gix::Repository) -> Result<Vec<BugIntro>> {
    let mut results = Vec::new();
    
    // Step 1: Find fix commits (linked to bug issues)
    let fix_commits = find_fix_commits(repo)?;  // parse "fixes #N" in messages
    
    for fix in &fix_commits {
        // Step 2: Get lines changed in fix
        let changed_lines = get_diff_lines(repo, fix)?;
        
        // Step 3: git blame those lines at parent commit to find introducing commit
        for (file, lines) in &changed_lines {
            for line in lines {
                let introducing = git_blame(repo, file, *line, fix.parent_id())?;
                results.push(BugIntro {
                    fix_commit: fix.id(),
                    introducing_commit: introducing,
                    file: file.clone(),
                    line: *line,
                });
            }
        }
    }
    
    Ok(results)
}
```

### 5.5 Fallback: No Issue Tracker

If no issue tracker data is available, use heuristic:

- Commits with messages containing “fix”, “bug”, “hotfix”, “patch”, “revert” are treated as fix commits
- Less accurate but still useful for temporal defect analysis

-----

## 6. Cross-Layer Signal Detection

This is the core innovation. Each signal is a **comparison between layers**.

### 6.1 Signal Definitions

**Signal 1: Ghost Coupling**

```
ghost_coupling(A, B) = 
    structural_edge(A, B) == False  AND
    temporal_coupling(A, B) > 0.5
```

Meaning: A and B have no code-level dependency, but always change together. Indicates hidden dependency, likely copy-paste code or shared implicit contract.

**Priority**: High if either A or B is a hotspot.

**Signal 2: Fragile Boundary**

```
fragile_boundary(A, B) =
    structural_edge(A, B) == True  AND
    temporal_coupling(A, B) > 0.3  AND
    fault_propagation(A, B) > 0.1
```

Meaning: A depends on B, they change together, AND changes to A cause bugs in B. This is the most dangerous pattern — the interface between A and B is broken.

**Priority**: Critical. This is where the Agent should be most careful.

**Signal 3: Over-Engineering**

```
over_engineering(A, B) =
    structural_edge(A, B) == True  AND
    temporal_coupling(A, B) < 0.05  AND
    fault_propagation(A, B) == 0
```

Meaning: A depends on B in code, but they never change together and never cause each other bugs. The dependency may be unnecessary abstraction.

**Priority**: Low (informational for refactoring).

**Signal 4: Stable Core**

```
stable_core(A) =
    change_freq(A) < bottom_10%  AND
    fan_in(A) > top_20%  AND
    defect_density(A) < bottom_10%
```

Meaning: A is heavily depended upon, rarely changes, and rarely has bugs. This is a stable foundation module — protect it from unnecessary changes.

**Priority**: Guard these modules.

**Signal 5: Ticking Bomb**

```
ticking_bomb(A) =
    hotspot(A) > top_10%  AND
    defect_density(A) > top_10%  AND
    sum_coupling(A) > top_20%
```

Meaning: A is complex, frequently changed, buggy, AND coupled to many other files. Changes here are extremely risky.

**Priority**: Critical. Refactor before making more changes.

### 6.2 Signal Computation

```rust
fn detect_signals(
    structural: &Graph,
    change: &Graph,
    defect: &Graph,
) -> Vec<Signal> {
    let mut signals = Vec::new();
    let all_nodes: HashSet<&str> = structural.nodes()
        .chain(change.nodes())
        .chain(defect.nodes())
        .collect();
    
    // Edge-level signals (iterate over change graph edges — sparser than all pairs)
    for (a, b, coupling) in change.edges_of_type(EdgeType::CoChanges) {
        let s_edge = structural.has_edge(a, b);
        let f_prop = defect.edge_weight(a, b, EdgeType::FaultPropagates).unwrap_or(0.0);
        
        // Ghost Coupling
        if !s_edge && coupling > 0.5 {
            signals.push(Signal::new(SignalType::GhostCoupling, a, Some(b), coupling));
        }
        
        // Fragile Boundary
        if s_edge && coupling > 0.3 && f_prop > 0.1 {
            signals.push(Signal::new(SignalType::FragileBoundary, a, Some(b), coupling * f_prop));
        }
    }
    
    // Over-Engineering: structural edges with no temporal activity
    for (a, b) in structural.edges_of_type(EdgeType::Imports) {
        let coupling = change.edge_weight(a, b, EdgeType::CoChanges).unwrap_or(0.0);
        let f_prop = defect.edge_weight(a, b, EdgeType::FaultPropagates).unwrap_or(0.0);
        if coupling < 0.05 && f_prop == 0.0 {
            signals.push(Signal::new(SignalType::OverEngineering, a, Some(b), 0.3));
        }
    }
    
    // Pre-compute percentiles for node-level signals
    let freq_p10 = percentile(change.all_attr(NodeAttr::ChangeFreq), 10);
    let fan_in_p80 = percentile(structural.all_attr(NodeAttr::FanIn), 80);
    let hotspot_p90 = percentile(change.all_attr(NodeAttr::Hotspot), 90);
    let defect_p90 = percentile(defect.all_attr(NodeAttr::DefectDensity), 90);
    
    // Node-level signals
    for node in &all_nodes {
        let freq = change.node_attr(node, NodeAttr::ChangeFreq).unwrap_or(0);
        let fan_in = structural.node_attr(node, NodeAttr::FanIn).unwrap_or(0);
        let hotspot = change.node_attr(node, NodeAttr::Hotspot).unwrap_or(0.0);
        let defect_d = defect.node_attr(node, NodeAttr::DefectDensity).unwrap_or(0.0);
        
        // Stable Core
        if freq < freq_p10 && fan_in > fan_in_p80 {
            signals.push(Signal::new(SignalType::StableCore, node, None, 0.1));
        }
        
        // Ticking Bomb
        if hotspot > hotspot_p90 && defect_d > defect_p90 {
            signals.push(Signal::new(SignalType::TickingBomb, node, None, hotspot * defect_d));
        }
    }
    
    signals.sort_by(|a, b| b.severity.partial_cmp(&a.severity).unwrap());
    signals
}
```

-----

## 7. Storage Schema (SQLite)

### 7.1 Tables

```sql
-- Nodes (unified across all layers)
CREATE TABLE nodes (
    id TEXT PRIMARY KEY,          -- qualified name: "src/auth/login.py::LoginService"
    type TEXT NOT NULL,           -- module | class | function
    file_path TEXT NOT NULL,
    line_start INTEGER,
    line_end INTEGER,
    language TEXT,
    loc INTEGER,
    complexity INTEGER,
    nesting_depth INTEGER
);

-- Edges (multi-layer, typed)
CREATE TABLE edges (
    source TEXT NOT NULL,
    target TEXT NOT NULL,
    layer TEXT NOT NULL,          -- structural | change | defect
    edge_type TEXT NOT NULL,      -- calls | imports | co_changes | fault_propagates | ...
    weight REAL DEFAULT 1.0,
    metadata JSON,               -- layer-specific data
    FOREIGN KEY (source) REFERENCES nodes(id),
    FOREIGN KEY (target) REFERENCES nodes(id)
);

-- Layer 2 specific: per-node change metrics
CREATE TABLE change_metrics (
    node_id TEXT PRIMARY KEY,
    change_freq INTEGER,
    churn_lines INTEGER,
    churn_rate REAL,
    hotspot_score REAL,
    sum_coupling REAL,
    last_changed TEXT,           -- ISO datetime
    FOREIGN KEY (node_id) REFERENCES nodes(id)
);

-- Layer 3 specific: per-node defect metrics
CREATE TABLE defect_metrics (
    node_id TEXT PRIMARY KEY,
    bug_count INTEGER,
    defect_density REAL,
    fix_inducing_rate REAL,
    FOREIGN KEY (node_id) REFERENCES nodes(id)
);

-- Cross-layer signals
CREATE TABLE signals (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    signal_type TEXT NOT NULL,    -- ghost_coupling | fragile_boundary | ...
    node_a TEXT NOT NULL,
    node_b TEXT,                  -- NULL for node-level signals
    severity REAL NOT NULL,
    details JSON,
    detected_at TEXT NOT NULL,    -- ISO datetime
    FOREIGN KEY (node_a) REFERENCES nodes(id)
);

-- Build metadata
CREATE TABLE build_info (
    key TEXT PRIMARY KEY,
    value TEXT
);
-- e.g., ("last_build", "2025-01-01T00:00:00"), ("commit_sha", "abc123"), ("time_window", "6 months")
```

### 7.2 Indexes

```sql
CREATE INDEX idx_edges_source ON edges(source);
CREATE INDEX idx_edges_target ON edges(target);
CREATE INDEX idx_edges_layer ON edges(layer);
CREATE INDEX idx_signals_type ON signals(signal_type);
CREATE INDEX idx_signals_severity ON signals(severity DESC);
```

-----

## 8. CLI Interface

### 8.1 Commands

```bash
# Full build: parse code + analyze git history + detect signals
ising build [--repo-path .] [--since "6 months ago"] [--db ising.db]

# Incremental update: only re-process changed files since last build
ising update

# Watch mode: rebuild on file changes and git commits
ising watch

# Query: what does the Agent need to know?
ising impact <file_or_function>      # blast radius + signals for a node
ising hotspots [--top 20]            # top hotspots ranked by score
ising signals [--type ghost_coupling] [--min-severity 0.5]
ising path <node_a> <node_b>         # shortest dependency path between two nodes
ising neighbors <node> [--depth 2]   # local subgraph around a node
ising stats                          # global health metrics

# Export
ising export --format json           # full graph as JSON
ising export --format dot            # Graphviz DOT format
ising export --format mermaid        # Mermaid diagram
```

### 8.2 Example Output

```bash
$ ising impact src/auth/login.py

Module: src/auth/login.py
  Complexity: 42 | LOC: 380 | Hotspot: 0.87 (top 5%)
  Change Freq: 34 commits in 6 months | Churn Rate: 2.3

Structural Dependencies (fan-out: 5):
  → src/db/user_store.py        (imports: get_user, update_user)
  → src/auth/token.py           (imports: generate_jwt)
  → src/auth/password.py        (calls: verify_hash)
  → src/middleware/rate_limit.py (imports: check_rate)
  → src/events/audit.py         (calls: log_event)

Temporal Coupling (co-change > 0.3):
  ↔ src/auth/token.py           coupling: 0.82
  ↔ src/api/v2/auth_routes.py   coupling: 0.71
  ↔ tests/test_auth.py          coupling: 0.65

⚠ SIGNALS:
  [CRITICAL] fragile_boundary: login.py → token.py
    Structural dep + high co-change (0.82) + fault propagation (0.18)
    → 18% of changes to login.py historically cause bugs in token.py
  
  [HIGH] ghost_coupling: login.py ↔ auth_routes.py
    No structural dependency, but 71% co-change rate
    → Likely missing an abstraction layer
  
  [INFO] stable dependency: login.py → password.py
    Low co-change, zero fault propagation. This is a clean boundary.

Recommendation: If changing login.py, also review token.py (fragile) 
and auth_routes.py (hidden dependency). Run tests for both.
```

-----

## 9. Agent Integration (MCP Server)

Expose as MCP tools for Claude Code / Cursor / other agents:

### 9.1 Tools

```json
{
  "tools": [
    {
      "name": "ising_impact",
      "description": "Get blast radius, dependencies, and risk signals for a file or function before making changes",
      "parameters": {
        "target": "string — file path or qualified function name",
        "depth": "integer — how many hops to traverse (default: 2)"
      }
    },
    {
      "name": "ising_locate",
      "description": "Find the most relevant files for a given task based on structural + temporal + defect signals",
      "parameters": {
        "intent": "string — natural language description of what you want to change",
        "top_k": "integer — number of results (default: 10)"
      }
    },
    {
      "name": "ising_signals",
      "description": "Get active risk signals, optionally filtered by type or severity",
      "parameters": {
        "type": "string — ghost_coupling | fragile_boundary | ticking_bomb | ...",
        "min_severity": "float — minimum severity threshold (default: 0.3)"
      }
    },
    {
      "name": "ising_path",
      "description": "Find dependency path between two nodes across all three layers",
      "parameters": {
        "source": "string — source node",
        "target": "string — target node"
      }
    }
  ]
}
```

### 9.2 Agent Workflow

The expected Agent behavior when making code changes:

```
1. Receive task: "Fix the login timeout bug"
2. Call ising_locate("login timeout") → returns ranked files
3. Call ising_impact("src/auth/login.py") → get blast radius + signals
4. Read signals: token.py is fragile boundary → include in review scope
5. Make code changes
6. Run ising update → check if new signals were introduced
7. If new ticking_bomb or fragile_boundary signal → flag for human review
```

-----

## 10. Tech Stack

|Component               |Technology                                                                 |Rationale                                                                  |
|------------------------|---------------------------------------------------------------------------|---------------------------------------------------------------------------|
|Language                |Rust (edition 2021)                                                        |Performance, memory safety, single binary distribution                     |
|AST Parsing             |tree-sitter (rust crate) + tree-sitter-python, tree-sitter-typescript, etc.|Native Rust bindings, zero-copy parsing, multi-language                    |
|Graph computation       |petgraph                                                                   |Rust-native graph library, PageRank, DFS/BFS, strongly connected components|
|Storage                 |rusqlite (SQLite)                                                          |Zero-dependency, single-file, embedded, fast                               |
|Git analysis            |gix (gitoxide)                                                             |Pure Rust git implementation, no libgit2 dependency, high performance      |
|CLI                     |clap (derive)                                                              |Standard Rust CLI framework, auto-generated help and completions           |
|MCP Server              |axum + tokio (SSE transport)                                               |Async Rust HTTP, expose tools to Claude Code / Cursor                      |
|Serialization           |serde + serde_json                                                         |Zero-cost serialization for config, export, MCP protocol                   |
|Config                  |toml (serde)                                                               |`ising.toml` config file, Rust ecosystem standard                          |
|Visualization (optional)|Mermaid / DOT string export                                                |For human review                                                           |
|Error handling          |thiserror + anyhow                                                         |Typed errors for library, ergonomic errors for CLI                         |
|Logging                 |tracing + tracing-subscriber                                               |Structured logging, performance tracing                                    |

### 10.1 Key Cargo Dependencies

```toml
[dependencies]
tree-sitter = "0.24"
tree-sitter-python = "0.23"
tree-sitter-typescript = "0.23"
tree-sitter-go = "0.23"
tree-sitter-c-sharp = "0.23"
gix = { version = "0.68", features = ["max-performance"] }
petgraph = "0.7"
rusqlite = { version = "0.32", features = ["bundled"] }
clap = { version = "4", features = ["derive"] }
axum = "0.8"
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
rayon = "1.10"
thiserror = "2"
anyhow = "1"
tracing = "0.1"
tracing-subscriber = "0.3"
walkdir = "2"
regex = "1"
```

-----

## 11. MVP Scope (Phase 1)

### In Scope

- Layer 1: Structural graph for Python and TypeScript (Tree-sitter)
- Layer 2: Change graph from git log (temporal coupling + hotspots)
- Cross-layer signals: ghost_coupling, fragile_boundary, stable_core, ticking_bomb
- CLI: `build`, `impact`, `hotspots`, `signals`
- SQLite storage
- MCP server with `ising_impact` and `ising_signals` tools

### Out of Scope (Phase 2+)

- Layer 3: Defect graph (requires issue tracker integration)
- Additional analysis target languages (Go, C#, Java, Rust, C/C++)
- Watch mode / incremental updates
- Web UI visualization
- `ising_locate` with semantic search (requires embeddings)
- Multi-repo support
- IDE plugin

-----

## 12. File Structure

```
ising/
├── Cargo.toml
├── Cargo.lock
├── README.md
├── ising.toml                      # Default config (thresholds, time windows)
├── src/
│   ├── main.rs                     # CLI entry point (clap)
│   ├── lib.rs                      # Library root, re-exports
│   ├── config.rs                   # Configuration and defaults (serde + toml)
│   ├── db.rs                       # SQLite schema, migrations, queries (rusqlite)
│   ├── graph.rs                    # Unified graph types (petgraph wrappers)
│   ├── builders/
│   │   ├── mod.rs
│   │   ├── structural.rs           # Layer 1: Tree-sitter graph builder
│   │   ├── change.rs               # Layer 2: Git log graph builder (gix)
│   │   └── defect.rs               # Layer 3: Defect graph builder (Phase 2)
│   ├── analysis/
│   │   ├── mod.rs
│   │   ├── signals.rs              # Cross-layer signal detection
│   │   ├── metrics.rs              # Node and graph metric computation
│   │   └── hotspots.rs             # Hotspot ranking (PageRank via petgraph)
│   ├── parsers/
│   │   ├── mod.rs
│   │   ├── treesitter.rs           # Tree-sitter wrapper, language dispatch
│   │   └── gitlog.rs               # Git log/blame parser (gix)
│   ├── server/
│   │   ├── mod.rs
│   │   └── mcp.rs                  # MCP server (axum + SSE)
│   └── export/
│       ├── mod.rs
│       ├── json.rs                 # JSON export (serde_json)
│       ├── dot.rs                  # Graphviz DOT export
│       └── mermaid.rs              # Mermaid diagram export
└── tests/
    ├── fixtures/                   # Sample repos for testing
    ├── structural_test.rs
    ├── change_test.rs
    ├── signals_test.rs
    └── cli_test.rs                 # Integration tests (assert_cmd)
```

-----

## 13. Key Design Decisions

1. **Rust for performance and single-binary distribution.** Tree-sitter is natively Rust, gix (gitoxide) is pure Rust, and the final product ships as one statically-linked binary — no runtime dependencies, no Python environment, instant startup. Target: `cargo install ising` just works.
1. **File-level granularity first, function-level optional.** File-level is sufficient for Layer 2 and 3 (git operates on files). Layer 1 extracts function-level detail but can roll up to file-level for cross-layer comparison.
1. **SQLite over Neo4j/Memgraph.** Zero dependency (rusqlite bundles SQLite), ships as single file, fast enough for repos under 10k files. Can migrate to graph DB later if needed.
1. **petgraph for in-memory graph, SQLite for persistence.** Build graph in petgraph (fast iteration, PageRank, cycle detection), persist to SQLite for query serving. Agent tools read from SQLite, analysis runs on petgraph.
1. **gix (gitoxide) over shelling out to git.** Pure Rust, no subprocess overhead, safe concurrent access to git objects. Parallel commit traversal via rayon for large repos.
1. **Offline-first, incremental-capable.** `build` does full analysis. Future `update` only re-processes files changed since last build (using git diff + file mtime).
1. **Thresholds are configurable.** All coupling thresholds, time windows, and severity cutoffs are in `ising.toml` with sensible defaults. Users can tune for their repo.
1. **No ML in MVP.** All signals are computed via deterministic graph algorithms and threshold-based rules. ML-based defect prediction is a Phase 3 concern.
1. **Parallelism via rayon.** File parsing (Tree-sitter) and commit traversal (gix) are embarrassingly parallel. Use rayon’s par_iter for both. Target: near-linear speedup on multi-core machines.
1. **Graph is append-only per build.** Each `build` creates a fresh graph. Historical comparison (is the codebase getting better or worse?) is Phase 2.

-----

## 14. Success Criteria

- `ising build` completes in < 5 seconds for a 1000-file Python/TS repo (Rust + rayon parallelism)
- `ising build` completes in < 30 seconds for a 10,000-file repo
- `ising impact <file>` returns in < 50ms from SQLite
- Single binary < 20MB, zero runtime dependencies
- Ghost coupling detection finds at least 1 non-obvious hidden dependency in a real repo (manual verification)
- Fragile boundary detection correlates with actual bug-prone interfaces (validate against git history)
- MCP integration works with Claude Code: Agent uses `ising_impact` before making changes

-----

## 15. References

- Adam Tornhill, *Your Code as a Crime Scene* (Pragmatic Programmers, 2015) — hotspot analysis, temporal coupling
- CodeScene documentation — temporal coupling algorithms, sum of coupling
- ICSE 2019: *Investigating the Impact of Multiple Dependency Structures on Software Defects* — multi-layer dependency validation
- CGCN (ISSRE 2021): AST + Class Dependency Network for defect prediction via GNN
- Aider RepoMap — Tree-sitter + PageRank for agent context selection
- code-review-graph — Tree-sitter graph for Claude Code with blast radius
- code-graph-rag — Knowledge graph + MCP for codebase RAG