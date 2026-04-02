# Spec 045: Graph Neural Network Risk Model

**Status**: Draft — research phase
**Created**: 2026-04-02
**Priority**: Medium (research track)
**Depends on**: Spec 044 (modularity analysis), Spec 034 (structural force analysis)

## Motivation

The current risk model is a hand-crafted pipeline:

```
change_load → capacity → Jacobi propagation → safety_factor → zone → health_index
```

Every step uses manually-tuned parameters:
- Capacity weights: complexity×0.4 + instability×0.3 + coupling×0.3
- Damping factors: cochange_damping=0.8, structural_damping=0.7
- Zone boundaries: Critical<1.0, Danger<1.5, Warning<2.0, Healthy<3.0
- Signal weights: cycles=4x, god_modules=3x, ghost_coupling=1x
- Health formula: zone_sub × coupling_mod − signal_penalty

These are **engineering judgment**, not empirically validated (as noted in CLAUDE.md).
The ordering is defensible; the magnitudes are arbitrary. A graph neural network could
**learn** these relationships from data, potentially discovering:

1. **Non-linear interactions** the hand-tuned formula misses (e.g., high complexity is
   only dangerous when combined with high fan-out AND recent churn)
2. **Optimal propagation patterns** (which edge types matter, how much damping, whether
   propagation should be asymmetric)
3. **Implicit modularity** (GNN message passing naturally aggregates neighborhood info,
   capturing community structure without explicit detection)
4. **Cross-repo transfer** (patterns learned from one repo apply to another)

### When This Becomes Compelling

A GNN approach is premature until we have:
- A benchmark corpus of 50+ repos with defect histories (ground truth)
- Evidence that the hand-tuned formulas are hitting a performance ceiling
- Users who need predictions ("which modules will break?") not just descriptions
  ("which modules are risky right now?")

This spec is a **research design** — it defines what we'd build and what we'd need to
validate it, so we're ready when the prerequisites are met.

## Architecture

### Overview

```
                    ┌──────────────────────────────────────────┐
                    │           GNN Risk Model                 │
                    │                                          │
  UnifiedGraph ────►│  Node Features ──► Encoder ──► GNN ──►  │──► Risk Predictions
  ChangeMetrics ───►│  Edge Features ──►         (message     │    (per-node scores,
  DefectMetrics ───►│  Edge Types   ──►          passing)     │     community labels,
                    │                                          │     risk containment)
                    └──────────────────────────────────────────┘
```

### Node Feature Vector

Each module node gets a feature vector from existing computed metrics:

| Feature | Source | Range | Notes |
|---------|--------|-------|-------|
| `loc` | Node.loc | [0, ∞) | Log-normalized |
| `complexity` | Node.complexity | [0, ∞) | Log-normalized |
| `nesting_depth` | Node.nesting_depth | [0, ∞) | Raw |
| `fan_in` | compute_node_metrics | [0, ∞) | Log-normalized |
| `fan_out` | compute_node_metrics | [0, ∞) | Log-normalized |
| `cbo` | compute_node_metrics | [0, ∞) | Log-normalized |
| `instability` | fan_out / (fan_in + fan_out) | [0, 1] | Raw |
| `change_freq` | ChangeMetrics | [0, ∞) | Log-normalized |
| `churn_rate` | ChangeMetrics | [0, ∞) | Log-normalized |
| `hotspot_score` | ChangeMetrics | [0, 1] | Raw |
| `bug_count` | DefectMetrics | [0, ∞) | Log-normalized |
| `defect_density` | DefectMetrics | [0, 1] | Raw |
| `community_id` | Spec 044 modularity | One-hot | From community detection |

**Dimension**: ~15-20 features per node (before community one-hot encoding).

**Normalization**: Log-normalize unbounded features: `log(1 + x)`. This prevents
large codebases from dominating. Per-graph normalization (z-score) after log transform.

### Edge Feature Vector

| Feature | Source | Notes |
|---------|--------|-------|
| `edge_type` | EdgeType enum | One-hot: Imports, CoChanges, Calls, Contains, etc. |
| `weight` | Edge.weight | Raw edge weight |
| `is_boundary` | Spec 044 | 1 if cross-community, 0 if intra-community |
| `co_change_strength` | CoChanges weight | 0 if not a CoChanges edge |
| `structural_strength` | Imports weight | 0 if not an Import edge |

### GNN Architecture Options

#### Option A: GraphSAGE (Recommended Starting Point)

```
Layer 1: SAGE(in=node_features, out=64, aggregator=mean)
Layer 2: SAGE(in=64, out=64, aggregator=mean)
Layer 3: SAGE(in=64, out=32, aggregator=mean)
Readout: Linear(32 → 1) per node (risk score)
         Linear(32 → k) per node (community classification, optional)
```

**Why GraphSAGE?**
- Inductive: can generalize to unseen graphs (new repos) without retraining
- Sampling-based: scales to large graphs
- Simple: easy to implement and debug
- Well-studied: known properties and failure modes

#### Option B: Graph Attention Network (GAT)

```
Layer 1: GAT(in=node_features, out=64, heads=4)
Layer 2: GAT(in=64, out=64, heads=4)
Layer 3: GAT(in=64, out=32, heads=1)
Readout: Linear(32 → 1)
```

**Why GAT?**
- Attention weights reveal *which* neighbors matter most for risk — interpretability
- Can learn that CoChanges edges matter more than Imports for risk propagation
- Attention weights are inspectable: "this module is risky because of its relationship
  with *these specific* neighbors"

#### Option C: Heterogeneous GNN (HetGNN)

Model different edge types with separate message functions:

```
Message_import(src, tgt) = W_import × h_src
Message_cochange(src, tgt) = W_cochange × h_src
Message_calls(src, tgt) = W_calls × h_src

h_tgt = Aggregate(all incoming messages) + self_loop
```

**Why HetGNN?**
- Our graph is naturally heterogeneous (8 edge types, 4 node types)
- Separate weight matrices per edge type = learns different propagation per relationship
- More parameter-efficient than treating edge type as a feature

### Training Signals

This is the hardest part. What constitutes ground truth?

#### Signal 1: Defect Prediction (Primary)

**Task**: Given the graph at time T, predict which modules will have bug-fix commits
in the window [T, T+Δ].

**Label**: Binary per node — did this module appear in a bug-fix commit?

**Data source**: Git history. Identify bug-fix commits by:
- Commit message patterns: "fix", "bug", "issue", "patch", "CVE"
- Linked issue references: "#123" where issue has "bug" label
- Revert commits

**Loss**: Binary cross-entropy, weighted by class imbalance (most modules don't have bugs).

**Evaluation**: Precision@k, Recall@k, AUC-ROC. Compare against the current hand-tuned
risk ranking.

#### Signal 2: Change Prediction (Self-Supervised)

**Task**: Given a change to module A at time T, predict which other modules will also
change in the same commit or within Δ days.

**Label**: Set of co-changed modules.

**Data source**: Git history (already extracted as CoChanges edges). Hold out recent
commits for test set.

**Loss**: Binary cross-entropy per node.

**Why useful?**: If the GNN can predict change propagation better than the current
Jacobi iteration, it's learning something about risk flow that the hand-tuned model misses.

#### Signal 3: Self-Supervised Pre-Training

**Task**: Masked feature prediction — mask 15% of node features, predict them from
graph structure and remaining features.

**No labels needed**: Pure self-supervised. Learns graph structure.

**Use case**: Pre-train on many repos without labels, then fine-tune on repos with
defect data. This is the "foundation model" approach.

### Training Data Pipeline

```
For each repo in corpus:
  1. Clone repo
  2. For each time window [T-Δ, T]:
     a. Build UnifiedGraph at time T (structural snapshot)
     b. Extract ChangeMetrics from [T-Δ, T] window
     c. Extract DefectMetrics from [T-Δ, T] window
     d. Run community detection (Spec 044)
     e. Build node/edge feature matrices
     f. Extract labels: bug-fix commits in [T, T+Δ] (lookahead)
  3. Output: PyG Data object (node_features, edge_index, edge_features, labels)
```

**Time windows**: Non-overlapping 90-day windows. For a repo with 5 years of history,
this gives ~20 training samples per repo. With 50 repos, ~1000 training graphs.

### Inference Integration

The GNN would run alongside (not replace) the current hand-tuned model:

```rust
pub struct GnnRiskPrediction {
    /// GNN-predicted risk score per node [0, 1]
    pub risk_scores: HashMap<String, f64>,
    /// GNN-predicted defect probability per node [0, 1]
    pub defect_probabilities: HashMap<String, f64>,
    /// Learned community assignments (from GNN embeddings)
    pub learned_communities: HashMap<String, usize>,
    /// Attention weights for interpretability (if using GAT)
    pub attention_weights: Option<HashMap<(String, String), f64>>,
}
```

#### Rust Runtime Options

| Option | Pros | Cons |
|--------|------|------|
| **ONNX Runtime** (`ort` crate) | Fast inference, cross-platform, no Python dependency | Export complexity, limited dynamic graph support |
| **Burn** (Rust ML framework) | Pure Rust, no FFI, good for deployment | Smaller ecosystem, fewer GNN primitives |
| **PyO3 + PyTorch Geometric** | Full PyG ecosystem, easy development | Python dependency, heavier runtime |
| **Candle** (Hugging Face Rust ML) | Pure Rust, GPU support | No built-in GNN layers, would need custom impl |

**Recommended**: Train in Python (PyTorch Geometric), export to ONNX, run inference
in Rust via `ort`. This separates the training environment (rich ML ecosystem) from the
deployment environment (lean Rust binary).

## Implementation Phases

### Phase 1: Data Pipeline (No ML Yet)

Build the feature extraction pipeline that converts UnifiedGraph + metrics into
GNN-ready feature matrices. This is useful even without the GNN — it validates that
we have the right features and enough data.

**Deliverables**:
- `ising-analysis/src/gnn_features.rs`: Feature extraction from graph
- Feature matrix export to NumPy/CSV for offline analysis
- Script to process N repos and output a training dataset

### Phase 2: Baseline Model (Python)

Train a simple GNN in Python (PyTorch Geometric) on the extracted features.

**Deliverables**:
- Python training script in `scripts/train_gnn.py`
- Evaluation against current hand-tuned model: does the GNN rank risky modules
  more accurately than direct_score?
- Ablation study: which features matter most? Which edge types?

### Phase 3: Interpretability Analysis

Before deploying, understand what the GNN learned:

- **Attention analysis** (GAT): Which edges carry the most risk signal?
- **Feature importance**: Ablate each feature — which ones change predictions most?
- **Embedding visualization**: t-SNE of node embeddings — do they cluster by community?
  By risk level? By language?
- **Comparison with hand-tuned**: Where does the GNN disagree with the current model?
  Is the GNN right or is it overfitting?

### Phase 4: Rust Integration

Export the trained model and integrate into the Rust pipeline:

- ONNX export from PyTorch
- `ising-analysis/src/gnn_inference.rs`: Load ONNX model, run inference
- Feature flag: `--gnn` CLI flag to enable GNN predictions alongside hand-tuned
- Display both scores in output, let users compare

### Phase 5: Hybrid Model

Combine GNN predictions with hand-tuned model:

```
hybrid_score = α * hand_tuned_score + (1 - α) * gnn_score
```

Where α is validated against benchmark repos. The GNN should improve predictions
where the hand-tuned model is weakest (complex interactions, implicit modularity),
while the hand-tuned model provides stability and interpretability.

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Insufficient training data | GNN overfits to few repos | Self-supervised pre-training, data augmentation (subgraph sampling) |
| Ground truth quality | Bug-fix labels are noisy (not all "fix" commits fix bugs) | Use multiple labeling heuristics, measure inter-annotator agreement |
| Computational cost | GNN inference adds latency | ONNX optimization, batch inference, optional feature flag |
| Black-box predictions | Users don't trust opaque scores | GAT attention for interpretability, always show hand-tuned alongside |
| Training/serving skew | Features computed differently at train vs inference time | Share feature extraction code between Python and Rust |
| Overfitting to large repos | Corpus dominated by popular repos | Stratified sampling by size/language, per-graph normalization |

## Success Criteria

| Criterion | Target |
|-----------|--------|
| GNN defect prediction AUC-ROC > hand-tuned ranking | > 0.65 AUC |
| GNN change prediction better than Jacobi propagation | Lower MAE on held-out co-changes |
| Inference time < 2x current analysis time | < 500ms for 5000-node graph |
| At least one "insight" the GNN discovers that hand-tuned misses | Qualitative |
| No benchmark repo degrades by more than 1 grade in hybrid mode | Always |

## Open Questions

1. **How many repos do we need?** Standard ML wisdom says 10x features × classes.
   With ~20 features and binary labels, that's 400+ graphs minimum. With 50 repos
   and 20 windows each, we get 1000 — probably enough, but diversity matters more
   than count.

2. **Graph size variance**: Flask has 98 modules, kubernetes has 5000+. How do we
   handle this? Per-graph normalization helps features, but the model still sees very
   different graph structures. GraphSAGE with sampling handles this naturally.

3. **Temporal leakage**: The CoChanges edges are computed from the same time window
   we're predicting. This creates information leakage for change prediction. Need
   strict temporal splits: features from [T-2Δ, T-Δ], labels from [T-Δ, T].

4. **Is the hand-tuned model good enough?** If defect prediction AUC of the current
   direct_score ranking is already 0.75+, the GNN needs to beat that by a meaningful
   margin to justify the complexity. We need to measure the current baseline first.

5. **Single model vs ensemble**: Should we train one model across all repos (more data,
   but language/architecture differences are noise), or per-language models (less data,
   but more focused)?

6. **Edge type heterogeneity**: Our graph has 8 edge types and 4 node types. A simple
   GNN treats them uniformly. HetGNN is more principled but more complex. Which tradeoff
   is right?

## References

- **GraphSAGE**: Hamilton et al., "Inductive Representation Learning on Large Graphs" (2017)
- **GAT**: Velickovic et al., "Graph Attention Networks" (2018)
- **GNN for defect prediction**: "Software Defect Prediction via Graph Neural Networks"
  (various papers applying GNNs to code property graphs)
- **PyTorch Geometric**: Fey & Lenssen, "Fast Graph Representation Learning with PyTorch
  Geometric" (2019)
- **ONNX Runtime for Rust**: `ort` crate — https://github.com/pykeio/ort
