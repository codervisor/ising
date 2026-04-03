---
status: draft
created: 2026-04-03
priority: critical
tags:
- architecture
- modularity
- boundary-analysis
- strategy
- false-positives
depends_on:
- '044'
- '042'
- '039'
created_at: 2026-04-03T12:00:00Z
updated_at: 2026-04-03T12:00:00Z
---

# Spec 046: Boundary-Aware Analysis — Strategic Redesign

## Motivation

Ising's current analysis framework is **flat**: every module is a peer, every signal fires
on individual file pairs, and risk propagates uniformly across all edges. This is the root
cause of systematic false positives that incremental fixes (spec 023, GAP-5, GAP-6, GAP-13)
cannot resolve.

### Evidence from validation

| Problem | Root Cause | Examples |
|---------|-----------|----------|
| 96 ghost couplings in Spring PetClinic | Same-package files co-change = normal, not ghost | `Owner.java` ↔ `Pet.java` in same package |
| 57 ghost couplings in React | Sibling files in same feature module co-change | feature flag files |
| 94 unnecessary abstractions in java-design-patterns | Delegation layers within a pattern are by-design | per-pattern thin wrappers |
| transformers/vllm rated D | Active development ≠ architectural risk | high churn but contained within subsystems |
| Flask `__init__.py` rated critical | Risk propagation ignores that re-exports are boundaries, not internals | SF=0.19 from propagation alone |

**Common thread**: all of these would be resolved if Ising understood module boundaries.
Intra-module co-change is healthy cohesion. Cross-boundary co-change is architectural risk.
The current model cannot distinguish them.

### What commercial tools do

Research into CodeScene, Sonargraph, Lattix, DV8, and Structure101 reveals a consistent
pattern: **none of them auto-detect modules**. They all:

1. **Accept existing directory/package/namespace structure as module boundaries** (ground truth)
2. **Measure boundary health** — coupling that crosses boundaries, temporal coupling across
   boundaries, risk that propagates across boundaries
3. **Flag boundary violations** — dependencies that shouldn't exist based on declared architecture
4. **Report per-boundary metrics** — not per-file, per-boundary

This is the correct approach. Auto-detection (Louvain, spectral clustering) has known
limitations: hub nodes get mis-assigned, utility modules are ambiguous, and the results
lack actionability because developers think in directories, not in algorithm-assigned clusters.

### The FEA analogy (from spec 034)

In finite element analysis, you don't report stress uniformly across a structure. You analyze
stress **at section boundaries** — joints, welds, material interfaces. A beam under internal
compression is fine; stress at a joint connecting two beams is where failures originate.

Directory boundaries are the software equivalent of structural joints.

## Strategic Shift

**FROM**: Flat graph → per-file risk → file-pair signals
**TO**: Boundary-aware graph → per-boundary stress → boundary health report

### What changes

| Aspect | Current | Proposed |
|--------|---------|----------|
| Module identity | None (flat graph) | Directory-derived boundaries |
| Ghost coupling | Fires on any co-changing pair without structural edge | Only fires on **cross-boundary** pairs without structural edge |
| Risk propagation | Uniform across all edges | Attenuated at boundary crossings |
| Health index | Zone fractions (count/N) | Boundary containment score |
| Signal priority | All signals equal weight | Cross-boundary signals weighted higher |
| Primary output | Per-file safety factor ranking | Per-boundary health report + per-file detail |

### What does NOT change

- Tree-sitter structural parsing (working well)
- Git history change graph construction (working well)
- Hotspot ranking (consistently accurate across all validation rounds)
- FEA core math (Jacobi iteration, safety factors)
- CLI/MCP interface

## Design

### Phase 1: Module Boundary Layer

Boundary detection uses a **resolution chain**: try strategies in order from most
authoritative to least, stop at the first one that produces a valid result.

```
Resolution chain:
  1. User-defined      ising.toml [boundaries.modules]     — always wins
  2. Build manifest    Cargo.toml / package.json / go.mod   — project's own declaration
  3. Directory fallback  file path prefix grouping           — last resort
```

Each strategy feeds into the same `BoundaryStructure` — downstream code (signals,
propagation, health index) is strategy-agnostic.

#### 1.1 Strategy 1: User-Defined (highest priority)

Users know their codebase best. Let them declare boundaries explicitly in `ising.toml`:

```toml
[boundaries]
# Explicit module definitions — takes precedence over auto-detection
modules = [
    { id = "auth",    glob = "src/auth/**" },
    { id = "billing", glob = "src/billing/**" },
    { id = "shared",  glob = "src/shared/**", role = "foundation" },
    { id = "api",     glob = "src/api/**",    role = "gateway" },
]

# Optional: declare allowed dependency direction
# (future extension, not in MVP)
# rules = [
#     "api -> auth",
#     "api -> billing",
#     "auth -> shared",
#     "billing -> shared",
# ]
```

`role` annotation is optional metadata:
- `foundation`: expected to be imported by everyone (suppress ghost coupling TO this module)
- `gateway`: expected to import many modules (suppress fan-out warnings)
- No role = normal module

**When to use**: monorepos with non-standard layouts, repos where directory structure
doesn't match logical modules, or when auto-detection produces unsatisfactory results.

**Validation**: after loading, check that declared globs cover >80% of source files.
Warn (don't fail) about uncovered files — they go into a synthetic `_uncategorized` module.

#### 1.2 Strategy 2: Build Manifest Detection (primary auto-detection)

Most ecosystems already declare module boundaries in their build system. These are
authoritative — they're what the compiler/bundler actually uses.

| Ecosystem | Manifest | Module = | Example |
|-----------|----------|----------|---------|
| Rust | `Cargo.toml` workspace `[workspace] members` | Each workspace member crate | `ising-core/`, `ising-analysis/` |
| JS/TS | `package.json` workspaces, `pnpm-workspace.yaml`, `lerna.json` | Each workspace package | `packages/auth/`, `packages/api/` |
| Go | `go.mod` per directory, or single `go.mod` + package directories | Each directory with `.go` files (Go package) | `internal/auth/`, `cmd/server/` |
| Python | `pyproject.toml` / `setup.py` / `src/` layout convention | Each top-level package under `src/` or root | `src/flask/`, `src/werkzeug/` |
| Java | `pom.xml` modules / `build.gradle` subprojects / `settings.gradle` | Each Maven module or Gradle subproject | `auth-service/`, `common/` |
| C# | `.sln` + `.csproj` per project | Each `.csproj` project | `AutoMapper/`, `AutoMapper.Extensions/` |

**Detection algorithm**:

```rust
pub fn detect_from_manifests(repo_root: &Path) -> Option<Vec<ManifestModule>> {
    // Try each ecosystem detector in order of reliability
    // Return first non-empty result

    // 1. Rust workspace
    if let Some(modules) = detect_cargo_workspace(repo_root) {
        return Some(modules);
    }

    // 2. JS/TS workspace (pnpm > lerna > package.json workspaces)
    if let Some(modules) = detect_js_workspaces(repo_root) {
        return Some(modules);
    }

    // 3. Go modules
    if let Some(modules) = detect_go_modules(repo_root) {
        return Some(modules);
    }

    // 4. Python packages
    if let Some(modules) = detect_python_packages(repo_root) {
        return Some(modules);
    }

    // 5. Java/Maven/Gradle
    if let Some(modules) = detect_jvm_modules(repo_root) {
        return Some(modules);
    }

    // 6. C# solution
    if let Some(modules) = detect_dotnet_projects(repo_root) {
        return Some(modules);
    }

    None // fall through to directory fallback
}
```

**Concrete examples from Ising's own validation repos**:

| Repo | Manifest | Detected Modules |
|------|----------|-----------------|
| `ising` (self) | `Cargo.toml` workspace: `["ising-core", "ising-builders", ...]` | 7 crates |
| `react` | Root `package.json` workspaces: `["packages/*"]` | ~30 packages (react, react-dom, scheduler, ...) |
| `spring-boot` | `settings.gradle`: subprojects | ~20 modules |
| `langchain` | `pyproject.toml` per package in `libs/` | langchain-core, langchain-community, ... |
| `kubernetes` | `go.mod` per component in `staging/src/k8s.io/` | ~30 components |
| `axum` | `Cargo.toml` workspace members | axum, axum-core, axum-extra, axum-macros |

Each of these is already a solved problem — the build system declares exactly what
the module boundaries are. We just need to read the manifests.

**Implementation priority**: Rust (Cargo) and JS/TS (package.json/pnpm) first,
since these cover Ising itself and the largest set of validation repos. Python (pyproject.toml)
and Go (go.mod) second. Java/C# third.

**Limitation**: workspace manifests only give Level 1 boundaries. For single-package repos
(Flask, gin, Express — most of our validation set), this produces just ONE module = the whole
repo. We need a second level.

#### 1.3 Strategy 2b: Intra-Package Module Detection (language module systems)

Within a single package/crate, every language has its own way to declare internal modules.
These declarations are as authoritative as workspace manifests — they're what the compiler
or runtime uses to resolve imports.

| Language | Module declaration mechanism | Boundary = |
|----------|---------------------------|------------|
| Rust | `mod auth;` in `lib.rs` → resolves to `auth.rs` or `auth/mod.rs` | Each `mod` declaration creates a module boundary |
| Python | Directory with `__init__.py` | Each `__init__.py` directory is a sub-package |
| Go | Each directory = one Go package (package declaration in `.go` files) | Each directory with `.go` files |
| Java | Package hierarchy: `com.example.auth.*` maps to `com/example/auth/` | Each package directory under source root |
| TS/JS | Directory with barrel file (`index.ts` / `index.js`) | Each directory with a barrel file |
| C# | Namespace + directory convention | Each directory with `.cs` files (namespace usually matches) |

**Concrete examples from validation repos**:

```
Flask (single Python package):
  flask/
    __init__.py       → module "flask" (root)
    app.py            → member of "flask"
    blueprints.py     → member of "flask"
    json/
      __init__.py     → module "flask.json" (sub-package boundary)
      provider.py     → member of "flask.json"
    sansio/
      __init__.py     → module "flask.sansio"

→ Detected boundaries: flask, flask.json, flask.sansio
→ app.py ↔ blueprints.py = same module (intra-module co-change is normal)
→ app.py ↔ json/provider.py = cross-boundary (worth investigating)
```

```
gin (single Go module):
  gin.go              → package "gin" (root)
  context.go          → member of "gin"
  binding/
    binding.go        → package "binding" (sub-package boundary)
    json.go           → member of "binding"
  render/
    render.go         → package "render"

→ Detected boundaries: gin, binding, render
→ gin.go ↔ context.go = same module
→ gin.go ↔ binding/binding.go = cross-boundary
```

```
axum (Rust, single crate axum/):
  lib.rs
    mod routing;      → module "routing" (boundary)
    mod extract;      → module "extract" (boundary)
  routing/
    mod.rs
      mod method_routing;  → module "routing::method_routing"
    method_routing.rs
  extract/
    mod.rs
      mod path;       → module "extract::path"

→ Detected boundaries: root, routing, extract, routing::method_routing, extract::path
→ routing/mod.rs ↔ routing/method_routing.rs = same module (parent contains child)
→ routing/mod.rs ↔ extract/mod.rs = cross-boundary
```

**Detection approach**: this doesn't need new parsing — the information already exists:

1. **Rust**: `mod` declarations are import edges from `lib.rs`/`mod.rs` to child files.
   The structural graph already has these as `Imports` edges. Module tree = the `mod`
   declaration tree rooted at `lib.rs`.
2. **Python**: scan for `__init__.py` files. Each directory containing one = a sub-package.
3. **Go**: each unique directory containing `.go` files = one Go package. Already tracked
   by `is_go_intra_package_pair()` — we're generalizing this.
4. **Java**: each unique directory under the source root = one Java package.
5. **TS/JS**: directories containing `index.ts`/`index.js` = module with barrel file.
   Directories without = flat namespace (group with parent).
6. **C#**: each unique directory containing `.cs` files = approximate namespace boundary.

**Fallback for languages without clear module systems** (C/C++): use directory grouping,
same as Strategy 3.

#### 1.4 Hierarchical Boundaries: Two Levels

The two strategies compose naturally:

```
Level 1 (workspace): Cargo workspace / npm workspaces / go.work / single-package root
Level 2 (intra-package): mod declarations / __init__.py dirs / Go package dirs
```

For a monorepo with workspace manifests:
```
kubernetes/
  staging/src/k8s.io/
    api/           ← Level 1 boundary (go.mod)
      core/v1/     ← Level 2 boundary (Go package dir)
      apps/v1/     ← Level 2 boundary
    client-go/     ← Level 1 boundary (go.mod)
      rest/        ← Level 2 boundary
      tools/       ← Level 2 boundary
```

For a single-package repo:
```
flask/             ← Level 1 = whole repo (single package)
  flask/           ← Level 2 boundary (__init__.py)
  flask/json/      ← Level 2 boundary (__init__.py)
  flask/sansio/    ← Level 2 boundary (__init__.py)
  tests/           ← Level 2 boundary (test dir, excluded from analysis)
```

**Signal scoping uses both levels**:
- Cross-Level-1 signals (between workspace packages): highest severity
- Cross-Level-2 signals (between intra-package modules): elevated severity
- Intra-Level-2 signals (within a single module): lowest severity / suppressed

This solves the single-package problem while keeping monorepo analysis intact.

#### 1.5 Strategy 3: Directory Fallback (lowest priority)

When no manifest is found, no language module system applies, AND no user config exists,
fall back to directory grouping.

```rust
pub fn detect_from_directories(node_ids: &[&str]) -> Vec<DirectoryModule> {
    // 1. Strip common source prefix (src/, lib/, app/, internal/, pkg/)
    // 2. Group by first directory component after prefix
    // 3. Require minimum 2 files per group (singletons → _uncategorized)
    // 4. If result is <3 groups, try depth+1
}
```

This is the weakest strategy and should be explicitly flagged in output:

```
⚠ No build manifest or language module system found. Using directory-based boundary detection.
  Consider adding [boundaries.modules] to ising.toml for more accurate results.
```

#### 1.6 Unified BoundaryStructure

All strategies produce the same hierarchical output:

```rust
pub struct BoundaryStructure {
    /// How Level 1 boundaries were detected
    pub l1_source: BoundarySource,
    /// How Level 2 boundaries were detected
    pub l2_source: BoundarySource,

    /// Level 1: workspace / package boundaries
    pub packages: Vec<PackageInfo>,

    /// Level 2: intra-package module boundaries
    /// Map from node_id → (package_id, module_id)
    pub assignments: HashMap<String, (String, String)>,

    /// Files not assigned to any module
    pub uncategorized: Vec<String>,
}

pub enum BoundarySource {
    /// User declared in ising.toml
    UserDefined,
    /// Auto-detected from build manifests
    Manifest { ecosystem: String },
    /// Detected from language module system
    LanguageModules { language: String },
    /// Fallback: directory prefix grouping
    Directory,
    /// Not applicable (e.g., single-package repo has no L1 differentiation)
    SingleRoot,
}

pub struct PackageInfo {
    pub id: String,
    pub root_path: String,
    pub role: Option<ModuleRole>,
    /// Level 2 modules within this package
    pub modules: Vec<ModuleInfo>,
}

pub struct ModuleInfo {
    pub id: String,
    /// File paths belonging to this module
    pub members: Vec<String>,
    /// How this module was detected
    pub detection: ModuleDetection,
}

pub enum ModuleDetection {
    /// Rust `mod` declaration in parent
    RustMod,
    /// Python `__init__.py` directory
    PythonPackage,
    /// Go package directory
    GoPackage,
    /// Java package directory
    JavaPackage,
    /// TS/JS barrel file (index.ts/index.js)
    BarrelFile,
    /// C# directory with .cs files
    CSharpNamespace,
    /// Directory grouping fallback
    Directory,
    /// User-defined
    UserDefined,
}

pub enum ModuleRole {
    /// Core/shared/utils — expected high fan-in, suppress fan-in warnings
    Foundation,
    /// API gateway/CLI entry — expected high fan-out
    Gateway,
    /// Normal module
    Normal,
}

impl BoundaryStructure {
    /// Resolution chain:
    /// L1: user config → workspace manifest → single root
    /// L2: user config → language module system → directory fallback
    pub fn detect(
        repo_root: &Path,
        node_ids: &[&str],
        config: &Config,
    ) -> Self {
        // Level 1: workspace boundaries
        let packages = if let Some(user_modules) = &config.boundaries {
            Self::l1_from_user_config(user_modules, node_ids)
        } else if let Some(ws) = detect_workspace_manifests(repo_root) {
            Self::l1_from_workspace(ws, node_ids)
        } else {
            Self::l1_single_root(node_ids)
        };

        // Level 2: intra-package modules (for each L1 package)
        for package in &mut packages {
            package.modules = detect_intra_package_modules(
                repo_root, &package.root_path, &package.member_files,
            );
        }

        Self::build(packages, node_ids)
    }

    /// Are two nodes in the same Level 2 module?
    pub fn same_module(&self, a: &str, b: &str) -> bool {
        self.assignments.get(a) == self.assignments.get(b)
    }

    /// Are two nodes in the same Level 1 package?
    pub fn same_package(&self, a: &str, b: &str) -> bool {
        let pkg_a = self.assignments.get(a).map(|(p, _)| p);
        let pkg_b = self.assignments.get(b).map(|(p, _)| p);
        pkg_a == pkg_b
    }

    /// Classify the boundary crossing type for a pair of nodes.
    pub fn crossing_type(&self, a: &str, b: &str) -> CrossingType {
        if self.same_module(a, b) {
            CrossingType::SameModule
        } else if self.same_package(a, b) {
            CrossingType::CrossModule  // same package, different module
        } else {
            CrossingType::CrossPackage // different packages entirely
        }
    }

    pub fn module_of(&self, node_id: &str) -> (&str, &str) {
        self.assignments
            .get(node_id)
            .map(|(p, m)| (p.as_str(), m.as_str()))
            .unwrap_or(("_uncategorized", "_uncategorized"))
    }
}

/// Three-level classification for signal severity scaling.
pub enum CrossingType {
    /// Both nodes in same L2 module — co-change is expected, suppress most signals
    SameModule,
    /// Same L1 package, different L2 modules — moderate concern
    CrossModule,
    /// Different L1 packages — highest concern, architectural boundary violation
    CrossPackage,
}
```

**Signal severity scaling by crossing type**:

```rust
fn severity_multiplier(crossing: &CrossingType) -> f64 {
    match crossing {
        CrossingType::SameModule   => 0.0,  // suppress
        CrossingType::CrossModule  => 1.0,  // normal
        CrossingType::CrossPackage => 2.0,  // elevated
    }
}
```

#### 1.5 CLI Integration

```bash
# Show detected boundaries and which strategy was used
$ ising boundaries
Boundary source: Manifest (cargo-workspace)
Modules: 7

  ising-core        12 files   role: foundation
  ising-builders    18 files
  ising-analysis     4 files
  ising-db           6 files
  ising-cli          3 files
  ising-server       3 files
  ising-scip         4 files
  _uncategorized     2 files   ⚠

# Override: use directory fallback even if manifest exists
$ ising boundaries --strategy directory

# Validate user config
$ ising boundaries --validate
✓ 48/50 files covered by [boundaries.modules]
⚠ 2 files uncategorized: scripts/bench.sh, README.md
```

### Phase 2: Boundary-Aware Signal Detection

Every signal detector gets a `BoundaryStructure` parameter. Signals are classified as:

```rust
pub enum SignalScope {
    /// Both nodes in the same module — lower priority, often expected
    IntraModule,
    /// Nodes in different modules — higher priority, architectural concern
    CrossBoundary,
}
```

#### 2.1 Ghost Coupling → Cross-Boundary Ghost Coupling

Current: fires on any co-changing pair without structural edge.
Proposed: **only fires on cross-boundary pairs**.

Intra-module co-change without structural edge is almost always explained by shared parent
or sibling relationship — this is what spec 023 tried to fix case-by-case. The boundary
filter eliminates the entire class of FPs at once.

```rust
fn detect_ghost_coupling(
    co_change_edges: &[(&str, &str, f64)],
    import_edges: &[(&str, &str, f64)],
    graph: &UnifiedGraph,
    boundaries: &BoundaryStructure,    // NEW
    thresholds: &ThresholdConfig,
) -> Vec<Signal> {
    for (a, b, coupling) in co_change_edges {
        // Keep existing filters...
        if graph.has_structural_edge(a, b) || *coupling <= threshold { continue; }

        // NEW: classify by boundary crossing type
        let crossing = boundaries.crossing_type(a, b);
        let multiplier = severity_multiplier(&crossing);
        if multiplier == 0.0 { continue; }  // SameModule → suppress

        let (pkg_a, mod_a) = boundaries.module_of(a);
        let (pkg_b, mod_b) = boundaries.module_of(b);

        let scope_desc = match crossing {
            CrossingType::CrossPackage => format!("Cross-package ({} ↔ {})", pkg_a, pkg_b),
            CrossingType::CrossModule => format!("Cross-module ({} ↔ {}) in {}", mod_a, mod_b, pkg_a),
            CrossingType::SameModule => unreachable!(),
        };

        signals.push(Signal::new(
            SignalType::GhostCoupling,
            a, Some(b),
            coupling * multiplier,
            format!(
                "{}. {:.0}% co-change with no structural dependency.",
                scope_desc, coupling * 100.0
            ),
        ));
    }
}
```

**Expected impact**: eliminates 50-80% of ghost coupling FPs based on validation data.
Spring PetClinic's 96 signals → likely <20 (only cross-package pairs remain).

#### 2.2 Unnecessary Abstraction: Boundary-Aware

Current: fires when B has fan-in=1 and low complexity.
Proposed: only fires for **intra-module** single-consumer wrappers.

Cross-module thin wrappers (adapter, facade) are intentional architecture — they exist
precisely to create a clean boundary. Flagging them is a false positive.

```rust
// In detect_unnecessary_abstraction:
let crossing = boundaries.crossing_type(a, b);
if matches!(crossing, CrossingType::CrossPackage | CrossingType::CrossModule) {
    continue; // Cross-boundary thin wrapper = intentional adapter, not unnecessary
}
```

#### 2.3 Fragile Boundary: Elevated to Primary Signal

Current: fires on structural dep + co-change + fault propagation.
Proposed: **cross-boundary fragile boundaries become the #1 priority signal**.

A fragile boundary within a module is a local code quality issue. A fragile boundary
between modules is an architectural problem. Weight accordingly:

```rust
let crossing = boundaries.crossing_type(a, b);
let severity_multiplier = match crossing {
    CrossingType::CrossPackage => 3.0,  // worst: inter-package fragility
    CrossingType::CrossModule => 1.5,   // bad: inter-module fragility
    CrossingType::SameModule => 0.5,    // minor: local code quality
};
```

#### 2.4 New Signal: Boundary Leakage

Replace scattered ghost coupling/shotgun surgery signals with a single boundary-level signal:

```rust
SignalType::BoundaryLeakage
```

**Definition**: Module A has >N% of its change edges crossing into module B, but the
structural coupling between A and B is low. This means A's changes routinely require
changes in B through some non-obvious path.

This is the signal that CodeScene, DV8, and Sonargraph all converge on: **unexpected
cross-boundary coupling detected from change history**.

### Phase 3: Boundary Health Metrics

Instead of per-file safety factors as the primary output, compute per-boundary metrics:

```rust
pub struct BoundaryHealth {
    pub module_id: String,

    // --- Containment ---
    /// What fraction of this module's change edges stay within the module?
    /// 1.0 = perfect containment, 0.0 = all changes leak out
    pub containment_ratio: f64,

    // --- Interface quality ---
    /// Cross-boundary structural edges / total structural edges
    /// Lower = more encapsulated
    pub coupling_ratio: f64,

    /// Cross-boundary temporal coupling / total temporal coupling
    /// If this >> coupling_ratio, there's hidden cross-boundary dependency
    pub temporal_leakage: f64,

    // --- Internal health ---
    /// Fraction of internal nodes in Critical/Danger zone
    pub internal_stress: f64,

    /// Number of internal dependency cycles
    pub internal_cycles: usize,

    // --- Risk propagation ---
    /// How much risk originating inside this module propagates out?
    pub risk_export: f64,

    /// How much external risk propagates into this module?
    pub risk_import: f64,
}
```

**Key insight from DV8**: the two most validated architecture-level metrics are
**Decoupling Level** (how well modules are separated) and **Propagation Cost**
(how far changes travel). Both are boundary-level metrics, not file-level metrics.

`containment_ratio` ≈ DV8's Decoupling Level.
`risk_export + risk_import` ≈ DV8's Propagation Cost.

### Phase 4: Health Index Redesign

Replace zone fractions with boundary health as the primary scoring axis:

```
Old: score = zone_sub_score × coupling_modifier − signal_penalty
New: score = boundary_health_score × internal_quality_score − signal_penalty
```

Where:
- `boundary_health_score` = weighted average of all modules' containment_ratio
  (weighted by module size). This directly measures "is risk contained within modules?"
- `internal_quality_score` = current zone fractions, but computed per-module and aggregated.
  A module with 40% critical files that are all contained internally is less concerning
  than a module with 10% critical files that all leak across boundaries.
- `signal_penalty` = only counts cross-boundary signals. Intra-module signals are reported
  but don't affect the health grade.

**This solves the transformers/vllm problem**: high internal churn with good containment
→ high boundary_health_score → grade reflects architecture quality, not development velocity.

**This solves the large repo inflation problem**: boundary_health_score is not count/N.
A 40K-file repo with 5 leaky boundaries scores worse than a 40K-file repo with clean
boundaries, regardless of the percentage of critical files.

### Phase 5: Boundary-Aware Risk Propagation

Current: risk propagates uniformly along all edges.
Proposed: risk attenuates when crossing module boundaries.

```rust
// In propagate_risk(), when computing neighbor contribution:
let boundary_attenuation = if boundaries.crosses_boundary(source, target) {
    config.fea.boundary_attenuation  // default: 0.3
} else {
    1.0  // full propagation within module
};
let contribution = neighbor_risk * edge_weight * damping * boundary_attenuation;
```

**Rationale**: In well-designed systems, module boundaries are firewalls. Risk inside
`auth/` should not freely propagate into `billing/`. If it does (high cross-boundary
coupling), that's what the boundary health metrics detect. But the propagation model
itself should respect boundaries.

**This solves the Flask __init__.py problem**: re-export modules sit at boundaries.
With attenuation, risk from child modules propagates out at 30% instead of 100%.

## Implementation Plan

### Part 1: BoundaryStructure (ising-core) — ~500 lines

1. Add `boundary.rs` to `ising-core/src/`
2. Core types: `BoundaryStructure`, `PackageInfo`, `ModuleInfo`, `CrossingType`, `BoundarySource`
3. Resolution chain: `detect()` dispatching to L1 and L2 strategies
4. **L1 — Workspace manifest detectors** (priority order):
   - Cargo workspace (`Cargo.toml` → `[workspace] members`)
   - JS/TS workspaces (`pnpm-workspace.yaml`, `package.json` workspaces)
   - Go workspaces (`go.work`, or multiple `go.mod`)
   - Python multi-package (`pyproject.toml` per package in `libs/` or `packages/`)
   - JVM modules (`pom.xml` modules, `settings.gradle` subprojects)
   - .NET solutions (`.sln` + `.csproj`)
   - Single-root fallback (whole repo = one L1 package)
5. **L2 — Intra-package module detectors**:
   - Rust: `mod` declaration tree from import edges rooted at `lib.rs`
   - Python: directories with `__init__.py`
   - Go: directories with `.go` files
   - Java: package directories under source root
   - TS/JS: directories with barrel files (`index.ts`/`index.js`)
   - C#: directories with `.cs` files
   - Directory fallback for unrecognized languages
6. User override via `[boundaries]` in `ising.toml`
7. `ising boundaries` CLI command with `--strategy` flag
8. `crossing_type(a, b)` → `SameModule | CrossModule | CrossPackage`
9. Tests: Ising self-analysis, Flask layout, axum layout, Spring PetClinic layout

### Part 2: Signal Detection Refactor (ising-analysis) — ~200 lines changed

1. Pass `BoundaryStructure` into `detect_signals()`
2. Ghost coupling: use `crossing_type()` — SameModule suppressed, CrossModule normal, CrossPackage elevated
3. Unnecessary abstraction: skip CrossModule/CrossPackage (intentional adapters)
4. Fragile boundary: severity scaled by crossing type (3× for CrossPackage)
5. All signals: add `crossing: CrossingType` field to `Signal` struct
6. New signal: `BoundaryLeakage` detector (module-level cross-boundary coupling metric)
7. **Remove** these ad-hoc helpers (subsumed by `crossing_type()`):
   - `is_go_intra_package_pair()` → `SameModule` for Go files in same dir
   - `is_cross_crate_pair()` → `CrossPackage` for files in different workspace members
   - Common-parent suppression block in `detect_ghost_coupling()` → `SameModule` handles it
8. Tests: boundary-aware signals for known scenarios

### Part 3: Boundary Health Metrics (ising-analysis) — ~200 lines new

1. Add `boundary_health.rs` to `ising-analysis/src/`
2. Compute `BoundaryHealth` for each module
3. Aggregate into `BoundaryHealthReport`
4. Store in SQLite (new table `boundary_health`)

### Part 4: Health Index Integration (ising-analysis/stress.rs) — ~100 lines changed

1. `compute_health_index()` takes `BoundaryHealthReport`
2. New formula: `boundary_health_score × internal_quality_score − signal_penalty`
3. Signal penalty only counts cross-boundary signals

### Part 5: Risk Propagation Attenuation (ising-analysis/stress.rs) — ~30 lines changed

1. `propagate_risk()` takes `BoundaryStructure`
2. Apply `boundary_attenuation` to cross-boundary edges

### Part 6: CLI Output (ising-cli) — ~100 lines changed

1. `ising boundaries` command: show boundary health report
2. `ising signals` output: add scope column (intra/cross)
3. `ising safety` output: group by module, show containment ratio

### Part 7: Validation — mandatory

Re-run full 27-repo benchmark after each part:
- Part 2: ghost coupling count should drop 50%+ across all repos
- Part 4: transformers/vllm should improve from D; flask should not get worse
- Part 5: Flask `__init__.py` should drop from critical

## Acceptance Criteria

- [ ] `ising boundaries` command shows module structure with health metrics
- [ ] Ghost coupling FP rate drops >50% (measured against Spring PetClinic, React baseline)
- [ ] transformers grade improves from D to C or better
- [ ] Flask `__init__.py` drops from critical zone
- [ ] No regression: hotspot top-3 accuracy maintained across all repos
- [ ] No regression: true positive signals (Flask-Admin god modules, React shotgun surgery) maintained
- [ ] 27-repo benchmark automated and passing

## What This Does NOT Do

- **No auto-detection of module boundaries via graph algorithms** — no Louvain, no spectral
  clustering, no label propagation. Boundaries come from build manifests (authoritative),
  user config (explicit), or directory structure (fallback). This is the validated commercial
  approach and avoids the hub-node / false-cluster problems demonstrated in Louvain experiments.
- **No architecture rule enforcement (yet)** — unlike Sonargraph, we don't enforce
  allowed/forbidden cross-boundary dependencies in MVP. The `rules` config key is reserved
  for future extension. MVP focuses on measurement and detection, not enforcement.
- **No replacement of per-file metrics** — safety factors, hotspots, and per-file risk remain.
  Boundary analysis is an additional layer on top, providing architectural context for the
  existing per-file data.

## Relation to Spec 044

Spec 044 (Modularity Analysis) proposed label propagation for community detection. This spec
supersedes that approach based on the finding that commercial tools universally avoid
auto-detection. The `CommunityStructure` from spec 044 is replaced by `BoundaryStructure`
using directory paths. The boundary health metrics in Phase 3 incorporate spec 044's
containment analysis concept but ground it in real (not inferred) module boundaries.
