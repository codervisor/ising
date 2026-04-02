# Spec 044: Modularity Analysis — Risk Containment Detection

**Status**: Draft
**Created**: 2026-04-02
**Priority**: High
**Depends on**: Spec 034 (structural force analysis), Spec 039 (signal-aware health)

## Motivation

The current risk model treats the codebase graph as **flat**. Every module is a peer,
and risk propagates uniformly along edges regardless of whether modules are in the same
logical subsystem or across architectural boundaries. This creates two systematic errors:

1. **Contained risk is over-penalized**: If `auth/login.py`, `auth/session.py`, and
   `auth/tokens.py` all churn heavily and co-change frequently, they inflate each other's
   propagated risk. But if the auth subsystem has a clean interface boundary, that risk
   is *contained* — it doesn't threaten the rest of the system. The current model can't
   distinguish this from systemic risk.

2. **Cross-boundary risk is under-penalized**: A change in `auth/` that propagates into
   `billing/` through tight coupling is far more dangerous than internal auth churn, but
   the current model treats both propagation paths identically.

3. **The coupling modifier (λ/√N) is too blunt**: It measures global graph coupling density,
   but what matters is *where* coupling exists. A codebase with tight intra-cluster coupling
   but clean inter-cluster boundaries is fundamentally different from one with sprawling
   cross-boundary dependencies — yet both can have similar λ/√N.

### FEA Analogy

In real finite element analysis, you don't just compute stress everywhere uniformly.
You analyze stress at **section boundaries** — joints, welds, material interfaces.
A beam under internal compression is fine; stress at a joint connecting two beams is
where failures originate. Community boundaries are the software equivalent of structural
joints.

## Design

### Phase 1: Community Detection

Detect natural module clusters (communities) in the codebase graph. These represent
subsystems where code is tightly interconnected internally but loosely connected externally.

#### Algorithm: Label Propagation

Use label propagation on the combined Import + CoChanges adjacency graph:

1. Initialize each node with a unique label (its own ID).
2. Iteratively: each node adopts the most frequent label among its neighbors (weighted by
   edge weight). Ties broken randomly.
3. Converge when no node changes its label.

**Why label propagation over Louvain?**
- O(E) time complexity — fast even for large codebases
- No resolution parameter to tune (Louvain's resolution affects cluster granularity)
- Naturally uses edge weights (Import weight + CoChange weight)
- Deterministic with tie-breaking by label value (reproducible results)

**Edge weight combination for community detection:**
```
combined_weight(A, B) = import_weight(A, B) * α + cochange_weight(A, B) * β
```
Where α = 0.6 (structural coupling matters more for boundaries) and β = 0.4.

**Fallback**: If label propagation produces degenerate results (one giant community or
all singletons), fall back to directory-based clustering using file path prefixes. This
is a natural heuristic — `src/auth/*` files likely form a logical subsystem.

#### Output: `CommunityStructure`

```rust
pub struct CommunityStructure {
    /// Map from node_id to community_id
    pub assignments: HashMap<String, usize>,
    /// Number of communities detected
    pub num_communities: usize,
    /// Newman's modularity score Q [-0.5, 1.0]
    /// Q > 0.3 = significant community structure
    /// Q > 0.7 = strong community structure
    pub modularity_score: f64,
    /// Per-community metadata
    pub communities: Vec<CommunityInfo>,
}

pub struct CommunityInfo {
    pub id: usize,
    /// Node IDs in this community
    pub members: Vec<String>,
    /// Number of intra-community edges
    pub internal_edges: usize,
    /// Number of cross-community edges
    pub boundary_edges: usize,
    /// Internal density: internal_edges / possible_internal_edges
    pub density: f64,
    /// Aggregate risk metrics for this community
    pub aggregate_risk: CommunityRisk,
}
```

### Phase 2: Boundary Analysis

Once communities are detected, classify every edge as **internal** (both endpoints in
the same community) or **boundary** (endpoints in different communities).

#### Metrics

| Metric | Formula | Interpretation |
|--------|---------|----------------|
| **Modularity Q** | Newman's Q on the community partition | Global measure: how well the partition separates the graph. Q > 0.3 = meaningful structure. |
| **Boundary ratio** | boundary_edges / total_edges | Fraction of coupling that crosses boundaries. Lower = more contained. |
| **Risk containment** | 1 - (cross_boundary_risk / total_propagated_risk) | How much of the propagated risk stays within communities. 1.0 = fully contained. |
| **Community cohesion** | avg(internal_density per community) | How tightly coupled each community is internally. |
| **Boundary conductance** | For each boundary edge: weight / min(community_size_a, community_size_b) | How "leaky" each boundary is. |

#### Cross-Boundary Risk Flow

For each pair of communities (A, B) that share boundary edges, compute:

```
cross_risk(A→B) = sum(propagated_risk flowing from A nodes to B nodes along boundary edges)
```

This reveals which community boundaries are stress points — where risk escapes containment.

### Phase 3: Integration with Health Index

Replace the blunt λ/√N coupling modifier with a **modularity-aware modifier**:

#### Current (Spec 039):
```
coupling_modifier = 1.0 ± f(λ/√N)    // global, blunt
```

#### Proposed:
```
modularity_modifier = f(Q, risk_containment, boundary_ratio)
```

**Formula:**

```
// Base: modularity quality
modularity_base = Q * 0.6 + risk_containment * 0.4

// Convert to modifier [0.90, 1.10]
if modularity_base > 0.5:
    // Well-modularized: bonus (risk is contained)
    modifier = 1.0 + (modularity_base - 0.5) * 0.20    // up to +10%
else:
    // Poorly modularized: penalty (risk cascades)
    modifier = 1.0 - (0.5 - modularity_base) * 0.20    // down to -10%
```

**Wider range than λ/√N**: The current coupling modifier is [0.95, 1.03] — intentionally
gentle because λ/√N is a blunt proxy. Modularity is a much more precise measure of risk
containment, so it warrants a wider impact range [0.90, 1.10].

#### Caveat adjustment

Add modularity-aware caveats:

- Q < 0.2: "Weak community structure; risk propagation is not well-contained"
- Risk containment < 0.5: "Over half of propagated risk crosses community boundaries"
- Boundary ratio > 0.4: "High fraction of edges cross community boundaries"

### Phase 4: Modularity-Aware Propagation (Future)

Optional enhancement: modify the propagation step itself based on boundaries.

```
// Current: uniform damping
weight = edge_weight * damping

// Future: boundary-aware damping
if same_community(src, tgt):
    weight = edge_weight * intra_damping    // e.g., 0.6 (attenuate: contained risk)
else:
    weight = edge_weight * cross_damping    // e.g., 0.9 (amplify: boundary-crossing risk)
```

This would make cross-boundary risk propagation *stronger* (it matters more) and
intra-community propagation *weaker* (it's contained). But this changes the entire
risk field, so it needs careful validation against the benchmark set before adoption.

**Defer to Phase 4** — get the measurement right (Phases 1-3) before changing the model.

## Implementation Plan

### Files to modify/create

| File | Changes |
|------|---------|
| `ising-analysis/src/modularity.rs` | **New**: Community detection, modularity scoring, boundary analysis |
| `ising-analysis/src/lib.rs` | Add `pub mod modularity;` |
| `ising-analysis/src/stress.rs` | Integrate modularity into `compute_health_index()` |
| `ising-core/src/fea.rs` | Add `ModularityInfo` to `HealthIndex` |
| `ising-cli/src/main.rs` | Display modularity metrics in safety/health output |
| `ising-db/src/schema.rs` | Store community assignments (optional, Phase 2+) |

### New types in `ising-core/src/fea.rs`

```rust
/// Modularity analysis results for the codebase.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModularityInfo {
    /// Newman's modularity score Q [-0.5, 1.0].
    pub modularity_score: f64,
    /// Number of detected communities.
    pub num_communities: usize,
    /// Fraction of edges that cross community boundaries.
    pub boundary_ratio: f64,
    /// Fraction of propagated risk contained within communities [0, 1].
    pub risk_containment: f64,
    /// The modifier applied to health score based on modularity.
    pub modularity_modifier: f64,
}
```

### Algorithm Detail: Label Propagation

```
Input: graph G = (V, E) with weighted edges
Output: community assignments labels: V → {1..k}

1. For each v in V: labels[v] = v.id    (unique label per node)
2. order = shuffle(V)                    (random traversal order)
3. Repeat until convergence (max 100 iterations):
   a. changed = false
   b. For each v in order:
      i.  For each neighbor u of v:
            accumulate label_weights[labels[u]] += edge_weight(v, u)
      ii. new_label = argmax(label_weights)   (break ties by smallest label)
      iii. If new_label != labels[v]:
              labels[v] = new_label
              changed = true
   c. If not changed: BREAK
4. Renumber labels contiguously: {0, 1, ..., k-1}
5. Compute Newman's Q:
   Q = (1/2m) * sum_ij [ A_ij - k_i*k_j/(2m) ] * delta(labels[i], labels[j])
   where m = sum of all edge weights, k_i = weighted degree of node i
```

### Algorithm Detail: Newman's Modularity Q

```
Q = sum_c [ L_c/m - (d_c / 2m)^2 ]

Where:
  c = community
  L_c = sum of edge weights within community c
  d_c = sum of weighted degrees of nodes in community c
  m = sum of all edge weights in the graph
```

Q ranges from -0.5 to 1.0:
- Q ≈ 0: partition is no better than random
- Q > 0.3: significant community structure
- Q > 0.7: strong community structure
- Real codebases typically: 0.3 - 0.8

### Testing Strategy

1. **Unit tests** in `modularity.rs`:
   - Two disconnected cliques → Q ≈ 1.0, two communities
   - Complete graph → Q ≈ 0, one community
   - Barbell graph (two cliques connected by a bridge) → two communities, bridge is boundary
   - Single node → Q = 0, one community
   - Star graph → low Q

2. **Integration tests**:
   - Build a `UnifiedGraph` with known community structure, verify detection
   - Verify modularity modifier correctly adjusts health score

3. **Benchmark validation**:
   - Run against 5-repo minimum set (flask, gin, express, django-rest-framework, fastapi)
   - Check that modularity scores are plausible (not degenerate)
   - Verify grade changes are bounded (no repo should shift more than one grade from
     modularity alone)

## Validation Criteria

| Criterion | Threshold |
|-----------|-----------|
| Community detection finds 2+ communities on repos with 50+ modules | Always |
| Newman's Q > 0.2 for repos with clear directory structure | Most repos |
| Modularity modifier stays within [0.90, 1.10] | Always |
| No benchmark repo shifts more than 1 grade from modularity alone | Always |
| Community detection completes in < 1s for graphs with 5000 nodes | Always |

## Open Questions

1. **Directory-based fallback**: Should we always augment label propagation with directory
   structure (e.g., shared path prefix = edge weight bonus), or only use it as a fallback
   when detection fails?

2. **Granularity**: How many communities is "right"? Too few (2-3 for a 500-module codebase)
   misses internal structure. Too many (200) is noise. Should we enforce a target range
   or let the algorithm decide?

3. **CoChanges vs Imports**: The current design weights Imports at 0.6 and CoChanges at 0.4
   for community detection. Should temporal coupling matter more for identifying "real"
   subsystems vs "accidental" structural dependencies?

4. **Multi-level communities**: Real codebases have hierarchy — `src/auth/` is a community,
   but so is `src/` at a coarser level. Should we detect hierarchical communities
   (recursive Louvain) or just flat partitions?
