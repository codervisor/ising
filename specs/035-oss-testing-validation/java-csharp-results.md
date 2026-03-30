# OSS Validation: Java and C# Language Support

> Run date: 2026-03-30 | Ising version: post spec-035 + Java/C# language support
> Method: `--depth=500` shallow clones, `--since "12 months ago"` git history

## Summary

Added Java and C# to the OSS validation test matrix following spec-025 (Java) and spec-026 (C#) implementation. Also fixed a structural import resolution bug discovered during this run.

## Bug Fixed: Java/C# Inter-File Import Edges

**Problem**: Java and C# showed zero inter-file structural edges. The namespace-to-path conversion produced paths like `com/example/Foo.java` that don't match Maven/project paths (`src/main/java/com/example/Foo.java`), and C# `using` directives similarly failed exact-match resolution.

**Fix**: Added suffix-index-based resolution in `ising-builders/src/structural.rs`. For `.java` and `.cs` imports, if exact match fails, find any module whose path ends with `/{import_path}`. This correctly resolves Maven-layout Java projects and C# projects where namespace components match file names.

**Impact**:
- Spring PetClinic: 0 → 22 inter-file import edges detected
- java-design-patterns: 0 → 922 inter-file import edges detected
- AutoMapper: 0 → 14 inter-file import edges detected
- NUnit: 0 import edges (expected — C# namespace-level `using` directives don't map to single files)

Two tests added: `test_java_maven_import_resolution`, `test_csharp_import_resolution`. Total test count: 143 → 145.

## Repositories Analyzed

| Repository | Language | Nodes | Struct Edges (imports) | Change Edges | Signals | Critical / Danger | Build Time |
|---|---|---|---|---|---|---|---|
| **spring-projects/spring-petclinic** | Java | 237 | 22 | 125 | 111 | 26 / 2 | ~0.9s |
| **iluwatar/java-design-patterns** | Java | 7,758 | 922 | 120 | 212 | 17 / 2 | ~7.5s |
| **AutoMapper/AutoMapper** | C# | 1,772 | 14 | 0 | 1 | 6 / 4 | ~1.4s |
| **nunit/nunit** | C# | 1,102 | 0 | 10 | 4 | 7 / 3 | ~1.5s |

## Results by Repository

### Spring PetClinic (Java, 237 nodes)

- `PetController.java` correctly #1 hotspot and critical (SF=0.14): highest change frequency + highest complexity in the owner package.
- `Owner.java` and `OwnerController.java` correctly flagged critical — both are highly connected domain objects.
- 1 dependency cycle detected in test classes (`OwnerControllerTests` ↔ `PetClinicIntegrationTests`) — plausible, test integration suites cross-reference each other.
- 96 ghost couplings, mostly cross-package pairs that co-change during feature additions. Expected for a small app where all domain objects change together.
- 13 shotgun surgery signals on domain entities — correct, Spring PetClinic has thin model objects that ripple across layers.
- 26/47 modules critical (55%) — high ratio. Reflects a small, tightly-coupled demo app where every object connects to every other.
- **Verdict**: Java parsing works. Hotspot identification correct. High critical ratio expected for a 200-node demo app.

### java-design-patterns (Java, 7,758 nodes)

- 922 import edges across 250+ design pattern sub-projects — suffix matching correctly resolves intra-pattern imports.
- 11 dependency cycles detected, all within individual pattern implementations (e.g., `PhysicComponent` ↔ `ObjectPhysicComponent` in the Component pattern). These are real cycles in the sample code illustrating design trade-offs.
- 94 UnnecessaryAbstraction signals — expected for a patterns repo, many patterns have thin delegation layers.
- 92 ghost couplings — patterns often change together during style/formatting updates.
- `clean-architecture/` module cluster dominates the critical list with SF~0.20. High propagated risk from co-change. Accurately reflects that clean-architecture example has many tightly coupled classes.
- `FlatFileCustomerDAO.java` is #1 hotspot (complexity=51, freq=1) — correctly identified as the most complex file in the repo.
- **Verdict**: Good Java parsing at scale (7.5s for 7758 nodes). Signal output rich and plausible.

### AutoMapper (C#, 1,772 nodes)

- Zero change edges from 128 commits in the last 12 months — co-change thresholds too high even after gap-2 fix. AutoMapper has very focused, single-file commits.
- 14 import edges via suffix matching. Limited because AutoMapper's `using` directives reference file-level namespaces (`AutoMapper.Features`) where `Features.cs` exists as a file.
- `TypeMapPlanBuilder.cs` correctly #1 hotspot (complexity=103, freq=5) and critical — the central mapping plan construction class.
- `ProfileMap.cs` and `MappingExpression.cs` correctly in top critical — core configuration classes.
- 1 StableCore signal: `Features.cs` — correctly identified as a highly imported interface definition.
- Only 6 critical modules (1.2%) — appropriate for a well-maintained library.
- **Verdict**: C# parsing works. Structural edges limited by namespace-vs-file mismatch but suffix matching captures file-level namespaces. Co-change signal unavailable for this low-churn library.

### NUnit (C#, 1,102 nodes)

- 0 inter-file import edges: NUnit's `using NUnit.Framework.Interfaces` refers to a *namespace directory* (`Interfaces/`), not a single file. Suffix matching cannot resolve namespace-level imports to specific files. This is a known C# limitation.
- `ArgumentsExtensions.cs` and `TestCaseParameters.cs` correctly flagged critical — these are core parameter resolution utilities with high fan-in.
- 4 ghost coupling signals are plausible — test attributes and their implementation tests co-change.
- 7 critical modules (0.7%) — appropriate for a mature testing framework.
- **Verdict**: C# parsing works for node extraction. Import edges limited to file-name-matching namespaces. NUnit's namespace-directory pattern not covered.

## Safety Zone Distribution

| Repo | Critical | Danger | Warning | Healthy | Stable | Total Modules |
|---|---|---|---|---|---|---|
| Spring PetClinic | 26 (55%) | 2 (4%) | 3 (6%) | 0 | 16 (34%) | 47 |
| java-design-patterns | 17 (1%) | 2 (<1%) | 4 (<1%) | 2 (<1%) | 1,852 (99%) | 1,877 |
| AutoMapper | 6 (1.2%) | 4 (0.8%) | 3 (0.6%) | 6 (1.2%) | 492 (96%) | 511 |
| NUnit | 7 (0.7%) | 3 (0.3%) | 2 (0.2%) | 11 (1.1%) | 1,031 (97%) | 1,054 |

## Signal Distribution

| Signal Type | Spring PetClinic | java-design-patterns | AutoMapper | NUnit | Total |
|---|---|---|---|---|---|
| DependencyCycle | 1 | 11 | 0 | 0 | 12 |
| GodModule | 0 | 0 | 0 | 0 | 0 |
| GhostCoupling | 96 | 92 | 0 | 4 | 192 |
| ShotgunSurgery | 13 | 14 | 0 | 0 | 27 |
| UnnecessaryAbstraction | 1 | 94 | 0 | 0 | 95 |
| StableCore | 0 | 1 | 1 | 0 | 2 |
| **Total** | **111** | **212** | **1** | **4** | **328** |

## Top 3 Hotspots Per Repo (excl. tests)

| Repo | #1 | #2 | #3 |
|---|---|---|---|
| Spring PetClinic | owner/PetController.java | owner/Owner.java | owner/OwnerController.java |
| java-design-patterns | daofactory/FlatFileCustomerDAO.java | rate-limiting/App.java | daofactory/H2CustomerDAO.java |
| AutoMapper | Execution/TypeMapPlanBuilder.cs | TypeMap.cs | QueryableExtensions/ProjectionBuilder.cs |
| NUnit | benchmarks/ParamAttributeTypeConversionsBenchmark.cs | Comparers/IgnoreLineEndingFormatStringComparer.cs | TestResult.cs |

## New Gap Identified

**GAP-12: C# namespace-level `using` directives cannot resolve to specific files.** `using NUnit.Framework.Interfaces` refers to a namespace directory, not a single `.cs` file. Suffix matching only works when the last namespace component is a class name that matches a file (e.g., `using AutoMapper.Features` → `Features.cs`). For repos that group classes in namespace-named directories, structural import edges are unavailable.

Fix options:
1. Build a class-name-to-file index and match the last namespace component against class names in all files.
2. Treat the namespace as a directory prefix and create edges to all `.cs` files under the matching directory (similar to the Go directory-based resolution).

This is P2 — structural edges are still partially available, co-change edges still work, and risk scores remain meaningful.

## What Works Well for Java/C#

1. **Node extraction**: Functions, classes, complexity scores correct for both languages.
2. **Change edge detection**: Java repos with moderate churn produce good co-change data.
3. **Hotspot ranking**: Consistently identifies the most complex, frequently-changed files.
4. **Signal detection**: Dependency cycles, shotgun surgery, ghost coupling all fire correctly on Java.
5. **Scale**: 7,758-node Java repo builds in 7.5s — good performance.
6. **Suffix-based import resolution**: Correctly handles Maven directory structure and file-level C# namespaces.
